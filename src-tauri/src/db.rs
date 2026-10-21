//! Índice persistente: un archivo SQLite por carpeta, con texto completo en FTS5.

use rusqlite::Connection;
use std::path::Path;

/// El rowid de cada fila de texto es `id_archivo << PAGE_BITS | página`.
/// La página 0 guarda el nombre del archivo para poder buscarlo también.
pub const PAGE_BITS: i64 = 20;
pub const PAGE_MASK: i64 = (1 << PAGE_BITS) - 1;

pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("No se pudo abrir el índice: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(30)).map_err(err)?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = -65536;
        PRAGMA mmap_size = 268435456;
        CREATE TABLE IF NOT EXISTS files (
            id          INTEGER PRIMARY KEY,
            rel         TEXT NOT NULL UNIQUE,
            size        INTEGER NOT NULL,
            mtime       INTEGER NOT NULL,
            pages       INTEGER NOT NULL DEFAULT 0,
            ocr_pages   INTEGER NOT NULL DEFAULT 0,
            ocr_pending INTEGER NOT NULL DEFAULT 0,
            error       TEXT,
            indexed_at  INTEGER NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(text, tokenize = "unicode61 remove_diacritics 2");
        CREATE VIRTUAL TABLE IF NOT EXISTS fts_vocab USING fts5vocab(fts, 'row');
        "#,
    )
    .map_err(err)?;
    Ok(conn)
}

#[derive(serde::Serialize, Clone, Default)]
pub struct Stats {
    pub files: i64,
    pub pages: i64,
    pub ocr_pages: i64,
    pub ocr_pending: i64,
    pub errors: i64,
}

pub fn stats(conn: &Connection) -> Result<Stats, String> {
    conn.query_row(
        "SELECT count(*), coalesce(sum(pages),0), coalesce(sum(ocr_pages),0), coalesce(sum(ocr_pending),0), count(error) FROM files",
        [],
        |r| {
            Ok(Stats {
                files: r.get(0)?,
                pages: r.get(1)?,
                ocr_pages: r.get(2)?,
                ocr_pending: r.get(3)?,
                errors: r.get(4)?,
            })
        },
    )
    .map_err(err)
}

pub fn err(e: rusqlite::Error) -> String {
    format!("Error del índice: {e}")
}
