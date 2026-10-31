use std::ops::DerefMut;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

use super::logics;
use super::logics::dict::{CollinsItem, OxfordItem};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct DictPath(pub String);

impl DictPath {
    pub fn new(
        portable: bool,
        path_resolver: &tauri::path::PathResolver<impl tauri::Runtime>,
    ) -> Result<Self, String> {
        let dict_path = if portable {
            logics::utils::current_exe_dir()?
                .join("resources")
                .join("dict.db")
        } else {
            path_resolver
                .resolve("resources/dict.db", BaseDirectory::Resource)
                .map_err(|e| format!("failed to resolve resources/dict.db: {e}"))?
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

/// 取得（必要时打开）词典连接并执行查询。
///
/// 在阻塞线程池中调用（见各查询命令）：冷启动首次查询要读取数十 MB 的词典文件，
/// 且查询语句用了 collate nocase、无法命中按默认排序规则建的索引而全表扫描，
/// 放在主线程会造成明显卡顿。
fn with_connection<T>(
    app: &AppHandle,
    query: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let conn = app.state::<Mutex<Option<Connection>>>();
    let dict_path = app.state::<DictPath>();
    let dict_path: &Path = dict_path.0.as_ref();
    let mut guard = conn
        .lock()
        .map_err(|e| format!("failed to lock connection: {e}"))?;
    let conn = guard.connection(dict_path)?;
    return query(conn);
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search_collins(word: String, app: AppHandle) -> Result<Vec<CollinsItem>, String> {
    return logics::utils::run_blocking(move || {
        with_connection(&app, |conn| logics::dict::search_collins(conn, word))
    })
    .await;
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search_oxford(word: String, app: AppHandle) -> Result<Vec<OxfordItem>, String> {
    return logics::utils::run_blocking(move || {
        with_connection(&app, |conn| logics::dict::search_oxford(conn, word))
    })
    .await;
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_word_base(word: String, app: AppHandle) -> Result<Option<String>, String> {
    return logics::utils::run_blocking(move || {
        with_connection(&app, |conn| logics::dict::get_word_base(conn, word))
    })
    .await;
}

#[tauri::command(rename_all = "snake_case")]
pub fn sanitize_filename(filename: String) -> Result<String, String> {
    return Ok(sanitise_file_name::sanitise(&filename));
}
