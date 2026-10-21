//! OCR con el motor nativo de Windows (Windows.Media.Ocr), sin dependencias externas.

use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

/// Idiomas preferidos, en orden. El reconocedor de español también lee texto en inglés.
const PREFERRED: [&str; 4] = ["es-MX", "es-ES", "es", "en-US"];

pub struct Ocr {
    engine: OcrEngine,
    pub language: String,
}

impl Ocr {
    /// Crea un motor para el hilo actual. Cada hilo de OCR debe tener el suyo.
    pub fn new() -> Result<Self, String> {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        for tag in PREFERRED {
            let Ok(lang) = Language::CreateLanguage(&HSTRING::from(tag)) else { continue };
            if !OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
                continue;
            }
            if let Ok(engine) = OcrEngine::TryCreateFromLanguage(&lang) {
                return Ok(Ocr { engine, language: tag.to_string() });
            }
        }
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|_| "Windows no tiene instalado ningún idioma de OCR".to_string())?;
        let language = engine
            .RecognizerLanguage()
            .and_then(|l| l.LanguageTag())
            .map(|t| t.to_string())
            .unwrap_or_default();
        Ok(Ocr { engine, language })
    }

    /// Lado máximo en píxeles que acepta el motor.
    pub fn max_side() -> f32 {
        OcrEngine::MaxImageDimension().unwrap_or(10000) as f32
    }

    /// Reconoce la página y, si el resultado no parece texto legible, la reintenta girada
    /// (escaneos de lado o de cabeza), quedándose con la orientación que mejor se lee.
    /// Las palabras quedan en la orientación en que se leyeron (`quarter_turns` giros de 90°
    /// horarios respecto a la imagen original); `unrotate` las lleva de vuelta a la página.
    pub fn read_any_orientation(&self, gray: &[u8], width: i32, height: i32) -> Result<Reading, String> {
        let upright = self.read(gray, width, height)?;
        let mut best_score = readability(&upright.text);
        if best_score >= READABLE {
            return Ok(upright);
        }
        let mut best = upright;
        for quarter_turns in 1..4u8 {
            let (pixels, w, h) = rotate(gray, width as usize, height as usize, quarter_turns);
            let Ok(mut reading) = self.read(&pixels, w as i32, h as i32) else { continue };
            let score = readability(&reading.text);
            if score >= READABLE && score > best_score {
                reading.quarter_turns = quarter_turns;
                best_score = score;
                best = reading;
            }
        }
        Ok(best)
    }

    fn read(&self, gray: &[u8], width: i32, height: i32) -> Result<Reading, String> {
        let run = || -> windows::core::Result<Reading> {
            let buffer = CryptographicBuffer::CreateFromByteArray(gray)?;
            let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Gray8, width, height)?;
            let result = self.engine.RecognizeAsync(&bitmap)?.get()?;
            let (w, h) = (width as f32, height as f32);
            let mut reading = Reading::default();
            for line in result.Lines()? {
                reading.text.push_str(&line.Text()?.to_string());
                reading.text.push('\n');
                for word in line.Words()? {
                    let r = word.BoundingRect()?;
                    reading.words.push(Word {
                        text: word.Text()?.to_string(),
                        rect: [r.X / w, r.Y / h, (r.X + r.Width) / w, (r.Y + r.Height) / h],
                    });
                }
            }
            Ok(reading)
        };
        run().map_err(|e| format!("OCR: {}", e.message()))
    }
}

/// Palabra con su rectángulo [x0, y0, x1, y1] en fracciones del ancho y alto de la página.
#[derive(Clone)]
pub struct Word {
    pub text: String,
    pub rect: [f32; 4],
}

#[derive(Default)]
pub struct Reading {
    /// Texto por renglones, idéntico al que se guarda en el índice.
    pub text: String,
    pub words: Vec<Word>,
    /// Giros de 90° (horarios) que se aplicaron a la imagen para leerla derecha.
    pub quarter_turns: u8,
}

/// Lleva un rectángulo de la imagen girada `quarter_turns` × 90° (horario) a la imagen original.
pub fn unrotate(r: [f32; 4], quarter_turns: u8) -> [f32; 4] {
    let map = |u: f32, v: f32| match quarter_turns % 4 {
        1 => (v, 1.0 - u),
        2 => (1.0 - u, 1.0 - v),
        3 => (1.0 - v, u),
        _ => (u, v),
    };
    let (ax, ay) = map(r[0], r[1]);
    let (bx, by) = map(r[2], r[3]);
    [ax.min(bx), ay.min(by), ax.max(bx), ay.max(by)]
}

/// Palabras muy frecuentes (español e inglés): un texto bien orientado contiene varias,
/// mientras que la lectura de una página girada casi nunca produce alguna.
const COMMON_WORDS: [&str; 27] = [
    "de", "la", "el", "en", "que", "los", "del", "las", "por", "con", "para", "se", "un", "una", "al", "su", "es", "no", "lo", "como", "the", "of",
    "and", "to", "in", "is", "for",
];
/// Coincidencias con COMMON_WORDS a partir de las cuales una lectura se da por buena.
const READABLE: usize = 3;

fn readability(text: &str) -> usize {
    text.split(|c: char| !c.is_alphabetic()).filter(|w| COMMON_WORDS.contains(&w.to_lowercase().as_str())).count()
}

/// Gira la imagen `quarter_turns` × 90° en sentido horario.
pub fn rotate(src: &[u8], w: usize, h: usize, quarter_turns: u8) -> (Vec<u8>, usize, usize) {
    let mut out = vec![0u8; src.len()];
    match quarter_turns % 4 {
        1 => {
            for y in 0..h {
                for x in 0..w {
                    out[x * h + (h - 1 - y)] = src[y * w + x];
                }
            }
            (out, h, w)
        }
        2 => {
            out.copy_from_slice(src);
            out.reverse();
            (out, w, h)
        }
        3 => {
            for y in 0..h {
                for x in 0..w {
                    out[(w - 1 - x) * h + y] = src[y * w + x];
                }
            }
            (out, h, w)
        }
        _ => (src.to_vec(), w, h),
    }
}
