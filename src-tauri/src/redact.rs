//! Testado: cubre con negro partes de los PDF de la carpeta y guarda copias testadas.
//!
//! Dos modos:
//! - «Conservar la fila»: en cada tabla se testan todas las filas excepto la de la persona, que se
//!   reconoce por puntaje (palabras del nombre y número de empleado), tolerando errores de OCR.
//! - «Testar textos»: se cubre cada aparición de los textos indicados.
//!
//! El análisis propone regiones por página; el usuario las revisa o corrige y después se aplican.
//! Las páginas con regiones se vuelven a dibujar como imagen con las barras quemadas en los píxeles
//! y sin capa de texto: una barra superpuesta se podría quitar con un editor y dejaría el texto
//! debajo. Las demás páginas se copian sin cambios y los originales no se tocan.

use crate::db::{self, PAGE_BITS, PAGE_MASK};
use crate::indexer::{clean, ocr_decision, OCR_DPI, OCR_MAX_SIDE};
use crate::layout::{self, Layout};
use crate::ocr::{rotate, unrotate, Ocr, Word};
use crate::pdf::{Gray, PageImage, Pdfium};
use crate::matching::{tokens_from_text, tokens_from_words, Query, Token};
pub use crate::matching::Term;
use crate::search::{tokenize, Searcher};
use crossbeam_channel::{bounded, unbounded, Sender};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Resolución de salida cuando la página no es un escaneo (o no se puede saber la del escaneo).
const OUTPUT_DPI: f32 = 200.0;
const OUTPUT_DPI_RANGE: (f32, f32) = (150.0, 300.0);
const OUTPUT_MAX_SIDE: f32 = 5000.0;
const PREVIEW_DPI: f32 = 110.0;
const PREVIEW_MAX_SIDE: f32 = 1700.0;
const JPEG_QUALITY: u8 = 80;
/// Margen de la barra alrededor de una palabra, como fracción de la altura del renglón.
const PAD_Y: f32 = 0.25;
const PAD_X: f32 = 0.15;
/// Puntaje mínimo para reconocer la fila de la persona: dos palabras del nombre, o el número.
const MIN_ROW_SCORE: usize = 2;

/// Lo que identifica a la persona cuya fila se conserva: palabras del nombre y números.
#[derive(Clone, Default)]
pub struct Identity {
    words: Vec<Query>,
}

/// Palabras que no ayudan a distinguir a una persona.
const IGNORED: [&str; 18] = [
    "de", "del", "la", "las", "los", "y", "pdf", "lista", "listas", "asistencia", "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto",
];
const IGNORED_MORE: [&str; 4] = ["septiembre", "octubre", "noviembre", "diciembre"];

impl Identity {
    pub fn new<S: AsRef<str>>(texts: &[S]) -> Self {
        let mut seen = HashSet::new();
        let mut words = Vec::new();
        for text in texts {
            for w in tokenize(text.as_ref()) {
                let q = Query::new(&w);
                let useful = if q.digits { q.text.len() >= 4 } else { q.chars.len() >= 3 && !IGNORED.contains(&q.text.as_str()) && !IGNORED_MORE.contains(&q.text.as_str()) };
                if useful && seen.insert(q.text.clone()) {
                    words.push(q);
                }
            }
        }
        Identity { words }
    }

    /// Tomada del nombre del archivo, p. ej. «00002022195 ANDRES MARTINEZ FLORES junio.pdf».
    pub fn from_file_name(name: &str) -> Self {
        let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        Identity::new(&[stem])
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn labels(&self) -> Vec<String> {
        self.words.iter().map(|q| q.text.clone()).collect()
    }

    /// Cada palabra del nombre que aparece suma 1; el número de empleado suma 2.
    fn score(&self, tokens: &[&Token]) -> usize {
        self.words.iter().filter(|q| tokens.iter().any(|t| q.matches(&t.folded))).map(|q| if q.digits { 2 } else { 1 }).sum()
    }
}

// ── Análisis ────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Keep,
    Text,
}

#[derive(serde::Serialize, Clone)]
pub struct Region {
    /// row | text
    pub kind: &'static str,
    /// Si se testa. Las filas que quedan visibles llevan `active = false`.
    pub active: bool,
    /// La fila reconocida como la de la persona.
    pub keep: bool,
    pub label: String,
    pub rects: Vec<[f32; 4]>,
}

#[derive(serde::Serialize, Clone)]
pub struct WordBox {
    pub text: String,
    pub rect: [f32; 4],
}

#[derive(serde::Serialize, Clone)]
pub struct PageAnalysis {
    pub page: usize,
    /// ok | review | empty
    pub status: &'static str,
    pub note: String,
    pub regions: Vec<Region>,
    pub words: Vec<WordBox>,
    /// Textos (por posición en la solicitud) encontrados en la página.
    #[serde(skip)]
    found: Vec<usize>,
}

#[derive(serde::Serialize, Clone)]
pub struct DocAnalysis {
    pub id: i64,
    pub name: String,
    pub rel: String,
    pub pages: Vec<PageAnalysis>,
    pub error: Option<String>,
}

/// Documento a analizar con la identidad de su persona (modo «Conservar la fila»).
#[derive(serde::Serialize, Clone)]
pub struct Candidate {
    pub id: i64,
    pub name: String,
    pub rel: String,
    pub pages: i64,
    /// Palabras con que se reconocerá la fila de la persona.
    pub identity: Vec<String>,
    #[serde(skip)]
    pub ident: Identity,
}

/// Documentos de la carpeta a los que aplica la solicitud. En «Conservar la fila» con identidad
/// por nombre de archivo son todos; en otro caso, los que mencionan los textos según el índice.
pub fn candidates(searcher: &mut Searcher, mode: Mode, texts: &[String], from_file_name: bool) -> Result<Vec<Candidate>, String> {
    let mut files: Vec<(i64, String, i64)> = Vec::new();
    {
        let mut stmt = searcher.conn().prepare("SELECT id, rel, pages FROM files WHERE error IS NULL ORDER BY rel COLLATE NOCASE").map_err(db::err)?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(db::err)?;
        files.extend(rows.flatten());
    }
    let make = |(id, rel, pages): (i64, String, i64), ident: Identity| {
        let name = Path::new(&rel).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| rel.clone());
        Candidate { id, name, rel, pages, identity: ident.labels(), ident }
    };

    if mode == Mode::Keep && from_file_name {
        return Ok(files
            .into_iter()
            .filter_map(|f| {
                let ident = Identity::from_file_name(&f.1);
                (!ident.is_empty()).then(|| make(f, ident))
            })
            .collect());
    }

    // Páginas que contienen alguna de las palabras (o variantes cercanas); luego se confirma
    let identity = Identity::new(texts);
    let terms: Vec<Term> = texts.iter().filter_map(|t| Term::new(t)).collect();
    let mut groups = Vec::new();
    for q in identity.words.iter().chain(terms.iter().flat_map(|t| t.words.iter())) {
        let options = if q.max == 0 { vec![q.text.clone()] } else { searcher.variants(&q.text, q.max)? };
        groups.extend(options.into_iter().map(|t| format!("\"{}\"", t.replace('"', ""))));
    }
    if groups.is_empty() {
        return Ok(Vec::new());
    }
    groups.sort();
    groups.dedup();
    let mut matched = HashSet::new();
    {
        let mut stmt = searcher.conn().prepare("SELECT rowid, text FROM fts WHERE fts MATCH ?1").map_err(db::err)?;
        let mut rows = stmt.query([groups.join(" OR ")]).map_err(db::err)?;
        while let Some(row) = rows.next().map_err(db::err)? {
            let rowid: i64 = row.get(0).map_err(db::err)?;
            if rowid & PAGE_MASK == 0 {
                continue;
            }
            let text: String = row.get(1).map_err(db::err)?;
            let tokens = tokens_from_text(&text);
            let hit = match mode {
                Mode::Text => terms.iter().any(|t| !t.find(&tokens).is_empty()),
                Mode::Keep => identity.score(&tokens.iter().collect::<Vec<_>>()) >= MIN_ROW_SCORE,
            };
            if hit {
                matched.insert(rowid >> PAGE_BITS);
            }
        }
    }
    Ok(files.into_iter().filter(|f| matched.contains(&f.0)).map(|f| make(f, identity.clone())).collect())
}

#[derive(serde::Serialize, Clone, Default)]
pub struct Progress {
    /// analyzing | saving | done | cancelled | error
    pub phase: &'static str,
    pub total_files: usize,
    pub done_files: usize,
    pub total_pages: usize,
    pub done_pages: usize,
    pub current: String,
    pub elapsed_ms: u64,
    pub message: String,
    pub report: Option<Report>,
}

struct Job {
    page: usize,
    gray: Gray,
    text_words: Vec<Word>,
    needs_ocr: bool,
    reply: Sender<(usize, Result<PageAnalysis, String>)>,
}

pub struct Plan<'a> {
    pub mode: Mode,
    pub terms: &'a [Term],
}

/// Analiza cada documento y entrega su propuesta con `on_doc` en cuanto termina.
pub fn analyze(
    pdfium: &Pdfium,
    pdf_lock: &Mutex<()>,
    folder: &Path,
    docs: &[Candidate],
    plan: &Plan,
    cancel: &AtomicBool,
    emit: &dyn Fn(&Progress),
    on_doc: &dyn Fn(DocAnalysis),
) -> Progress {
    let started = Instant::now();
    let mut p = Progress { phase: "analyzing", total_files: docs.len(), total_pages: docs.iter().map(|d| d.pages as usize).sum(), ..Default::default() };
    emit(&p);
    let ocr_ready = Ocr::new().is_ok();
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).max(1);
    let (job_tx, job_rx) = bounded::<(Job, Identity)>(workers);
    let mut last_emit = Instant::now();

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let rx = job_rx.clone();
            scope.spawn(move || {
                let engine = if ocr_ready { Ocr::new().ok() } else { None };
                for (job, ident) in rx {
                    let result = analyze_page(job.page, job.gray, job.text_words, job.needs_ocr, engine.as_ref(), plan, &ident);
                    let _ = job.reply.send((job.page, result));
                }
            });
        }
        drop(job_rx);
        let job_tx = job_tx;

        for doc in docs {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            p.current = doc.rel.clone();
            emit(&p);
            let mut result = DocAnalysis { id: doc.id, name: doc.name.clone(), rel: doc.rel.clone(), pages: Vec::new(), error: None };
            let mut on_page = |p: &mut Progress| {
                p.done_pages += 1;
                if last_emit.elapsed().as_millis() >= 150 {
                    p.elapsed_ms = started.elapsed().as_millis() as u64;
                    emit(p);
                    last_emit = Instant::now();
                }
            };
            match analyze_doc(pdfium, pdf_lock, &folder.join(&doc.rel), doc, &job_tx, cancel, &mut p, &mut on_page, plan.terms) {
                Ok(pages) => result.pages = pages,
                Err(e) => result.error = Some(e),
            }
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            on_doc(result);
            p.done_files += 1;
        }
    });

    p.current.clear();
    p.elapsed_ms = started.elapsed().as_millis() as u64;
    p.phase = if cancel.load(Ordering::Relaxed) { "cancelled" } else { "done" };
    p
}

#[allow(clippy::too_many_arguments)]
fn analyze_doc(
    pdfium: &Pdfium,
    pdf_lock: &Mutex<()>,
    source: &Path,
    doc: &Candidate,
    jobs: &Sender<(Job, Identity)>,
    cancel: &AtomicBool,
    p: &mut Progress,
    on_page: &mut dyn FnMut(&mut Progress),
    terms: &[Term],
) -> Result<Vec<PageAnalysis>, String> {
    let data = std::fs::read(source).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let ocr_side = OCR_MAX_SIDE.min(Ocr::max_side());
    let count = {
        let _guard = pdf_lock.lock().unwrap();
        pdfium.open(&data)?.page_count()
    };
    let (reply_tx, reply_rx) = unbounded();
    let mut sent = 0;
    for i in 0..count {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelado".into());
        }
        // PDFium se usa por página y con candado, para que la vista previa pueda dibujar mientras tanto
        let (gray, text_words, needs_ocr) = {
            let _guard = pdf_lock.lock().unwrap();
            let pdf = pdfium.open(&data)?;
            let page = pdf.page(i)?;
            let text = clean(&page.text());
            let (needs_ocr, _) = ocr_decision(&page, &text);
            (page.render_gray(OCR_DPI, ocr_side)?, page.words(), needs_ocr)
        };
        let job = Job { page: i + 1, gray, text_words, needs_ocr, reply: reply_tx.clone() };
        jobs.send((job, doc.ident.clone())).map_err(|_| "El análisis se detuvo".to_string())?;
        sent += 1;
    }
    drop(reply_tx);
    let mut pages: Vec<PageAnalysis> = Vec::with_capacity(sent);
    for (page, result) in reply_rx.iter().take(sent) {
        on_page(p);
        pages.push(result.unwrap_or_else(|e| PageAnalysis { page, status: "review", note: e, regions: Vec::new(), words: Vec::new(), found: Vec::new() }));
    }
    pages.sort_by_key(|pa| pa.page);
    flag_missing(&mut pages, terms);
    Ok(pages)
}

/// Un texto que aparece en la mayoría de las hojas de un documento (p. ej. el mismo nombre en cada
/// lista) y falta en alguna, seguramente no se leyó bien ahí: esa hoja se marca para revisarla.
fn flag_missing(pages: &mut [PageAnalysis], terms: &[Term]) {
    for (n, term) in terms.iter().enumerate() {
        let text = &term.label;
        let total = pages.len();
        let present = pages.iter().filter(|p| p.found.contains(&n)).count();
        if present * 2 < total || present == total {
            continue;
        }
        for p in pages.iter_mut().filter(|p| !p.found.contains(&n)) {
            p.status = "review";
            let msg = format!("«{text}» aparece en {present} de {} hojas, pero aquí no se encontró. Si está en la hoja, márcalo a mano.", total);
            p.note = if p.note.is_empty() { msg } else { format!("{} {msg}", p.note) };
        }
    }
}

fn analyze_page(
    page: usize,
    gray: Gray,
    text_words: Vec<Word>,
    needs_ocr: bool,
    ocr: Option<&Ocr>,
    plan: &Plan,
    ident: &Identity,
) -> Result<PageAnalysis, String> {
    let (words, turns) = page_words(&gray, text_words, needs_ocr, ocr)?;
    let tokens = tokens_from_words(&words);
    let (upright, uw, uh) = if turns == 0 {
        (std::borrow::Cow::Borrowed(&gray.pixels[..]), gray.width as usize, gray.height as usize)
    } else {
        let (px, w, h) = rotate(&gray.pixels, gray.width as usize, gray.height as usize, turns);
        (std::borrow::Cow::Owned(px), w, h)
    };
    let aspect = uh as f32 / uw as f32;

    let mut regions = Vec::new();
    let mut found = Vec::new();
    let (status, note) = match plan.mode {
        Mode::Keep => {
            let layout = layout::detect(&upright, uw, uh);
            keep_row(&layout, &tokens, ident, aspect, &mut regions)
        }
        Mode::Text => {
            for (n, term) in plan.terms.iter().enumerate() {
                for (s, e) in term.find(&tokens) {
                    let label = words_text(&tokens[s..e]);
                    regions.push(Region { kind: "text", active: true, keep: false, label, rects: bars(&tokens[s..e], aspect) });
                    if !found.contains(&n) {
                        found.push(n);
                    }
                }
            }
            if regions.is_empty() {
                ("empty", String::new())
            } else {
                ("ok", String::new())
            }
        }
    };

    // Todo de vuelta a la orientación de la página
    for r in &mut regions {
        for rect in &mut r.rects {
            *rect = unrotate(*rect, turns);
        }
    }
    let words = words.into_iter().map(|w| WordBox { rect: unrotate(w.rect, turns), text: w.text }).collect();
    Ok(PageAnalysis { page, status, note, regions, words, found })
}

/// Palabras de la página con posición, en la orientación en que el texto se lee derecho, y los
/// cuartos de vuelta que hubo que girarla para leerla.
pub(crate) fn page_words(gray: &Gray, text_words: Vec<Word>, needs_ocr: bool, ocr: Option<&Ocr>) -> Result<(Vec<Word>, u8), String> {
    if !needs_ocr {
        return Ok((text_words, 0));
    }
    let engine = ocr.ok_or("Este equipo no tiene OCR de Windows para leer la página")?;
    let reading = engine.read_any_orientation(&gray.pixels, gray.width, gray.height)?;
    // La capa de texto está en la orientación original
    let mut words = if reading.quarter_turns != 0 { Vec::new() } else { text_words };
    words.extend(reading.words);
    Ok((words, reading.quarter_turns))
}

/// Propone testar todas las filas de datos y dejar visible la de la persona.
fn keep_row(layout: &Layout, tokens: &[Token], ident: &Identity, aspect: f32, regions: &mut Vec<Region>) -> (&'static str, String) {
    if layout.tables.is_empty() {
        return ("review", "No se detectó una tabla en esta página. Revísala y marca a mano lo que deba testarse.".into());
    }
    // Palabras de cada fila según su centro. Las que quedan a la altura de una fila pero fuera de
    // los bordes detectados también se cubren con ella, por si el borde se detectó corto.
    let mut rows: BTreeMap<(usize, usize), Vec<&Token>> = BTreeMap::new();
    let mut outside: BTreeMap<(usize, usize), Vec<Token>> = BTreeMap::new();
    for t in tokens {
        let Some(r) = t.rect else { continue };
        let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
        if let Some(key) = layout.band_at(cx, cy, false) {
            rows.entry(key).or_default().push(t);
        } else if let Some(key) = layout.band_at(cx, cy, true) {
            outside.entry(key).or_default().push(Token { folded: t.folded.clone(), rect: t.rect, span: t.span });
        }
    }
    // La fila con más puntaje (y sin empate) es la de la persona; el encabezado (fila 0) no cuenta
    let mut scored: Vec<((usize, usize), usize)> =
        rows.iter().filter(|(k, _)| k.1 > 0).map(|(k, toks)| (*k, ident.score(toks))).filter(|(_, s)| *s >= MIN_ROW_SCORE).collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    let person = match scored.as_slice() {
        [] => None,
        [only] => Some(only.0),
        [first, second, ..] if first.1 > second.1 => Some(first.0),
        _ => None,
    };
    let ambiguous = person.is_none() && scored.len() > 1;

    for (t, table) in layout.tables.iter().enumerate() {
        for (b, band) in table.bands.iter().enumerate().skip(1) {
            let keep = person == Some((t, b));
            let label = rows.get(&(t, b)).map(|toks| words_text_refs(toks)).unwrap_or_default();
            let mut rects = layout.band_rects(band);
            if let Some(extra) = outside.get(&(t, b)) {
                for tok in extra {
                    rects.extend(bars(std::slice::from_ref(tok), aspect));
                }
            }
            regions.push(Region { kind: "row", active: !keep, keep, label, rects });
        }
    }
    match person {
        Some(_) => ("ok", String::new()),
        None if ambiguous => ("review", "Varias filas se parecen a la persona; todas quedaron testadas. Haz clic en la fila correcta para dejarla visible.".into()),
        None => ("review", "No se encontró la fila de la persona; todas las filas quedaron testadas. Si está en la hoja, haz clic en su fila para dejarla visible.".into()),
    }
}

fn words_text(tokens: &[Token]) -> String {
    words_text_refs(&tokens.iter().collect::<Vec<_>>())
}

fn words_text_refs(tokens: &[&Token]) -> String {
    let s = tokens.iter().map(|t| t.folded.as_str()).collect::<Vec<_>>().join(" ");
    s.chars().take(90).collect()
}

/// Barras para una aparición: una por renglón, con margen alrededor de las letras.
pub(crate) fn bars(tokens: &[Token], aspect: f32) -> Vec<[f32; 4]> {
    let mut out: Vec<[f32; 4]> = Vec::new();
    for r in tokens.iter().filter_map(|t| t.rect) {
        match out.last_mut() {
            Some(last) if r[1] < last[3] && r[3] > last[1] => {
                *last = [last[0].min(r[0]), last[1].min(r[1]), last[2].max(r[2]), last[3].max(r[3])];
            }
            _ => out.push(r),
        }
    }
    for r in &mut out {
        let line = r[3] - r[1];
        let (px, py) = (line * PAD_X * aspect, line * PAD_Y);
        *r = [(r[0] - px).max(0.0), (r[1] - py).max(0.0), (r[2] + px).min(1.0), (r[3] + py).min(1.0)];
    }
    out
}

// ── Vista previa y aplicación ───────────────────────────────────────────────

/// Imagen JPEG de una página para revisarla en pantalla.
pub fn preview_image(pdfium: &Pdfium, pdf_lock: &Mutex<()>, source: &Path, page: usize) -> Result<Vec<u8>, String> {
    let data = std::fs::read(source).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let color = {
        let _guard = pdf_lock.lock().unwrap();
        let pdf = pdfium.open(&data)?;
        let p = pdf.page(page.checked_sub(1).ok_or("Página inválida")?)?;
        p.render_color(PREVIEW_DPI, PREVIEW_MAX_SIDE)?
    };
    encode_jpeg(&color.pixels, color.width, color.height, 72)
}

/// Lo que el usuario confirmó para un documento: páginas (base 1) y sus rectángulos.
#[derive(serde::Deserialize)]
pub struct ApplyDoc {
    pub id: i64,
    pub pages: Vec<ApplyPage>,
}

#[derive(serde::Deserialize)]
pub struct ApplyPage {
    pub page: usize,
    pub rects: Vec<[f32; 4]>,
}

#[derive(serde::Serialize, Clone, Default)]
pub struct FileReport {
    pub name: String,
    pub rel: String,
    /// Ruta del PDF testado; vacía si no se pudo generar.
    pub output: String,
    pub pages: usize,
    pub error: Option<String>,
}

#[derive(serde::Serialize, Clone, Default)]
pub struct Report {
    pub output_dir: String,
    pub files: Vec<FileReport>,
}

/// Escribe las copias testadas exactamente con los rectángulos confirmados.
pub fn apply(
    pdfium: &Pdfium,
    pdf_lock: &Mutex<()>,
    folder: &Path,
    output: &Path,
    docs: &[(String, ApplyDoc)],
    cancel: &AtomicBool,
    emit: &dyn Fn(&Progress),
) -> Progress {
    let started = Instant::now();
    let mut p = Progress {
        phase: "saving",
        total_files: docs.len(),
        total_pages: docs.iter().map(|(_, d)| d.pages.iter().filter(|pg| !pg.rects.is_empty()).count()).sum(),
        ..Default::default()
    };
    let mut report = Report { output_dir: output.to_string_lossy().to_string(), files: Vec::new() };
    emit(&p);
    for (rel, doc) in docs {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        p.current = rel.clone();
        p.elapsed_ms = started.elapsed().as_millis() as u64;
        emit(&p);
        let name = Path::new(rel).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| rel.clone());
        let mut file = FileReport { name, rel: rel.clone(), ..Default::default() };
        let out_path = output.join(rel);
        match apply_doc(pdfium, pdf_lock, &folder.join(rel), &out_path, doc, cancel, &mut p, emit, started) {
            Ok(pages) => {
                file.pages = pages;
                file.output = out_path.to_string_lossy().to_string();
            }
            Err(e) => file.error = Some(e),
        }
        report.files.push(file);
        p.done_files += 1;
    }
    p.current.clear();
    p.elapsed_ms = started.elapsed().as_millis() as u64;
    p.phase = if cancel.load(Ordering::Relaxed) { "cancelled" } else { "done" };
    p.report = Some(report);
    p
}

#[allow(clippy::too_many_arguments)]
fn apply_doc(
    pdfium: &Pdfium,
    pdf_lock: &Mutex<()>,
    source: &Path,
    out_path: &Path,
    doc: &ApplyDoc,
    cancel: &AtomicBool,
    p: &mut Progress,
    emit: &dyn Fn(&Progress),
    started: Instant,
) -> Result<usize, String> {
    let data = std::fs::read(source).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let mut images = BTreeMap::new();
    for pg in doc.pages.iter().filter(|pg| !pg.rects.is_empty()) {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelado".into());
        }
        let i = pg.page.checked_sub(1).ok_or("Página inválida")?;
        let (mut color, size) = {
            let _guard = pdf_lock.lock().unwrap();
            let pdf = pdfium.open(&data)?;
            if i >= pdf.page_count() {
                return Err(format!("La página {} ya no existe; vuelve a cargar la carpeta.", pg.page));
            }
            let page = pdf.page(i)?;
            let dpi = page.scan_dpi().unwrap_or(OUTPUT_DPI).clamp(OUTPUT_DPI_RANGE.0, OUTPUT_DPI_RANGE.1);
            (page.render_color(dpi, OUTPUT_MAX_SIDE)?, page.size())
        };
        for r in &pg.rects {
            fill(&mut color.pixels, color.width, color.height, 3, r);
        }
        let jpeg = encode_jpeg(&color.pixels, color.width, color.height, JPEG_QUALITY)?;
        images.insert(i, PageImage { jpeg, page_width: size.0, page_height: size.1 });
        p.done_pages += 1;
        p.elapsed_ms = started.elapsed().as_millis() as u64;
        emit(p);
    }
    let pdf = {
        let _guard = pdf_lock.lock().unwrap();
        pdfium.open(&data)?.rebuild(&images)?
    };
    if let Some(dir) = out_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear la carpeta de salida: {e}"))?;
    }
    let tmp = out_path.with_extension("pdf.tmp");
    std::fs::write(&tmp, pdf).map_err(|e| format!("No se pudo guardar: {e}"))?;
    std::fs::rename(&tmp, out_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("No se pudo guardar (¿el PDF testado está abierto?): {e}")
    })?;
    Ok(images.len())
}

/// Pinta de negro un rectángulo dado en fracciones de la página.
fn fill(pixels: &mut [u8], width: i32, height: i32, bpp: usize, r: &[f32; 4]) {
    let (w, h) = (width as f32, height as f32);
    let x0 = (r[0].clamp(0.0, 1.0) * w).floor() as usize;
    let x1 = (r[2].clamp(0.0, 1.0) * w).ceil() as usize;
    let y0 = (r[1].clamp(0.0, 1.0) * h).floor() as usize;
    let y1 = (r[3].clamp(0.0, 1.0) * h).ceil() as usize;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let stride = width as usize * bpp;
    for y in y0..y1.min(height as usize) {
        let row = &mut pixels[y * stride..(y + 1) * stride];
        row[x0 * bpp..x1.min(width as usize) * bpp].fill(0);
    }
}

/// JPEG en gris si la página no tiene color (escaneos en blanco y negro), si no a color.
pub(crate) fn encode_jpeg(bgr: &[u8], width: i32, height: i32, quality: u8) -> Result<Vec<u8>, String> {
    use jpeg_encoder::{ColorType, Encoder};
    let is_gray = bgr.chunks_exact(3).step_by(7).all(|p| {
        let (b, g, r) = (p[0] as i16, p[1] as i16, p[2] as i16);
        (b - g).abs() <= 12 && (g - r).abs() <= 12
    });
    let mut out = Vec::new();
    let encoder = Encoder::new(&mut out, quality);
    let (w, h) = (width as u16, height as u16);
    let result = if is_gray {
        let luma: Vec<u8> = bgr.chunks_exact(3).map(|p| ((p[0] as u16 + 2 * p[1] as u16 + p[2] as u16) / 4) as u8).collect();
        encoder.encode(&luma, w, h, ColorType::Luma)
    } else {
        encoder.encode(bgr, w, h, ColorType::Bgr)
    };
    result.map_err(|e| format!("No se pudo codificar la imagen: {e}"))?;
    Ok(out)
}

/// Carpeta de salida sugerida: junto a la de origen, con «- testados» al final.
pub fn default_output(folder: &Path) -> PathBuf {
    let name = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "documentos".into());
    folder.parent().unwrap_or(folder).join(format!("{name} - testados"))
}
