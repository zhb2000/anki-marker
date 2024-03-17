use std::ops::DerefMut;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use tauri::State;

use super::logics;
use super::logics::dict::{CollinsItem, OxfordItem};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct DictPath(pub String);

impl DictPath {
    pub fn new(portable: bool, path_resolver: tauri::PathResolver) -> Result<Self, String> {
        let dict_path = if portable {
            logics::utils::current_exe_dir()?
                .join("resources")
                .join("dict.db")
        } else {
            path_resolver
                .resolve_resource("resources/dict.db")
                .ok_or("failed to resolve resource dict.db")?
        };
        return Ok(DictPath(dict_path.to_string_lossy().into_owned()));
    }
}

trait GetConnection<'a> {
    fn connection(&'a mut self, dict_path: &Path) -> Result<&'a mut Connection, String>;
}

impl<'a> GetConnection<'a> for MutexGuard<'_, Option<Connection>> {
    fn connection(&'a mut self, dict_path: &Path) -> Result<&'a mut Connection, String> {
        let opt_conn = self.deref_mut();
        let conn = match opt_conn {
            Some(conn) => conn,
            None => {
                let conn = logics::dict::open_connection(dict_path)?;
                *opt_conn = Some(conn);
                opt_conn.as_mut().expect("unexpected None")
            }
        };
        return Ok(conn);
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn search_collins(
    word: String,
    conn: State<Mutex<Option<Connection>>>,
    dict_path: State<DictPath>,
) -> Result<Vec<CollinsItem>, String> {
    let dict_path: &Path = dict_path.0.as_ref();
    let mut guard = conn
        .lock()
        .map_err(|e| format!("failed to lock connection: {e}"))?;
    let conn = guard.connection(dict_path)?;
    return logics::dict::search_collins(conn, word);
}

#[tauri::command(rename_all = "snake_case")]
pub fn search_oxford(
    word: String,
    conn: State<Mutex<Option<Connection>>>,
    dict_path: State<DictPath>,
) -> Result<Vec<OxfordItem>, String> {
    let dict_path: &Path = dict_path.0.as_ref();
    let mut guard = conn
        .lock()
        .map_err(|e| format!("failed to lock connection: {e}"))?;
    let conn = guard.connection(dict_path)?;
    return logics::dict::search_oxford(conn, word);
}

#[tauri::command(rename_all = "snake_case")]
pub fn get_word_base(
    word: String,
    conn: State<Mutex<Option<Connection>>>,
    dict_path: State<DictPath>,
) -> Result<Option<String>, String> {
    let dict_path: &Path = dict_path.0.as_ref();
    let mut guard = conn
        .lock()
        .map_err(|e| format!("failed to lock connection: {e}"))?;
    let conn = guard.connection(dict_path)?;
    return logics::dict::get_word_base(conn, word);
}
