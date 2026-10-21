//! Visor de la búsqueda: imagen de cada página y ubicación de las coincidencias en ella.

use crate::indexer::{clean, ocr_decision, OCR_DPI, OCR_MAX_SIDE};
use crate::matching::tokens_from_words;
use crate::ocr::{unrotate, Ocr, Word};
use crate::pdf::Pdfium;
use crate::redact::{bars, encode_jpeg, page_words};
use crate::search::Matcher;
use std::path::Path;
use std::sync::Mutex;

const VIEW_DPI: f32 = 150.0;
const VIEW_MAX_SIDE: f32 = 2400.0;
const VIEW_QUALITY: u8 = 78;

/// Imagen JPEG de una página, con resolución suficiente para leerla con zoom.
pub fn page_image(pdfium: &Pdfium, pdf_lock: &Mutex<()>, source: &Path, page: usize) -> Result<Vec<u8>, String> {
    let data = std::fs::read(source).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let color = {
        let _guard = pdf_lock.lock().unwrap();
        let pdf = pdfium.open(&data)?;
        let i = page.checked_sub(1).filter(|i| *i < pdf.page_count()).ok_or("La página no existe")?;
        let p = pdf.page(i)?;
        p.render_color(VIEW_DPI, VIEW_MAX_SIDE)?
    };
    encode_jpeg(&color.pixels, color.width, color.height, VIEW_QUALITY)
}

/// Palabras de una página con posición, en la orientación en que se lee. Se guardan para no
/// repetir el OCR al volver a la página o al cambiar la búsqueda.
#[derive(Clone)]
pub struct PageWords {
    words: Vec<Word>,
    turns: u8,
    /// Alto entre ancho de la página derecha.
    aspect: f32,
}

pub fn read_words(pdfium: &Pdfium, pdf_lock: &Mutex<()>, source: &Path, page: usize) -> Result<PageWords, String> {
    let data = std::fs::read(source).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let (gray, text_words, size) = {
        let _guard = pdf_lock.lock().unwrap();
        let pdf = pdfium.open(&data)?;
        let i = page.checked_sub(1).filter(|i| *i < pdf.page_count()).ok_or("La página no existe")?;
        let p = pdf.page(i)?;
        let (needs_ocr, _) = ocr_decision(&p, &clean(&p.text()));
        let gray = if needs_ocr { Some(p.render_gray(OCR_DPI, OCR_MAX_SIDE.min(Ocr::max_side()))?) } else { None };
        (gray, p.words(), p.size())
    };
    let (words, turns) = match gray {
        Some(gray) => {
            let engine = Ocr::new().ok();
            page_words(&gray, text_words, true, engine.as_ref())?
        }
        None => (text_words, 0),
    };
    let aspect = if turns % 2 == 1 { size.0 / size.1 } else { size.1 / size.0 };
    Ok(PageWords { words, turns, aspect })
}

#[derive(serde::Serialize)]
pub struct Highlight {
    /// En fracciones de la página, como se muestra.
    pub rects: Vec<[f32; 4]>,
    pub exact: bool,
    /// Lo que se leyó en ese lugar.
    pub text: String,
}

/// Dónde aparece la búsqueda en la página, con el mismo criterio que la búsqueda en el índice.
pub fn locate(page: &PageWords, matcher: &Matcher) -> Vec<Highlight> {
    let tokens = tokens_from_words(&page.words);
    matcher
        .find(&tokens)
        .into_iter()
        .map(|f| {
            let part = &tokens[f.start..f.end];
            let (first, last) = (part[0].span.0, part[part.len() - 1].span.1);
            let text = page.words[first..last].iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            let rects = bars(part, page.aspect).into_iter().map(|r| unrotate(r, page.turns)).collect();
            Highlight { rects, exact: f.exact, text }
        })
        .collect()
}
