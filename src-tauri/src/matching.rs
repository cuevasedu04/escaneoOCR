//! Comparación de palabras tolerante a errores de OCR. La usan la búsqueda (qué páginas mencionan
//! un texto y dónde) y el testado (qué fila es la de la persona, qué cubrir).

use crate::ocr::Word;
use crate::search::{distance, fold, tokenize};

/// Palabra lista para comparar: normalizada, con su posición en la página si se conoce y de dónde
/// salió (bytes dentro del texto, o índice de la palabra del OCR).
pub struct Token {
    pub folded: String,
    pub rect: Option<[f32; 4]>,
    pub span: (usize, usize),
}

/// Minúsculas sin acentos; en los números se ignoran los ceros a la izquierda.
pub fn norm(s: &str) -> String {
    let f = fold(s);
    if !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()) {
        let t = f.trim_start_matches('0');
        return if t.is_empty() { "0".into() } else { t.to_string() };
    }
    f
}

pub fn tokens_from_text(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(Token { folded: norm(&text[s..i]), rect: None, span: (s, i) });
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Separa cada palabra del OCR en secuencias de letras y dígitos. Cuando trae signos pegados
/// («López,»), el rectángulo de cada parte se reparte en proporción a sus letras.
pub fn tokens_from_words(words: &[Word]) -> Vec<Token> {
    let mut out = Vec::new();
    for (index, w) in words.iter().enumerate() {
        let chars: Vec<char> = w.text.chars().collect();
        let n = chars.len().max(1) as f32;
        let mut i = 0;
        while i < chars.len() {
            if !chars[i].is_alphanumeric() {
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && chars[i].is_alphanumeric() {
                i += 1;
            }
            let part: String = chars[start..i].iter().collect();
            let width = w.rect[2] - w.rect[0];
            let rect = [w.rect[0] + width * start as f32 / n, w.rect[1], w.rect[0] + width * i as f32 / n, w.rect[3]];
            out.push(Token { folded: norm(&part), rect: Some(rect), span: (index, index + 1) });
        }
    }
    out
}

/// Dígitos a partir de los cuales un número tolera un error de lectura.
const LONG_NUMBER: usize = 8;

/// Letra que el OCR suele leer en lugar de un dígito.
fn ocr_digit(c: char) -> char {
    match c {
        'o' | 'q' | 'd' => '0',
        'l' | 'i' | 'j' | 't' => '1',
        'z' => '2',
        's' => '5',
        'b' => '8',
        'g' => '9',
        other => other,
    }
}

/// Palabra buscada con la tolerancia a errores de OCR que se le permite.
#[derive(Clone)]
pub struct Query {
    pub chars: Vec<char>,
    pub text: String,
    pub digits: bool,
    pub max: usize,
}

impl Query {
    pub fn new(word: &str) -> Self {
        let text = norm(word);
        let digits = text.chars().all(|c| c.is_ascii_digit());
        // Los números (folios, empleados) deben coincidir exactos
        let max = if digits {
            0
        } else {
            match text.chars().count() {
                0..=3 => 0,
                4..=7 => 1,
                _ => 2,
            }
        };
        Query { chars: text.chars().collect(), text, digits, max }
    }

    pub fn matches(&self, t: &str) -> bool {
        if self.digits && self.chars.len() >= LONG_NUMBER {
            // Folios y números de empleado largos: se corrigen las confusiones típicas del OCR
            // (o→0, l→1, s→5…) y se tolera un dígito distinto, de más o de menos
            let fixed: String = t.chars().map(ocr_digit).collect();
            if fixed.chars().filter(char::is_ascii_digit).count() * 10 < fixed.chars().count() * 7 {
                return false;
            }
            let fixed = fixed.trim_start_matches('0');
            return distance(&self.chars, fixed, 1).is_some();
        }
        if self.max == 0 {
            self.text == t
        } else {
            distance(&self.chars, t, self.max).is_some()
        }
    }

    /// Lectura muy dañada de la misma palabra («Blodes» por «Flores», «Marti» por «Martinez»).
    pub fn resembles(&self, t: &str) -> bool {
        !self.digits && t.chars().all(char::is_alphabetic) && distance(&self.chars, t, (self.chars.len() / 2).max(2)).is_some()
    }
}

/// Un texto a testar (una o varias palabras).
#[derive(Clone)]
pub struct Term {
    pub words: Vec<Query>,
    /// El texto tal como lo escribió el usuario.
    pub label: String,
}

impl Term {
    pub fn new(text: &str) -> Option<Self> {
        let words: Vec<Query> = tokenize(text).iter().map(|w| Query::new(w)).collect();
        (!words.is_empty() && words.iter().map(|q| q.chars.len()).sum::<usize>() >= 2).then(|| Term { words, label: text.trim().to_string() })
    }

    /// Apariciones como rangos de palabras. En textos de tres palabras o más se tolera una palabra
    /// muy mal leída o ausente, y hasta dos fragmentos sueltos (p. ej. «i», «z») intercalados.
    pub fn find(&self, tokens: &[Token]) -> Vec<(usize, usize)> {
        let n = self.words.len();
        let budget = usize::from(n >= 3);
        let mut out = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            match self.align(tokens, 0, i, i, budget, 2) {
                Some(end) if end > i => {
                    out.push((i, end));
                    i = end;
                }
                _ => i += 1,
            }
        }
        out
    }

    fn align(&self, tokens: &[Token], q: usize, t: usize, start: usize, errors: usize, junk: usize) -> Option<usize> {
        if q == self.words.len() {
            return Some(t);
        }
        let word = &self.words[q];
        if let Some(tok) = tokens.get(t) {
            if word.matches(&tok.folded) {
                if let Some(end) = self.align(tokens, q + 1, t + 1, start, errors, junk) {
                    return Some(end);
                }
            }
            if errors > 0 && word.resembles(&tok.folded) {
                if let Some(end) = self.align(tokens, q + 1, t + 1, start, errors - 1, junk) {
                    return Some(end);
                }
            }
            if junk > 0 && t > start && q > 0 && tok.folded.chars().count() <= 2 {
                if let Some(end) = self.align(tokens, q, t + 1, start, errors, junk - 1) {
                    return Some(end);
                }
            }
        }
        // Palabra que el OCR no leyó (nunca la primera)
        if errors > 0 && q > 0 {
            return self.align(tokens, q + 1, t, start, errors - 1, junk);
        }
        None
    }
}

// ── Búsqueda ────────────────────────────────────────────────────────────────

/// Qué tanto se parece una palabra del documento a una de la consulta.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Class {
    None,
    /// Lectura muy dañada («Blodes» por «Flores»).
    Resembles,
    /// Con los errores de lectura que se toleran («Flore», «Andros»).
    Close,
    Exact,
}

/// Palabras de más que se permiten entre las de la consulta (fragmentos sueltos, saltos de renglón).
const SLACK: usize = 3;

/// Lo que el usuario busca, con el mismo criterio tolerante del testado: se aceptan palabras con
/// errores de OCR y, en textos de tres palabras o más, que falte una.
pub struct Pattern {
    pub words: Vec<Query>,
    /// La última palabra puede estar incompleta (se está escribiendo).
    prefix_last: bool,
}

/// Una aparición: rango de palabras y si coincide tal cual.
#[derive(Clone, Copy, Debug)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub exact: bool,
}

impl Pattern {
    pub fn new(text: &str) -> Option<Self> {
        let words: Vec<Query> = tokenize(text).iter().map(|w| Query::new(w)).collect();
        let prefix_last = !text.ends_with(|c: char| !c.is_alphanumeric());
        (!words.is_empty() && words.iter().map(|q| q.chars.len()).sum::<usize>() >= 2).then_some(Pattern { words, prefix_last })
    }

    pub fn prefix_last(&self) -> bool {
        self.prefix_last
    }

    pub fn classify(&self, k: usize, folded: &str) -> Class {
        let q = &self.words[k];
        let last = self.prefix_last && k + 1 == self.words.len() && q.chars.len() >= 2 && !q.digits;
        if q.text == folded || (last && folded.starts_with(&q.text)) {
            Class::Exact
        } else if q.matches(folded) {
            Class::Close
        } else if q.chars.len() >= 4 && q.resembles(folded) {
            Class::Resembles
        } else {
            Class::None
        }
    }

    /// Cuántas palabras deben aparecer y cuántas de ellas sin daño grave.
    fn needed(&self) -> (usize, usize) {
        match self.words.len() {
            1 => (1, 1),
            2 => (2, 1),
            n => (n - 1, n - 2),
        }
    }

    /// Las palabras de la consulta que pueden faltar en una aparición (para preseleccionar páginas).
    pub fn may_miss(&self) -> usize {
        self.words.len() - self.needed().0
    }

    /// Palabra que distingue (no «de», «la», «y»): las cortas no bastan para sostener una aparición.
    pub fn significant(&self, k: usize) -> bool {
        let q = &self.words[k];
        q.digits || q.chars.len() >= 3
    }

    /// Palabras que distinguen que deben aparecer: todas menos una, y al menos dos si las hay.
    pub fn significant_needed(&self) -> usize {
        let total = (0..self.words.len()).filter(|k| self.significant(*k)).count();
        if self.may_miss() == 0 {
            total
        } else {
            total.saturating_sub(1).max(total.min(2))
        }
    }

    /// Apariciones en el texto, sin traslaparse: primero las exactas y luego las aproximadas.
    pub fn find(&self, tokens: &[Token]) -> Vec<Found> {
        let n = self.words.len();
        let classes: Vec<Vec<Class>> = tokens.iter().map(|t| (0..n).map(|k| self.classify(k, &t.folded)).collect()).collect();
        self.find_classified(&classes)
    }

    /// Como `find`, con la clase de cada palabra del texto respecto a cada palabra de la consulta.
    pub fn find_classified<C: AsRef<[Class]>>(&self, classes: &[C]) -> Vec<Found> {
        let n = self.words.len();
        let mut used = vec![false; classes.len()];
        let mut out = Vec::new();

        if classes.len() >= n {
            let mut s = 0;
            while s + n <= classes.len() {
                if (0..n).all(|k| classes[s + k].as_ref()[k] == Class::Exact) {
                    out.push(Found { start: s, end: s + n, exact: true });
                    used[s..s + n].fill(true);
                    s += n;
                } else {
                    s += 1;
                }
            }
        }

        let (need, strict) = self.needed();
        let significant = self.significant_needed();
        let mut s = 0;
        while s < classes.len() {
            if used[s] || classes[s].as_ref().iter().all(|c| *c == Class::None) {
                s += 1;
                continue;
            }
            // Cada palabra de la consulta toma la mejor palabra libre de la ventana
            let window = s..(s + n + SLACK).min(classes.len());
            let mut taken: Vec<usize> = Vec::new();
            let (mut found, mut good, mut distinct) = (0, 0, 0);
            for k in 0..n {
                let best = window.clone().filter(|j| !used[*j] && !taken.contains(j)).max_by_key(|j| (classes[*j].as_ref()[k], std::cmp::Reverse(*j)));
                if let Some(j) = best.filter(|j| classes[*j].as_ref()[k] != Class::None) {
                    taken.push(j);
                    found += 1;
                    distinct += usize::from(self.significant(k));
                    if classes[j].as_ref()[k] >= Class::Close {
                        good += 1;
                    }
                }
            }
            if found >= need && good >= strict && distinct >= significant && taken.contains(&s) {
                let end = taken.iter().max().unwrap() + 1;
                out.push(Found { start: s, end, exact: false });
                used[s..end].fill(true);
                s = end;
            } else {
                s += 1;
            }
        }
        out.sort_by_key(|f| f.start);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(q: &str, text: &str) -> Vec<(String, bool)> {
        let tokens = tokens_from_text(text);
        Pattern::new(q)
            .unwrap()
            .find(&tokens)
            .iter()
            .map(|f| (tokens[f.start..f.end].iter().map(|t| t.folded.as_str()).collect::<Vec<_>>().join(" "), f.exact))
            .collect()
    }

    #[test]
    fn lecturas_del_ocr() {
        let q = "Andres Martinez Flores ";
        assert_eq!(found(q, "Osornio Andres Martínez Flores Arely"), vec![("andres martinez flores".into(), true)]);
        assert_eq!(found(q, "Osornio Andres Martinez Blodes Arely"), vec![("andres martinez blodes".into(), false)]);
        assert_eq!(found(q, "Firma Andres Marti Flores Hora"), vec![("andres marti flores".into(), false)]);
        assert_eq!(found(q, "Osornio Andres Martínez F I ores reiy"), vec![("andres martinez f i ores".into(), false)]);
        assert_eq!(found(q, "Osornio Andres Marti.iíz Arely Cruz"), vec![("andres marti".into(), false)]);
        assert_eq!(found(q, "Osornio Andros Martinez Flores Arely"), vec![("andros martinez flores".into(), false)]);
        assert!(found(q, "Raul Martinez Arreola Sergio").is_empty());
        assert!(found("flores", "Arely Cruz Florencio").is_empty());
        assert!(found("recibo de nomina", "Tipo de Nomina ordinaria").is_empty());
        assert_eq!(found("Brenda de la Cruz", "Brenda de ia muz Esquivel"), vec![("brenda de ia muz".into(), false)]);
        assert_eq!(found("flor", "Andres Martinez Flores"), vec![("flores".into(), true)]);
        assert_eq!(found("2022195", "empleado 00002022195 y 2022135"), vec![("2022195".into(), true)]);
    }
}
