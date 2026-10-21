//! Herramienta de consola para probar el motor sin la interfaz.
//!   cli index  <carpeta> <indice.db>
//!   cli search <indice.db> <consulta>
//!   cli keep   <indice.db> <carpeta> <salida>
//!   cli text   <indice.db> <carpeta> <salida> <texto1> | <texto2>

use ocr_documentos_lib::{db, indexer, load_pdfium, redact, search::Searcher};
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("index") => {
            let pdfium = load_pdfium(&std::env::temp_dir().join("ocr-documentos-cli")).expect("pdfium");
            let db_path = Path::new(&args[3]);
            let last = indexer::run(&pdfium, &std::sync::Mutex::new(()), Path::new(&args[2]), db_path, &AtomicBool::new(false), &|p| {
                if p.phase != "indexing" || p.done % 100 == 0 {
                    eprintln!("{} {}/{} ocr={} err={} {:.1}s {}", p.phase, p.done, p.total, p.ocr_pages, p.errors, p.elapsed_ms as f64 / 1000.0, p.message);
                }
            });
            let s = db::stats(&db::open(db_path).unwrap()).unwrap();
            println!("{}: archivos={} páginas={} ocr={} sin_ocr={} errores={} sin_cambios={}", last.phase, s.files, s.pages, s.ocr_pages, s.ocr_pending, s.errors, last.unchanged);
        }
        Some("search") => {
            let mut searcher = Searcher::open(Path::new(&args[2])).expect("índice");
            let query = args[3..].join(" ");
            let cold = searcher.search(&query).expect("búsqueda").elapsed_ms;
            let r = searcher.search(&query).expect("búsqueda");
            println!("{} archivos en {:.1} ms (primera vez {:.1} ms)", r.total_files, r.elapsed_ms, cold);
            for h in r.hits.iter().take(10) {
                let pages: Vec<String> = h.pages.iter().map(|p| format!("{}{}", p.page, if p.exact { "" } else { "?" })).collect();
                let snippet = h.snippet.replace('\u{1}', "«").replace('\u{2}', "»");
                println!("[{}] {} p.{} ({} pág., {} exactas) {snippet}\n    páginas: {}", h.kind, h.name, h.page, h.pages_matched, h.exact_pages, pages.join(" "));
            }
        }
        Some(cmd @ ("keep" | "text")) => {
            // keep <indice.db> <carpeta> <salida>            → conserva la fila según el nombre del archivo
            // text <indice.db> <carpeta> <salida> <t1> | <t2> → testa los textos separados por «|»
            let (db_path, folder, output) = (Path::new(&args[2]), Path::new(&args[3]), Path::new(&args[4]));
            let mode = if cmd == "keep" { redact::Mode::Keep } else { redact::Mode::Text };
            let texts: Vec<String> = args[5..].join(" ").split('|').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let mut searcher = Searcher::open(db_path).expect("índice");
            let docs = redact::candidates(&mut searcher, mode, &texts, mode == redact::Mode::Keep).expect("candidatos");
            for d in &docs {
                println!("documento {} identidad={:?}", d.rel, d.identity);
            }
            let pdfium = load_pdfium(&std::env::temp_dir().join("ocr-documentos-cli")).expect("pdfium");
            let lock = std::sync::Mutex::new(());
            let terms: Vec<redact::Term> = texts.iter().filter_map(|t| redact::Term::new(t)).collect();
            let plan = redact::Plan { mode, terms: &terms };
            let analyzed = std::sync::Mutex::new(Vec::new());
            let last = redact::analyze(&pdfium, &lock, folder, &docs, &plan, &AtomicBool::new(false), &|_| {}, &|d| analyzed.lock().unwrap().push(d));
            println!("análisis: {} en {:.1}s", last.phase, last.elapsed_ms as f64 / 1000.0);
            let mut apply = Vec::new();
            for d in analyzed.into_inner().unwrap() {
                println!("{} error={:?}", d.rel, d.error);
                let mut pages = Vec::new();
                for p in &d.pages {
                    let kept: Vec<&str> = p.regions.iter().filter(|r| r.keep).map(|r| r.label.as_str()).collect();
                    let active = p.regions.iter().filter(|r| r.active).count();
                    println!("  p{:02} {:6} testadas={:2} visible={:?} {}", p.page, p.status, active, kept, p.note);
                    let rects = p.regions.iter().filter(|r| r.active).flat_map(|r| r.rects.clone()).collect();
                    pages.push(redact::ApplyPage { page: p.page, rects });
                }
                apply.push((d.rel.clone(), redact::ApplyDoc { id: d.id, pages }));
            }
            let done = redact::apply(&pdfium, &lock, folder, output, &apply, &AtomicBool::new(false), &|_| {});
            for f in done.report.unwrap_or_default().files {
                println!("guardado {} páginas testadas={} error={:?}", f.output, f.pages, f.error);
            }
            println!("guardado en {:.1}s", done.elapsed_ms as f64 / 1000.0);
        }
        _ => eprintln!("uso: cli index <carpeta> <indice.db> | search <indice.db> <consulta> | keep <indice.db> <carpeta> <salida> | text <indice.db> <carpeta> <salida> <t1> | <t2>"),
    }
}
