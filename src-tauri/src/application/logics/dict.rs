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

/// 查询牛津词典。除了词头（word），还会匹配短语列（phrase）——库里有一万多条短语词条
/// （如 word=gun、phrase=jump the gun），过去只按词头精确匹配，这些词条只有查词头时才看得到，
/// 直接查短语本身是 0 结果。
///
/// 短语列没有索引，这条 OR 查询会全表扫描，因此只在输入含空白时启用；
/// 单词查询保持只走 word 索引的快路径。
pub fn search_oxford(conn: &Connection, word: impl AsRef<str>) -> Result<Vec<OxfordItem>, String> {
    fn inner(conn: &Connection, word: &str) -> Result<Vec<OxfordItem>, String> {
        let sql = if word.chars().any(char::is_whitespace) {
            // 短语命中的行里有一批只有词头+短语、没有任何释义的空壳（如 word=jump 的空条目），
            // 直接显示会是一张空卡片，因此该分支要求至少有一项内容
            "select * from oxford \
             where word = ?1 collate nocase \
                or (phrase = ?1 collate nocase and coalesce(sense, enDef, cnDef, ext) is not null) \
             order by case when word = ?1 collate nocase then 0 else 1 end, rowid"
        } else {
            "select * from oxford where word = ?1 collate nocase order by rowid"
        };
        let mut stmt = conn
            .prepare_cached(sql)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 直接读仓库里的真实词典库（只读）。手写建表的 fixture 会和发布产物悄悄脱节：
    /// 列名改了、空壳规则变了，fixture 仍然自洽，测试照样全绿。
    fn real_db() -> Connection {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/dict.db");
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        assert!(
            size > 1024 * 1024,
            "resources/dict.db 不可用（{size} 字节），请先执行 git lfs pull"
        );
        return open_connection(&path).expect("failed to open resources/dict.db");
    }

    /// 查询语句读到的列必须真实存在，否则运行期只会得到空结果或报错
    #[test]
    fn query_columns_exist_in_the_real_dictionary() {
        let conn = real_db();
        let columns = |table: &str| -> Vec<String> {
            let mut stmt = conn
                .prepare(&format!("pragma table_info({table})"))
                .expect("failed to prepare pragma");
            return stmt
                .query_map([], |row| row.get::<_, String>(1))
                .expect("failed to query pragma")
                .map(|name| name.expect("failed to read column name"))
                .collect();
        };
        assert_eq!(
            columns("oxford"),
            ["word", "phrase", "phonetic", "sense", "ext", "enDef", "cnDef"]
        );
        assert_eq!(
            columns("collins"),
            ["word", "phonetic", "sense", "enDef", "cnDef"]
        );
        assert_eq!(columns("forms"), ["word", "base"]);
    }

    #[test]
    fn phrase_match_returns_the_entry_that_owns_it() {
        let conn = real_db();
        // 真实数据：gun 的词条带短语 jump the gun，jump 那条只剩词头与短语
        let items = search_oxford(&conn, "jump the gun").expect("query failed");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].word, "gun");
        assert_eq!(items[0].phrase.as_deref(), Some("jump the gun"));
        assert!(items[0].cn_def.is_some());
    }

    #[test]
    fn word_match_precedes_phrase_match() {
        let conn = real_db();
        // old bean 既是词头，也是 bean 的短语；词头命中的那条必须排在前面
        let items = search_oxford(&conn, "old bean").expect("query failed");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].word, "old bean");
        assert_eq!(items[1].word, "bean");
    }

    #[test]
    fn phrase_entries_without_content_are_skipped() {
        let conn = real_db();
        // 空壳样例从真实库里挑，字典更新后自动跟着变，不用手改 fixture
        let shell = conn
            .query_row(
                "select s.word, s.phrase from oxford s \
                 where s.phrase is not null \
                   and instr(s.phrase, ' ') > 0 \
                   and coalesce(s.sense, s.enDef, s.cnDef, s.ext) is null \
                   and exists ( \
                       select 1 from oxford o \
                       where o.phrase = s.phrase collate nocase \
                         and coalesce(o.sense, o.enDef, o.cnDef, o.ext) is not null \
                   ) \
                 limit 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .expect("query failed");
        let (shell_word, phrase) = shell.expect("词典里应仍有空壳短语");
        let items = search_oxford(&conn, &phrase).expect("query failed");
        assert!(!items.is_empty(), "短语 {phrase} 应有可展示的词条");
        assert!(
            items.iter().all(|item| item.word != shell_word),
            "空壳短语 {phrase} 不应作为 {shell_word} 返回"
        );
    }

    #[test]
    fn single_word_query_does_not_scan_phrases() {
        let conn = real_db();
        // balances 没有词头，只出现在 checks and balances 里：单词查询走索引，不扫短语
        assert!(search_oxford(&conn, "balances")
            .expect("query failed")
            .is_empty());
        assert!(!search_oxford(&conn, "checks and balances")
            .expect("query failed")
            .is_empty());
    }
}
