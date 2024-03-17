use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollinsItem {
    word: String,
    phonetic: Option<String>,
    sense: Option<String>,
    en_def: Option<String>,
    cn_def: Option<String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OxfordItem {
    word: String,
    phrase: Option<String>,
    phonetic: Option<String>,
    sense: Option<String>,
    ext: Option<String>,
    en_def: Option<String>,
    cn_def: Option<String>,
}

pub fn open_connection(dict_path: impl AsRef<Path>) -> Result<Connection, String> {
    fn inner(dict_path: &Path) -> Result<Connection, String> {
        use rusqlite::OpenFlags;
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(dict_path, flags)
            .map_err(|e| format!("failed to open resources/dict.db, error: {e}"))?;
        return Ok(conn);
    }
    return inner(dict_path.as_ref());
}

pub fn search_collins(
    conn: &Connection,
    word: impl AsRef<str>,
) -> Result<Vec<CollinsItem>, String> {
    fn inner(conn: &Connection, word: &str) -> Result<Vec<CollinsItem>, String> {
        let mut stmt = conn
            .prepare_cached("select * from collins where word = ?1 collate nocase order by rowid")
            .map_err(|e| format!("failed to prepare SQL statement for collins search: {e}"))?;
        let mut rows = stmt
            .query([word])
            .map_err(|e| format!("failed to query collins: {e}"))?;
        let mut items = vec![];
        while let Some(row) = rows
            .next()
            .map_err(|e| format!("failed to get next row from collins search result: {e}"))?
        {
            items.push(CollinsItem {
                word: row
                    .get("word")
                    .map_err(|e| format!("failed to get word: {e}"))?,
                phonetic: row
                    .get("phonetic")
                    .map_err(|e| format!("failed to get phonetic: {e}"))?,
                sense: row
                    .get("sense")
                    .map_err(|e| format!("failed to get sense: {e}"))?,
                en_def: row
                    .get("enDef")
                    .map_err(|e| format!("failed to get enDef: {e}"))?,
                cn_def: row
                    .get("cnDef")
                    .map_err(|e| format!("failed to get cnDef: {e}"))?,
            });
        }
        return Ok(items);
    }
    return inner(conn, word.as_ref());
}

pub fn search_oxford(conn: &Connection, word: impl AsRef<str>) -> Result<Vec<OxfordItem>, String> {
    fn inner(conn: &Connection, word: &str) -> Result<Vec<OxfordItem>, String> {
        let mut stmt = conn
            .prepare_cached("select * from oxford where word = ?1 collate nocase order by rowid")
            .map_err(|e| format!("failed to prepare SQL statement for oxford search: {e}"))?;
        let mut rows = stmt
            .query([word])
            .map_err(|e| format!("failed to query oxford: {e}"))?;
        let mut items = vec![];
        while let Some(row) = rows
            .next()
            .map_err(|e| format!("failed to get next row from oxford search result: {e}"))?
        {
            items.push(OxfordItem {
                word: row
                    .get("word")
                    .map_err(|e| format!("failed to get word: {e}"))?,
                phrase: row
                    .get("phrase")
                    .map_err(|e| format!("failed to get phrase: {e}"))?,
                phonetic: row
                    .get("phonetic")
                    .map_err(|e| format!("failed to get phonetic: {e}"))?,
                sense: row
                    .get("sense")
                    .map_err(|e| format!("failed to get sense: {e}"))?,
                ext: row
                    .get("ext")
                    .map_err(|e| format!("failed to get ext: {e}"))?,
                en_def: row
                    .get("enDef")
                    .map_err(|e| format!("failed to get enDef: {e}"))?,
                cn_def: row
                    .get("cnDef")
                    .map_err(|e| format!("failed to get cnDef: {e}"))?,
            });
        }
        return Ok(items);
    }
    return inner(conn, word.as_ref());
}

/// 获取单词的原型
pub fn get_word_base(conn: &Connection, word: impl AsRef<str>) -> Result<Option<String>, String> {
    fn inner(conn: &Connection, word: &str) -> Result<Option<String>, String> {
        let mut stmt = conn
            .prepare_cached("select * from forms where word = ?1 collate nocase")
            .map_err(|e| format!("failed to prepare SQL statement for word base search: {e}"))?;
        let base: Option<String> = stmt
            .query_row([word], |row| row.get("base"))
            .optional()
            .map_err(|e| format!("failed to query word base: {e}"))?;
        return Ok(base);
    }
    return inner(conn, word.as_ref());
}
