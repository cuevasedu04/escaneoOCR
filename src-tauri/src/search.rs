//! Búsqueda sobre el índice FTS5 con el mismo criterio tolerante del testado (ver `matching`):
//! se aceptan palabras con errores de OCR y, en textos de tres palabras o más, que falte una.
//! El índice preselecciona las páginas que contienen las palabras o sus variantes, y cada una se
//! confirma palabra por palabra. Cada página queda como coincidencia exacta o por validar.

use crate::db::{self, PAGE_BITS, PAGE_MASK};
use crate::matching::{norm, tokens_from_text, Class, Found, Pattern, Token};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

const MAX_HITS: usize = 300;
const MAX_ROWS: i64 = 200_000;
/// Variantes del vocabulario que se aceptan por palabra (las muy dañadas, a lo más RESEMBLING).
const MAX_VARIANTS: usize = 200;
const RESEMBLING: usize = 80;
/// Palabras de contexto alrededor de la coincidencia en el fragmento que se muestra.
const SNIPPET_BEFORE: usize = 12;
const SNIPPET_AFTER: usize = 22;

#[derive(serde::Serialize, Clone, Copy)]
pub struct PageHit {
    pub page: i64,
    /// false: la coincidencia es aproximada y conviene validarla.
    pub exact: bool,
}

#[derive(serde::Serialize)]
pub struct Hit {
    pub id: i64,
    pub name: String,
    pub rel: String,
    /// Página de la mejor coincidencia; 0 si coincide solo el nombre del archivo.
    pub page: i64,
    /// Texto con la coincidencia delimitada por U+0001 … U+0002.
    pub snippet: String,
    pub pages_matched: usize,
    pub exact_pages: usize,
    /// exact | approx | name
    pub kind: &'static str,
    pub pages: Vec<PageHit>,
}

#[derive(serde::Serialize, Default)]
pub struct SearchResult {
    pub hits: Vec<Hit>,
    pub total_files: usize,
    pub elapsed_ms: f64,
}

/// Palabra del índice: como la guarda FTS5, normalizada como la compara `matching`, y en cuántos
/// textos aparece.
struct VocabTerm {
    raw: String,
    normed: String,
    docs: i64,
}

/// Una consulta lista para comparar: qué palabras del índice cuentan como cada palabra buscada.
/// Una variante solo se acepta como error de lectura si es menos frecuente que la palabra buscada:
/// «Blodes» por «Flores» sí, pero «emisión» (en todos los recibos) no es «comisión» mal leída.
pub struct Matcher {
    pattern: Pattern,
    /// Por palabra de la consulta: variantes normalizadas y su clase.
    accepted: Vec<HashMap<String, Class>>,
    /// Por palabra: las mismas variantes como las guarda el índice.
    raw: Vec<Vec<(String, Class)>>,
}

impl Matcher {
    fn class(&self, k: usize, folded: &str) -> Class {
        let q = &self.pattern.words[k];
        let typing = self.pattern.prefix_last() && k + 1 == self.pattern.words.len() && q.chars.len() >= 2 && !q.digits;
        if q.text == folded || (typing && folded.starts_with(&q.text)) {
            return Class::Exact;
        }
        self.accepted[k].get(folded).copied().unwrap_or(Class::None)
    }

    /// Apariciones en una página (exactas y por validar).
    pub fn find(&self, tokens: &[Token]) -> Vec<Found> {
        let n = self.pattern.words.len();
        let flat: Vec<Class> = tokens.iter().flat_map(|t| (0..n).map(|k| self.class(k, &t.folded))).collect();
        let rows: Vec<&[Class]> = flat.chunks(n).collect();
        self.pattern.find_classified(&rows)
    }

    /// Expresión FTS5 que preselecciona las páginas: cada palabra con sus variantes y, si la
    /// consulta tolera que falte una palabra, cualquier combinación de las demás.
    fn expr(&self) -> Option<String> {
        let n = self.pattern.words.len();
        let groups: Vec<Option<String>> = (0..n).map(|k| self.group(k, |_| true)).collect();
        let all = |skip: Option<usize>| -> Option<String> {
            let parts: Option<Vec<&String>> = groups.iter().enumerate().filter(|(i, _)| Some(*i) != skip).map(|(_, g)| g.as_ref()).collect();
            parts.map(|p| format!("({})", p.into_iter().cloned().collect::<Vec<_>>().join(" AND ")))
        };
        // Solo puede faltar una palabra si las que distinguen que quedan son suficientes
        let significant = (0..n).filter(|k| self.pattern.significant(*k)).count();
        let can_miss = |i: usize| significant - usize::from(self.pattern.significant(i)) >= self.pattern.significant_needed();
        let combos: Vec<String> =
            if self.pattern.may_miss() == 0 { all(None).into_iter().collect() } else { (0..n).filter(|i| can_miss(*i)).filter_map(|i| all(Some(i))).collect() };
        (!combos.is_empty()).then(|| combos.join(" OR "))
    }

    /// Las variantes de la palabra `k` de las clases indicadas, unidas con OR.
    fn group(&self, k: usize, keep: impl Fn(Class) -> bool) -> Option<String> {
        let q = &self.pattern.words[k];
        let mut terms: Vec<String> = self.raw[k].iter().filter(|(_, c)| keep(*c)).map(|(t, _)| quote(t)).collect();
        // La palabra que se está escribiendo se busca como prefijo (salvo los números, que en el
        // índice pueden llevar ceros a la izquierda y ya vienen en `raw`)
        if keep(Class::Exact) && self.pattern.prefix_last() && k + 1 == self.pattern.words.len() && !q.digits && q.chars.len() >= 2 {
            terms.push(format!("{}*", quote(&q.text)));
        }
        (!terms.is_empty()).then(|| format!("({})", terms.join(" OR ")))
    }
}

pub struct Searcher {
    conn: Connection,
    vocab: Option<Vec<VocabTerm>>,
    /// La última consulta preparada: el visor la reutiliza para ubicar las coincidencias.
    last: Option<(String, Arc<Matcher>)>,
}

#[derive(Default)]
struct FileMatch {
    pages: Vec<PageHit>,
    name: bool,
}

impl Searcher {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        Ok(Searcher { conn: db::open(db_path)?, vocab: None, last: None })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Debe llamarse cuando el índice cambió, para releer el vocabulario.
    pub fn invalidate(&mut self) {
        self.vocab = None;
        self.last = None;
    }

    pub fn file_path(&self, folder: &Path, id: i64) -> Result<PathBuf, String> {
        let rel: String = self.conn.query_row("SELECT rel FROM files WHERE id = ?1", [id], |r| r.get(0)).map_err(db::err)?;
        Ok(folder.join(rel))
    }

    pub fn search(&mut self, query: &str) -> Result<SearchResult, String> {
        let started = Instant::now();
        let mut out = SearchResult::default();
        let Some(matcher) = self.matcher(query)? else { return Ok(out) };

        // Páginas con coincidencia (rowid → si es exacta)
        let mut found: HashMap<i64, bool> = HashMap::new();
        if matcher.pattern.words.len() == 1 {
            // Con una sola palabra el índice ya separa las páginas que la tienen tal cual de las que
            // solo tienen una lectura parecida, sin leer su texto
            if let Some(expr) = matcher.group(0, |c| c == Class::Exact) {
                self.each_row(&expr, |rowid| {
                    found.insert(rowid, true);
                })?;
            }
            if let Some(expr) = matcher.group(0, |c| c != Class::Exact) {
                self.each_row(&expr, |rowid| {
                    found.entry(rowid).or_insert(false);
                })?;
            }
        } else {
            let Some(expr) = matcher.expr() else { return Ok(out) };
            let mut stmt = self.conn.prepare_cached("SELECT rowid, text FROM fts WHERE fts MATCH ?1 LIMIT ?2").map_err(db::err)?;
            let mut rows = stmt.query(params![expr, MAX_ROWS]).map_err(db::err)?;
            while let Some(row) = rows.next().map_err(db::err)? {
                let rowid: i64 = row.get(0).map_err(db::err)?;
                let text: String = row.get(1).map_err(db::err)?;
                let hits = matcher.find(&tokens_from_text(&text));
                if !hits.is_empty() {
                    found.insert(rowid, hits.iter().any(|f| f.exact));
                }
            }
        }

        let mut files: HashMap<i64, FileMatch> = HashMap::new();
        for (rowid, exact) in found {
            let m = files.entry(rowid >> PAGE_BITS).or_default();
            match rowid & PAGE_MASK {
                0 => m.name = true,
                page => m.pages.push(PageHit { page, exact }),
            }
        }
        out.total_files = files.len();

        // Primero los documentos con más páginas exactas, luego con más páginas en total
        let mut ranked: Vec<(i64, FileMatch)> = files.into_iter().collect();
        for (_, m) in &mut ranked {
            m.pages.sort_by_key(|p| p.page);
        }
        let exact = |m: &FileMatch| m.pages.iter().filter(|p| p.exact).count();
        ranked.sort_by(|a, b| exact(&b.1).cmp(&exact(&a.1)).then(b.1.pages.len().cmp(&a.1.pages.len())).then(b.1.name.cmp(&a.1.name)).then(a.0.cmp(&b.0)));

        for (file, m) in ranked.into_iter().take(MAX_HITS) {
            let Ok(rel) = self.conn.prepare_cached("SELECT rel FROM files WHERE id = ?1").and_then(|mut s| s.query_row([file], |r| r.get::<_, String>(0))) else {
                continue;
            };
            let name = Path::new(&rel).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| rel.clone());
            // Se muestra la primera página exacta; si no hay, la primera por validar
            let page = m.pages.iter().find(|p| p.exact).or(m.pages.first()).map(|p| p.page).unwrap_or(0);
            let snippet = self.snippet(&matcher, (file << PAGE_BITS) | page)?;
            let exact_pages = exact(&m);
            let kind = if m.pages.is_empty() {
                "name"
            } else if exact_pages > 0 {
                "exact"
            } else {
                "approx"
            };
            out.hits.push(Hit { id: file, name, rel, page, snippet, pages_matched: m.pages.len(), exact_pages, kind, pages: m.pages });
        }
        out.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok(out)
    }

    /// Llama a `f` con el rowid de cada página que cumple la expresión.
    fn each_row(&self, expr: &str, mut f: impl FnMut(i64)) -> Result<(), String> {
        let mut stmt = self.conn.prepare_cached("SELECT rowid FROM fts WHERE fts MATCH ?1 LIMIT ?2").map_err(db::err)?;
        let mut rows = stmt.query(params![expr, MAX_ROWS]).map_err(db::err)?;
        while let Some(row) = rows.next().map_err(db::err)? {
            f(row.get(0).map_err(db::err)?);
        }
        Ok(())
    }

    /// Fragmento de una página alrededor de su mejor coincidencia.
    fn snippet(&self, matcher: &Matcher, rowid: i64) -> Result<String, String> {
        let text: String = self.conn.prepare_cached("SELECT text FROM fts WHERE rowid = ?1").and_then(|mut s| s.query_row([rowid], |r| r.get(0))).map_err(db::err)?;
        let tokens = tokens_from_text(&text);
        let hits = matcher.find(&tokens);
        Ok(match hits.iter().find(|f| f.exact).or(hits.first()) {
            Some(f) => snippet(&text, &tokens, f),
            None => text.split_whitespace().take(SNIPPET_AFTER).collect::<Vec<_>>().join(" "),
        })
    }

    /// Prepara la consulta con las variantes del vocabulario del índice.
    pub fn matcher(&mut self, query: &str) -> Result<Option<Arc<Matcher>>, String> {
        if let Some((q, m)) = &self.last {
            if q == query {
                return Ok(Some(m.clone()));
            }
        }
        let Some(pattern) = Pattern::new(query) else { return Ok(None) };
        let n = pattern.words.len();
        let vocab = self.load_vocab()?;
        let mut accepted = Vec::with_capacity(n);
        let mut raw = Vec::with_capacity(n);
        for k in 0..n {
            let q = &pattern.words[k];
            let typing = pattern.prefix_last() && k + 1 == n && !q.digits;
            // Qué tan frecuente es la palabra tal cual: las variantes deben serlo menos
            let common: i64 = vocab.iter().filter(|v| v.normed == q.text).map(|v| v.docs).sum();
            let mut exact: Vec<&VocabTerm> = Vec::new();
            let mut close: Vec<&VocabTerm> = Vec::new();
            let mut resembling: Vec<(usize, &VocabTerm)> = Vec::new();
            for v in vocab {
                if common > 0 && v.docs > common {
                    continue;
                }
                match pattern.classify(k, &v.normed) {
                    Class::None => {}
                    // Los que empiezan igual los encuentra el índice como prefijo
                    Class::Exact if typing && v.normed != q.text => {}
                    Class::Exact => exact.push(v),
                    Class::Resembles if n == 1 => {}
                    Class::Resembles => resembling.push((distance(&q.chars, &v.normed, q.chars.len()).unwrap_or(usize::MAX), v)),
                    Class::Close => close.push(v),
                }
            }
            close.sort_by(|a, b| b.docs.cmp(&a.docs));
            resembling.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.docs.cmp(&a.1.docs)));
            let mut words: HashMap<String, Class> = HashMap::new();
            let mut terms = Vec::new();
            let chosen = exact
                .iter()
                .map(|v| (*v, Class::Exact))
                .chain(close.iter().take(MAX_VARIANTS).map(|v| (*v, Class::Close)))
                .chain(resembling.iter().take(RESEMBLING).map(|(_, v)| (*v, Class::Resembles)));
            for (v, class) in chosen {
                words.insert(v.normed.clone(), class);
                terms.push((v.raw.clone(), class));
            }
            accepted.push(words);
            raw.push(terms);
        }
        let m = Arc::new(Matcher { pattern, accepted, raw });
        self.last = Some((query.to_string(), m.clone()));
        Ok(Some(m))
    }

    fn load_vocab(&mut self) -> Result<&[VocabTerm], String> {
        if self.vocab.is_none() {
            let mut stmt = self.conn.prepare("SELECT term, doc FROM fts_vocab").map_err(db::err)?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(db::err)?;
            self.vocab = Some(rows.flatten().map(|(raw, docs)| VocabTerm { normed: norm(&raw), raw, docs }).collect());
        }
        Ok(self.vocab.as_deref().unwrap_or(&[]))
    }

    /// Palabras del índice que difieren de `folded` en a lo más `max` letras (incluida ella misma).
    pub fn variants(&mut self, folded: &str, max: usize) -> Result<Vec<String>, String> {
        let target: Vec<char> = folded.chars().collect();
        Ok(self.load_vocab()?.iter().filter(|v| distance(&target, &v.raw, max).is_some()).map(|v| v.raw.clone()).collect())
    }
}

fn quote(term: &str) -> String {
    format!("\"{}\"", term.replace('"', ""))
}

/// Fragmento del texto alrededor de una aparición, con la aparición entre U+0001 y U+0002.
fn snippet(text: &str, tokens: &[Token], f: &Found) -> String {
    let from = f.start.saturating_sub(SNIPPET_BEFORE);
    let to = (f.end + SNIPPET_AFTER).min(tokens.len());
    let (a, b) = (tokens[from].span.0, tokens[to - 1].span.1);
    let (ms, me) = (tokens[f.start].span.0, tokens[f.end - 1].span.1);
    let mut s = String::new();
    if from > 0 {
        s.push('…');
    }
    s.push_str(&text[a..ms]);
    s.push('\u{1}');
    s.push_str(&text[ms..me]);
    s.push('\u{2}');
    s.push_str(&text[me..b]);
    if to < tokens.len() {
        s.push('…');
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Separa la consulta igual que el tokenizador del índice: solo letras y dígitos.
pub fn tokenize(query: &str) -> Vec<String> {
    query.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(|t| t.to_string()).collect()
}

/// Minúsculas sin acentos, como guarda las palabras el índice.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        out.push(match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        });
    }
    out
}

/// Distancia de edición si no supera `max`.
pub fn distance(a: &[char], b: &str, max: usize) -> Option<usize> {
    if a.len().abs_diff(b.chars().count()) > max {
        return None;
    }
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut row_min = cur[0];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
            row_min = row_min.min(cur[j + 1]);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    (prev[b.len()] <= max).then_some(prev[b.len()])
}
