//! Recorre la carpeta, extrae el texto de cada PDF (capa de texto u OCR) y lo guarda en el índice.
//! Es incremental: los archivos sin cambios desde la última carga no se vuelven a procesar.

use crate::db::{self, PAGE_BITS, PAGE_MASK};
use crate::ocr::Ocr;
use crate::pdf::{Gray, Page, Pdfium};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const OCR_DPI: f32 = 300.0;
pub const OCR_MAX_SIDE: f32 = 4200.0;
/// Con al menos este texto real en la página se confía en la capa de texto.
const ENOUGH_TEXT: usize = 200;
/// Por debajo de esto la página se considera sin texto.
const NO_TEXT: usize = 20;
/// Cobertura de imagen a partir de la cual la página se trata como escaneo.
const SCAN_COVERAGE: f32 = 0.3;

#[derive(serde::Serialize, Clone, Default)]
pub struct Progress {
    /// scanning | indexing | optimizing | done | cancelled | error
    pub phase: &'static str,
    pub total: usize,
    pub done: usize,
    pub unchanged: usize,
    pub errors: usize,
    pub ocr_pages: usize,
    pub current: String,
    pub elapsed_ms: u64,
    pub message: String,
}

struct Job {
    page: usize,
    gray: Gray,
    reply: Sender<(usize, Result<String, String>)>,
}

struct Extracted {
    pages: Vec<String>,
    ocr_pages: u32,
    ocr_pending: u32,
}

struct Found {
    rel: String,
    size: i64,
    mtime: i64,
}

/// `pdf_lock` se toma en cada página para que el visor pueda usar PDFium durante la carga.
pub fn run(pdfium: &Pdfium, pdf_lock: &Mutex<()>, folder: &Path, db_path: &Path, cancel: &AtomicBool, emit: &dyn Fn(&Progress)) -> Progress {
    let started = Instant::now();
    let mut p = Progress { phase: "scanning", ..Default::default() };
    emit(&p);
    let result = index(pdfium, pdf_lock, folder, db_path, cancel, emit, &mut p, started);
    p.current.clear();
    p.elapsed_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(()) if cancel.load(Ordering::Relaxed) => p.phase = "cancelled",
        Ok(()) => p.phase = "done",
        Err(e) => {
            p.phase = "error";
            p.message = e;
        }
    }
    emit(&p);
    p
}

fn index(
    pdfium: &Pdfium,
    pdf_lock: &Mutex<()>,
    folder: &Path,
    db_path: &Path,
    cancel: &AtomicBool,
    emit: &dyn Fn(&Progress),
    p: &mut Progress,
    started: Instant,
) -> Result<(), String> {
    let mut conn = db::open(db_path)?;

    // 1. Archivos en disco
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(folder).follow_links(false).into_iter().flatten() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let path = entry.path();
        if !entry.file_type().is_file() || !path.extension().map(|e| e.eq_ignore_ascii_case("pdf")).unwrap_or(false) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(rel) = path.strip_prefix(folder) else { continue };
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
        found.push(Found { rel: rel.to_string_lossy().to_string(), size: meta.len() as i64, mtime });
    }
    found.sort_by(|a, b| a.rel.cmp(&b.rel));

    // 2. Comparación con el índice
    struct Known {
        id: i64,
        size: i64,
        mtime: i64,
        retry: bool,
        seen: bool,
    }
    let mut known: HashMap<String, Known> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT rel, id, size, mtime, error IS NOT NULL OR ocr_pending > 0 FROM files").map_err(db::err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, Known { id: r.get(1)?, size: r.get(2)?, mtime: r.get(3)?, retry: r.get(4)?, seen: false })))
            .map_err(db::err)?;
        for row in rows {
            let (rel, k) = row.map_err(db::err)?;
            known.insert(rel, k);
        }
    }
    let mut todo: Vec<(Found, Option<i64>)> = Vec::new();
    for f in found {
        match known.get_mut(&f.rel) {
            Some(k) => {
                k.seen = true;
                if k.size != f.size || k.mtime != f.mtime || k.retry {
                    let id = k.id;
                    todo.push((f, Some(id)));
                } else {
                    p.unchanged += 1;
                }
            }
            None => todo.push((f, None)),
        }
    }
    let gone: Vec<i64> = known.values().filter(|k| !k.seen).map(|k| k.id).collect();
    if !gone.is_empty() {
        let tx = conn.transaction().map_err(db::err)?;
        for id in &gone {
            delete_text(&tx, *id)?;
            tx.execute("DELETE FROM files WHERE id = ?1", [id]).map_err(db::err)?;
        }
        tx.commit().map_err(db::err)?;
    }

    p.phase = "indexing";
    p.total = todo.len();
    emit(p);

    // 3. Extracción: PDFium en este hilo, OCR en un grupo de hilos
    let ocr_ready = Ocr::new().is_ok();
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).max(1);
    let (job_tx, job_rx) = bounded::<Job>(workers);
    let mut last_emit = Instant::now();

    std::thread::scope(|scope| -> Result<(), String> {
        if ocr_ready {
            for _ in 0..workers {
                let rx: Receiver<Job> = job_rx.clone();
                scope.spawn(move || {
                    let engine = Ocr::new();
                    for job in rx {
                        let text = match &engine {
                            Ok(e) => e.read_any_orientation(&job.gray.pixels, job.gray.width, job.gray.height).map(|r| r.text),
                            Err(e) => Err(e.clone()),
                        };
                        let _ = job.reply.send((job.page, text));
                    }
                });
            }
        }
        drop(job_rx);
        let job_tx = job_tx; // se cierra al salir del scope y detiene a los hilos de OCR
        let ocr_tx = ocr_ready.then_some(&job_tx);

        for (f, existing) in &todo {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            p.current = f.rel.clone();
            if last_emit.elapsed().as_millis() >= 120 {
                p.elapsed_ms = started.elapsed().as_millis() as u64;
                emit(p);
                last_emit = Instant::now();
            }
            let extracted = extract(pdfium, pdf_lock, &folder.join(&f.rel), ocr_tx, cancel);
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            match &extracted {
                Ok(x) => p.ocr_pages += x.ocr_pages as usize,
                Err(_) => p.errors += 1,
            }
            store(&mut conn, *existing, f, extracted)?;
            p.done += 1;
        }
        Ok(())
    })?;

    if p.done > 0 && !cancel.load(Ordering::Relaxed) {
        p.phase = "optimizing";
        p.current.clear();
        emit(p);
        conn.execute("INSERT INTO fts(fts) VALUES('optimize')", []).map_err(db::err)?;
    }
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    Ok(())
}

fn extract(pdfium: &Pdfium, pdf_lock: &Mutex<()>, path: &Path, ocr: Option<&Sender<Job>>, cancel: &AtomicBool) -> Result<Extracted, String> {
    let data = std::fs::read(path).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let doc = {
        let _guard = pdf_lock.lock().unwrap();
        pdfium.open(&data)?
    };
    // El documento también se cierra con el candado tomado
    let close = |doc| {
        let _guard = pdf_lock.lock().unwrap();
        drop(doc);
    };
    let count = doc.page_count().min(PAGE_MASK as usize);
    let mut out = Extracted { pages: vec![String::new(); count], ocr_pages: 0, ocr_pending: 0 };
    let (reply_tx, reply_rx) = unbounded();
    let max_side = OCR_MAX_SIDE.min(Ocr::max_side());

    for i in 0..count {
        if cancel.load(Ordering::Relaxed) {
            close(doc);
            return Err("Cancelado".into());
        }
        let (text, needs_ocr, keep_text, gray) = {
            let _guard = pdf_lock.lock().unwrap();
            let Ok(page) = doc.page(i) else { continue };
            let text = clean(&page.text());
            let (needs_ocr, keep_text) = ocr_decision(&page, &text);
            let gray = (needs_ocr && ocr.is_some()).then(|| page.render_gray(OCR_DPI, max_side));
            (text, needs_ocr, keep_text, gray)
        };
        out.pages[i] = if keep_text { text } else { String::new() };
        if !needs_ocr {
            continue;
        }
        match (ocr, gray) {
            (Some(tx), Some(Ok(gray))) => {
                // El canal acotado frena el dibujado cuando el OCR va atrasado
                let _ = tx.send(Job { page: i, gray, reply: reply_tx.clone() });
            }
            _ => out.ocr_pending += 1,
        }
    }
    close(doc);
    drop(reply_tx);
    for (i, result) in reply_rx {
        match result {
            Ok(text) => {
                out.ocr_pages += 1;
                let text = clean(&text);
                if !out.pages[i].is_empty() {
                    out.pages[i].push('\n');
                }
                out.pages[i].push_str(&text);
            }
            Err(_) => out.ocr_pending += 1,
        }
    }
    Ok(out)
}

fn store(conn: &mut Connection, existing: Option<i64>, f: &Found, extracted: Result<Extracted, String>) -> Result<(), String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let tx = conn.transaction().map_err(db::err)?;
    let (pages, ocr_pages, ocr_pending, error) = match &extracted {
        Ok(x) => (x.pages.len() as i64, x.ocr_pages, x.ocr_pending, None),
        Err(e) => (0, 0, 0, Some(e.as_str())),
    };
    let id = match existing {
        Some(id) => {
            delete_text(&tx, id)?;
            tx.execute(
                "UPDATE files SET size=?2, mtime=?3, pages=?4, ocr_pages=?5, ocr_pending=?6, error=?7, indexed_at=?8 WHERE id=?1",
                params![id, f.size, f.mtime, pages, ocr_pages, ocr_pending, error, now],
            )
            .map_err(db::err)?;
            id
        }
        None => {
            tx.execute(
                "INSERT INTO files (rel, size, mtime, pages, ocr_pages, ocr_pending, error, indexed_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![f.rel, f.size, f.mtime, pages, ocr_pages, ocr_pending, error, now],
            )
            .map_err(db::err)?;
            tx.last_insert_rowid()
        }
    };
    {
        let mut ins = tx.prepare_cached("INSERT INTO fts (rowid, text) VALUES (?1, ?2)").map_err(db::err)?;
        let name = Path::new(&f.rel).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        ins.execute(params![id << PAGE_BITS, name]).map_err(db::err)?;
        if let Ok(x) = &extracted {
            for (i, text) in x.pages.iter().enumerate() {
                if !text.trim().is_empty() {
                    ins.execute(params![(id << PAGE_BITS) | (i as i64 + 1), text]).map_err(db::err)?;
                }
            }
        }
    }
    tx.commit().map_err(db::err)
}

fn delete_text(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM fts WHERE rowid BETWEEN ?1 AND ?2", params![id << PAGE_BITS, (id << PAGE_BITS) | PAGE_MASK])
        .map(|_| ())
        .map_err(db::err)
}

/// Decide si la página pasa por OCR y si su capa de texto vale la pena conservarse.
/// El testado usa la misma regla para leer cada página exactamente como se indexó.
pub fn ocr_decision(page: &Page, text: &str) -> (bool, bool) {
    let real = real_chars(text);
    let needs_ocr = real < ENOUGH_TEXT && (page.image_coverage() >= SCAN_COVERAGE || (real < NO_TEXT && page.has_objects()));
    (needs_ocr, real >= NO_TEXT || !needs_ocr)
}

/// Letras y dígitos reales; descarta el texto ilegible de fuentes sin mapa Unicode.
fn real_chars(text: &str) -> usize {
    let mut good = 0usize;
    let mut bad = 0usize;
    for c in text.chars() {
        if c.is_alphanumeric() {
            good += 1;
        } else if c == '\u{FFFD}' || ('\u{E000}'..='\u{F8FF}').contains(&c) {
            bad += 1;
        }
    }
    if bad * 3 > good {
        0
    } else {
        good
    }
}

/// Normaliza saltos de línea y une las palabras cortadas con guion al final del renglón.
pub fn clean(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().filter(|c| *c != '\r').collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let after = chars.get(i + 2).copied();
        // PDFium marca con U+0002 el guion de corte de palabra
        let soft = c == '\u{2}';
        let hard = (c == '-' || c == '\u{AD}') && i > 0 && chars[i - 1].is_alphabetic();
        if (soft || hard) && next == Some('\n') && after.map(|a| a.is_lowercase()).unwrap_or(false) {
            i += 2;
            continue;
        }
        out.push(if soft || c == '\0' || c == '\u{FFFE}' || c == '\u{FFFF}' { ' ' } else { c });
        i += 1;
    }
    out
}
