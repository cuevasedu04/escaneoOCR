pub mod db;
pub mod indexer;
pub mod layout;
pub mod matching;
pub mod ocr;
pub mod pdf;
pub mod redact;
pub mod search;
pub mod viewer;

use indexer::Progress;
use pdf::Pdfium;
use search::{SearchResult, Searcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

/// PDFium viaja dentro del ejecutable para que la aplicación sea un solo archivo.
static PDFIUM_DLL: &[u8] = include_bytes!("../resources/pdfium.dll");
const PDFIUM_FILE: &str = "pdfium-8100.dll";

/// Igual que PDFium: con el toolchain GNU el cargador de WebView2 es una DLL aparte.
#[cfg(target_env = "gnu")]
static WEBVIEW2_LOADER_DLL: &[u8] = include_bytes!("../resources/WebView2Loader.dll");

/// Escribe en `dir` una DLL incrustada (si falta o cambió) y devuelve su ruta.
fn extract(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = dir.join(name);
    let current = std::fs::metadata(&path).map(|m| m.len() == bytes.len() as u64).unwrap_or(false);
    if !current {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("No se pudo preparar {name}: {e}"))?;
    }
    Ok(path)
}

pub fn load_pdfium(dir: &Path) -> Result<Pdfium, String> {
    Pdfium::load(&extract(dir, PDFIUM_FILE, PDFIUM_DLL)?)
}

/// Carga WebView2Loader.dll desde la copia incrustada antes de que Tauri la necesite.
/// Está enlazada con carga diferida (ver build.rs), así que basta con que ya esté en memoria.
#[cfg(target_env = "gnu")]
fn preload_webview2_loader() {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return };
    let dir = PathBuf::from(local).join("mx.gob.anam.ocrdocumentos");
    if let Ok(path) = extract(&dir, "WebView2Loader.dll", WEBVIEW2_LOADER_DLL) {
        if let Ok(lib) = unsafe { libloading::Library::new(&path) } {
            std::mem::forget(lib);
        }
    }
}

struct Current {
    folder: PathBuf,
    db_path: PathBuf,
    searcher: Searcher,
}

struct AppState {
    data_dir: PathBuf,
    pdfium: OnceLock<Result<Pdfium, String>>,
    ocr_language: OnceLock<Option<String>>,
    current: Mutex<Option<Current>>,
    indexing: AtomicBool,
    cancel: AtomicBool,
    progress: Mutex<Option<Progress>>,
    /// Hay un testado en curso (excluye cargas y otros testados).
    redacting: AtomicBool,
    /// Carpeta del último testado: solo de ahí se abren PDFs testados.
    last_output: Mutex<Option<PathBuf>>,
    /// PDFium no admite llamadas simultáneas: el testado y la vista previa lo usan por turnos.
    pdf_lock: Mutex<()>,
    /// Palabras de las páginas ya vistas en el visor, para no repetir su OCR.
    page_words: Mutex<HashMap<(i64, usize), viewer::PageWords>>,
}

type Shared<'a> = State<'a, Arc<AppState>>;

#[derive(serde::Serialize)]
struct AppInfo {
    folder: Option<String>,
    stats: Option<db::Stats>,
    indexing: bool,
    redacting: bool,
    progress: Option<Progress>,
    /// Idioma del OCR de Windows; None si el equipo no tiene ninguno instalado.
    ocr_language: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Settings {
    folder: Option<String>,
}

impl AppState {
    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    /// Un índice por carpeta, nombrado con el hash de su ruta.
    fn db_path_for(&self, folder: &Path) -> PathBuf {
        let key = folder.to_string_lossy().to_lowercase();
        let hash = key.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
        self.data_dir.join("indices").join(format!("{hash:016x}.db"))
    }

    fn open(&self, folder: &Path) -> Result<(), String> {
        let db_path = self.db_path_for(folder);
        std::fs::create_dir_all(db_path.parent().unwrap()).map_err(|e| e.to_string())?;
        let searcher = Searcher::open(&db_path)?;
        *self.current.lock().unwrap() = Some(Current { folder: folder.to_path_buf(), db_path, searcher });
        self.page_words.lock().unwrap().clear();
        Ok(())
    }

    fn info(&self) -> AppInfo {
        let current = self.current.lock().unwrap();
        AppInfo {
            folder: current.as_ref().map(|c| c.folder.to_string_lossy().to_string()),
            stats: current.as_ref().and_then(|c| db::stats(c.searcher.conn()).ok()),
            indexing: self.indexing.load(Ordering::Relaxed),
            redacting: self.redacting.load(Ordering::Relaxed),
            progress: self.progress.lock().unwrap().clone(),
            ocr_language: self.ocr_language.get_or_init(|| ocr::Ocr::new().ok().map(|o| o.language)).clone(),
        }
    }
}

#[tauri::command]
async fn app_info(state: Shared<'_>) -> Result<AppInfo, String> {
    Ok(state.info())
}

/// Abre el índice de la carpeta y lanza la carga (incremental) de sus PDFs.
#[tauri::command]
async fn load_folder(app: AppHandle, state: Shared<'_>, folder: String) -> Result<AppInfo, String> {
    let folder = PathBuf::from(folder.trim().trim_matches('"').trim_end_matches(['\\', '/']));
    if !folder.is_dir() {
        return Err("La ruta no existe o no es una carpeta.".into());
    }
    if state.indexing.swap(true, Ordering::SeqCst) {
        return Err("Ya hay una carga en curso.".into());
    }
    if state.redacting.load(Ordering::SeqCst) {
        state.indexing.store(false, Ordering::SeqCst);
        return Err("Espera a que termine el testado en curso.".into());
    }
    if let Err(e) = state.open(&folder) {
        state.indexing.store(false, Ordering::SeqCst);
        return Err(e);
    }
    let settings = Settings { folder: Some(folder.to_string_lossy().to_string()) };
    let _ = std::fs::write(state.settings_path(), serde_json::to_string_pretty(&settings).unwrap_or_default());

    state.cancel.store(false, Ordering::SeqCst);
    *state.progress.lock().unwrap() = Some(Progress { phase: "scanning", ..Default::default() });
    let shared = state.inner().clone();
    let db_path = shared.db_path_for(&folder);
    std::thread::spawn(move || {
        let publish = |p: &Progress| {
            *shared.progress.lock().unwrap() = Some(p.clone());
            let _ = app.emit("index-progress", p);
        };
        // El estado final lo publica `finish`, ya con la carga marcada como terminada
        let emit = |p: &Progress| {
            if !matches!(p.phase, "done" | "cancelled" | "error") {
                publish(p);
            }
        };
        let finish = |p: Progress| {
            // El índice cambió: el vocabulario en memoria ya no sirve
            if let Some(c) = shared.current.lock().unwrap().as_mut() {
                if c.db_path == db_path {
                    c.searcher.invalidate();
                    shared.page_words.lock().unwrap().clear();
                }
            }
            shared.indexing.store(false, Ordering::SeqCst);
            publish(&p);
        };
        match shared.pdfium.get_or_init(|| load_pdfium(&shared.data_dir)) {
            Ok(pdfium) => {
                let last = indexer::run(pdfium, &shared.pdf_lock, &folder, &db_path, &shared.cancel, &emit);
                finish(last);
            }
            Err(e) => finish(Progress { phase: "error", message: e.clone(), ..Default::default() }),
        }
    });
    Ok(state.info())
}

#[tauri::command]
async fn cancel_index(state: Shared<'_>) -> Result<(), String> {
    state.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
async fn search(state: Shared<'_>, query: String) -> Result<SearchResult, String> {
    let mut current = state.current.lock().unwrap();
    match current.as_mut() {
        Some(c) => c.searcher.search(&query),
        None => Ok(SearchResult::default()),
    }
}

#[derive(serde::Serialize)]
struct Problem {
    name: String,
    detail: String,
}

/// Archivos que no se pudieron leer o que quedaron con páginas sin OCR.
#[tauri::command]
async fn index_problems(state: Shared<'_>) -> Result<Vec<Problem>, String> {
    let current = state.current.lock().unwrap();
    let Some(c) = current.as_ref() else { return Ok(Vec::new()) };
    let mut stmt = c
        .searcher
        .conn()
        .prepare("SELECT rel, coalesce(error, ocr_pending || ' página(s) sin OCR') FROM files WHERE error IS NOT NULL OR ocr_pending > 0 ORDER BY rel LIMIT 500")
        .map_err(db::err)?;
    let rows = stmt.query_map([], |r| Ok(Problem { name: r.get(0)?, detail: r.get(1)? })).map_err(db::err)?;
    Ok(rows.flatten().collect())
}

/// Documentos a los que aplica la solicitud de testado, con la identidad que se usará en cada uno.
#[tauri::command]
async fn redaction_candidates(state: Shared<'_>, mode: redact::Mode, texts: Vec<String>, from_file_name: bool) -> Result<Vec<redact::Candidate>, String> {
    let mut current = state.current.lock().unwrap();
    let c = current.as_mut().ok_or("No hay carpeta cargada.")?;
    redact::candidates(&mut c.searcher, mode, &texts, from_file_name)
}

#[tauri::command]
async fn redaction_default_output(state: Shared<'_>) -> Result<String, String> {
    let current = state.current.lock().unwrap();
    let c = current.as_ref().ok_or("No hay carpeta cargada.")?;
    Ok(redact::default_output(&c.folder).to_string_lossy().to_string())
}

/// Ruta absoluta en minúsculas y con separador final, para comparar carpetas sin importar mayúsculas.
fn folder_key(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut s = abs.to_string_lossy().replace('/', "\\").to_lowercase();
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}

/// Marca el inicio de un análisis o guardado; falla si hay otra tarea en curso.
fn begin_redaction(state: &AppState) -> Result<(), String> {
    if state.redacting.swap(true, Ordering::SeqCst) {
        return Err("Ya hay un testado en curso.".into());
    }
    if state.indexing.load(Ordering::SeqCst) {
        state.redacting.store(false, Ordering::SeqCst);
        return Err("Espera a que termine la carga de documentos.".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    Ok(())
}

/// Analiza los documentos y propone qué testar en cada página. Cada documento llega con el evento
/// `redact-doc` en cuanto termina; el avance llega con `redact-progress`.
#[tauri::command]
async fn start_analysis(
    app: AppHandle,
    state: Shared<'_>,
    mode: redact::Mode,
    texts: Vec<String>,
    from_file_name: bool,
    file_ids: Vec<i64>,
) -> Result<(), String> {
    let (folder, docs) = {
        let mut current = state.current.lock().unwrap();
        let c = current.as_mut().ok_or("No hay carpeta cargada.")?;
        let wanted: std::collections::HashSet<i64> = file_ids.into_iter().collect();
        let docs: Vec<redact::Candidate> = redact::candidates(&mut c.searcher, mode, &texts, from_file_name)?.into_iter().filter(|d| wanted.contains(&d.id)).collect();
        (c.folder.clone(), docs)
    };
    if docs.is_empty() {
        return Err("No hay documentos seleccionados.".into());
    }
    let terms: Vec<redact::Term> = texts.iter().filter_map(|t| redact::Term::new(t)).collect();
    if mode == redact::Mode::Text && terms.is_empty() {
        return Err("Escribe al menos un texto a testar.".into());
    }
    begin_redaction(&state)?;
    let shared = state.inner().clone();
    std::thread::spawn(move || {
        let emit = |p: &redact::Progress| {
            let _ = app.emit("redact-progress", p);
        };
        let last = match shared.pdfium.get_or_init(|| load_pdfium(&shared.data_dir)) {
            Ok(pdfium) => {
                let plan = redact::Plan { mode, terms: &terms };
                redact::analyze(pdfium, &shared.pdf_lock, &folder, &docs, &plan, &shared.cancel, &emit, &|doc| {
                    let _ = app.emit("redact-doc", doc);
                })
            }
            Err(e) => redact::Progress { phase: "error", message: e.clone(), ..Default::default() },
        };
        shared.redacting.store(false, Ordering::SeqCst);
        emit(&last);
    });
    Ok(())
}

/// Página de un documento como JPEG, para la revisión en pantalla.
#[tauri::command]
async fn redaction_page_image(state: Shared<'_>, id: i64, page: usize) -> Result<tauri::ipc::Response, String> {
    let path = indexed_path(&state, id)?;
    let pdfium = state.pdfium.get_or_init(|| load_pdfium(&state.data_dir)).as_ref().map_err(|e| e.clone())?;
    let jpeg = redact::preview_image(pdfium, &state.pdf_lock, &path, page)?;
    Ok(tauri::ipc::Response::new(jpeg))
}

/// Guarda las copias testadas con los rectángulos que el usuario confirmó. Los originales no se modifican.
#[tauri::command]
async fn apply_redaction(app: AppHandle, state: Shared<'_>, output_dir: String, docs: Vec<redact::ApplyDoc>) -> Result<(), String> {
    let output = PathBuf::from(output_dir.trim().trim_matches('"').trim_end_matches(['\\', '/']));
    if output.as_os_str().is_empty() {
        return Err("Indica la carpeta donde se guardarán los documentos testados.".into());
    }
    let (folder, docs) = {
        let current = state.current.lock().unwrap();
        let c = current.as_ref().ok_or("No hay carpeta cargada.")?;
        // La salida no puede mezclarse con los originales: ni dentro de la carpeta ni conteniéndola
        let (src, out) = (folder_key(&c.folder), folder_key(&output));
        if out.starts_with(&src) || src.starts_with(&out) {
            return Err("La carpeta de salida debe estar fuera de la carpeta de documentos originales.".into());
        }
        let mut resolved = Vec::new();
        for d in docs {
            let rel: String = c.searcher.conn().query_row("SELECT rel FROM files WHERE id = ?1", [d.id], |r| r.get(0)).map_err(db::err)?;
            resolved.push((rel, d));
        }
        (c.folder.clone(), resolved)
    };
    if docs.is_empty() {
        return Err("No hay documentos para guardar.".into());
    }
    std::fs::create_dir_all(&output).map_err(|e| format!("No se pudo crear la carpeta de salida: {e}"))?;
    begin_redaction(&state)?;
    *state.last_output.lock().unwrap() = Some(output.clone());
    let shared = state.inner().clone();
    std::thread::spawn(move || {
        let emit = |p: &redact::Progress| {
            let _ = app.emit("redact-progress", p);
        };
        let last = match shared.pdfium.get_or_init(|| load_pdfium(&shared.data_dir)) {
            Ok(pdfium) => redact::apply(pdfium, &shared.pdf_lock, &folder, &output, &docs, &shared.cancel, &emit),
            Err(e) => redact::Progress { phase: "error", message: e.clone(), ..Default::default() },
        };
        shared.redacting.store(false, Ordering::SeqCst);
        emit(&last);
    });
    Ok(())
}

/// Abre un PDF testado (o la carpeta de salida si `path` está vacío). Solo dentro de la última salida.
#[tauri::command]
async fn open_redacted(app: AppHandle, state: Shared<'_>, path: String) -> Result<(), String> {
    let base = state.last_output.lock().unwrap().clone().ok_or("Aún no hay documentos testados.")?;
    let target = if path.is_empty() { base.clone() } else { PathBuf::from(&path) };
    if !folder_key(&target).starts_with(&folder_key(&base)) || !target.exists() {
        return Err("El documento testado ya no está en la carpeta de salida.".into());
    }
    app.opener().open_path(target.to_string_lossy(), None::<&str>).map_err(|e| format!("No se pudo abrir: {e}"))
}

fn indexed_path(state: &AppState, id: i64) -> Result<PathBuf, String> {
    let current = state.current.lock().unwrap();
    let c = current.as_ref().ok_or("No hay carpeta cargada.")?;
    let path = c.searcher.file_path(&c.folder, id)?;
    if !path.is_file() {
        return Err("El archivo ya no está en la carpeta. Vuelve a cargarla para actualizar el índice.".into());
    }
    Ok(path)
}

/// Abre el PDF. Con `page`, en Microsoft Edge directamente en esa página (el visor predeterminado
/// no recibe la página); si Edge no está, con el visor predeterminado.
#[tauri::command]
async fn open_file(app: AppHandle, state: Shared<'_>, id: i64, page: Option<usize>) -> Result<(), String> {
    let path = indexed_path(&state, id)?;
    if let (Some(page), Some(edge)) = (page, edge_path()) {
        let url = format!("{}#page={page}", file_url(&path));
        if std::process::Command::new(edge).arg(url).spawn().is_ok() {
            return Ok(());
        }
    }
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| format!("No se pudo abrir el archivo: {e}"))
}

fn edge_path() -> Option<PathBuf> {
    ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join(r"Microsoft\Edge\Application\msedge.exe"))
        .find(|p| p.is_file())
}

/// `file:///C:/...` con los caracteres especiales codificados (espacios, acentos, #, %).
fn file_url(path: &Path) -> String {
    let mut url = String::from("file:///");
    for b in path.to_string_lossy().replace('\\', "/").bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => url.push(b as char),
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

#[derive(serde::Serialize)]
struct FileEntry {
    id: i64,
    name: String,
    rel: String,
    pages: i64,
    error: Option<String>,
}

/// Todos los documentos de la carpeta cargada, en orden alfabético.
#[tauri::command]
async fn list_files(state: Shared<'_>) -> Result<Vec<FileEntry>, String> {
    let current = state.current.lock().unwrap();
    let Some(c) = current.as_ref() else { return Ok(Vec::new()) };
    let mut stmt = c.searcher.conn().prepare("SELECT id, rel, pages, error FROM files ORDER BY rel COLLATE NOCASE").map_err(db::err)?;
    let rows = stmt
        .query_map([], |r| {
            let rel: String = r.get(1)?;
            let name = Path::new(&rel).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| rel.clone());
            Ok(FileEntry { id: r.get(0)?, name, rel, pages: r.get(2)?, error: r.get(3)? })
        })
        .map_err(db::err)?;
    Ok(rows.flatten().collect())
}

/// Página de un documento como JPEG, para el visor.
#[tauri::command]
async fn view_page(state: Shared<'_>, id: i64, page: usize) -> Result<tauri::ipc::Response, String> {
    let path = indexed_path(&state, id)?;
    let pdfium = state.pdfium.get_or_init(|| load_pdfium(&state.data_dir)).as_ref().map_err(|e| e.clone())?;
    Ok(tauri::ipc::Response::new(viewer::page_image(pdfium, &state.pdf_lock, &path, page)?))
}

/// Dónde aparece la búsqueda en una página. En las escaneadas se lee con OCR la primera vez.
#[tauri::command]
async fn locate_matches(state: Shared<'_>, id: i64, page: usize, query: String) -> Result<Vec<viewer::Highlight>, String> {
    let cached = state.page_words.lock().unwrap().get(&(id, page)).cloned();
    let words = match cached {
        Some(w) => w,
        None => {
            let path = indexed_path(&state, id)?;
            let pdfium = state.pdfium.get_or_init(|| load_pdfium(&state.data_dir)).as_ref().map_err(|e| e.clone())?;
            let w = viewer::read_words(pdfium, &state.pdf_lock, &path, page)?;
            let mut cache = state.page_words.lock().unwrap();
            if cache.len() >= 300 {
                cache.clear();
            }
            cache.insert((id, page), w.clone());
            w
        }
    };
    let matcher = {
        let mut current = state.current.lock().unwrap();
        let c = current.as_mut().ok_or("No hay carpeta cargada.")?;
        c.searcher.matcher(&query)?
    };
    Ok(matcher.map(|m| viewer::locate(&words, &m)).unwrap_or_default())
}

#[tauri::command]
async fn reveal_file(app: AppHandle, state: Shared<'_>, id: i64) -> Result<(), String> {
    let path = indexed_path(&state, id)?;
    app.opener().reveal_item_in_dir(path).map_err(|e| format!("No se pudo mostrar el archivo: {e}"))
}

pub fn run() {
    #[cfg(target_env = "gnu")]
    preload_webview2_loader();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            load_folder,
            cancel_index,
            search,
            index_problems,
            open_file,
            list_files,
            view_page,
            locate_matches,
            reveal_file,
            redaction_candidates,
            redaction_default_output,
            start_analysis,
            redaction_page_image,
            apply_redaction,
            open_redacted
        ])
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let state = Arc::new(AppState {
                data_dir,
                pdfium: OnceLock::new(),
                ocr_language: OnceLock::new(),
                current: Mutex::new(None),
                indexing: AtomicBool::new(false),
                cancel: AtomicBool::new(false),
                progress: Mutex::new(None),
                redacting: AtomicBool::new(false),
                last_output: Mutex::new(None),
                pdf_lock: Mutex::new(()),
                page_words: Mutex::new(HashMap::new()),
            });
            // Reabre el índice de la última carpeta: queda listo para buscar sin volver a cargar
            let saved: Settings = std::fs::read_to_string(state.settings_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            if let Some(folder) = saved.folder {
                let _ = state.open(Path::new(&folder));
            }
            app.manage(state);

            let win = app.get_webview_window("main").unwrap();
            #[cfg(target_os = "windows")]
            {
                let _ = win.set_shadow(true);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
