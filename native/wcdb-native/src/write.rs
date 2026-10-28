//! Write-side operations: message mutation, session bookkeeping, triggers.

use crate::core::*;
use crate::features;
use rusqlite::params;
use serde_json::{json, Value};
use std::path::Path;

fn session_columns(conn: &rusqlite::Connection) -> Vec<String> {
    features::table_columns(conn, "SessionTable")
}

fn has_column(columns: &[String], name: &str) -> bool {
    columns.iter().any(|column| column.eq_ignore_ascii_case(name))
}

fn session_key_column(columns: &[String]) -> Option<&'static str> {
    if has_column(columns, "username") {
        Some("username")
    } else if has_column(columns, "user_name") {
        Some("user_name")
    } else {
        None
    }
}

fn update_session_row(
    conn: &rusqlite::Connection,
    session_id: &str,
    create_time: i64,
    summary: Option<&str>,
    sender: Option<&str>,
) -> bool {
    let columns = session_columns(conn);
    let Some(key) = session_key_column(&columns) else {
        return false;
    };
    let mut sets = Vec::new();
    if has_column(&columns, "sort_timestamp") {
        sets.push(format!("sort_timestamp = {create_time}"));
    }
    if has_column(&columns, "last_timestamp") {
        sets.push(format!("last_timestamp = {create_time}"));
    }
    if let Some(summary) = summary {
        if has_column(&columns, "summary") {
            sets.push(format!("summary = {}", sql_string(summary)));
        } else if has_column(&columns, "digest") {
            sets.push(format!("digest = {}", sql_string(summary)));
        }
    }
    if has_column(&columns, "last_msg_type") {
        sets.push("last_msg_type = 1".to_string());
    }
    if let Some(sender) = sender {
        if has_column(&columns, "last_msg_sender") {
            sets.push(format!("last_msg_sender = {}", sql_string(sender)));
        }
    }
    if sets.is_empty() {
        return false;
    }
    let sql = format!(
        "UPDATE SessionTable SET {} WHERE {} = {}",
        sets.join(", "),
        key,
        sql_string(session_id)
    );
    conn.execute(&sql, []).map(|rows| rows > 0).unwrap_or(false)
}

pub fn update_message(
    acc: &Account,
    session_id: &str,
    local_id: i64,
    create_time: i64,
    new_content: &str,
) -> Result<(), String> {
    let table = message_table_name(session_id);
    for shard in features::open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let conn = acc.db_rw(&shard.path).map_err(|code| format!("db open failed: {code}"))?;
        let sql = format!(
            "UPDATE \"{table}\" SET message_content = ?1 \
             WHERE local_id = ?2 AND create_time = ?3"
        );
        let changed = conn
            .execute(&sql, params![new_content, local_id, create_time])
            .map_err(|e| e.to_string())?;
        if changed > 0 {
            return Ok(());
        }
    }
    Err("message not found".to_string())
}

pub fn insert_text_message(
    acc: &Account,
    session_id: &str,
    content: &str,
    status: i64,
    create_time: i64,
) -> Result<Value, String> {
    let table = message_table_name(session_id);
    let created = if create_time > 0 {
        create_time
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    };
    let my_id = acc.my_account_id();
    let shards = features::open_shards(acc, &table);
    let target = shards
        .iter()
        .find(|shard| shard.has_table)
        .map(|shard| shard.path.clone())
        .or_else(|| acc.message_dbs().into_iter().next())
        .ok_or_else(|| "no message database".to_string())?;
    let conn = acc.db_rw(&target).map_err(|code| format!("db open failed: {code}"))?;
    if !table_exists(&conn, &table) {
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS \"{table}\" (\
             local_id INTEGER PRIMARY KEY AUTOINCREMENT, server_id INTEGER, local_type INTEGER, \
             sort_seq INTEGER, real_sender_id INTEGER, create_time INTEGER, status INTEGER, \
             message_content TEXT, compress_content TEXT, packed_info_data BLOB)"
        ))
        .map_err(|e| e.to_string())?;
    }
    let columns = features::table_columns(&conn, &table);
    let sender_row_id = if my_id.is_empty() {
        0
    } else {
        let existing: Option<i64> = conn
            .query_row(
                "SELECT rowid FROM Name2Id WHERE user_name = ?1 LIMIT 1",
                params![my_id],
                |row| row.get(0),
            )
            .ok();
        match existing {
            Some(id) => id,
            None => {
                conn.execute("INSERT INTO Name2Id(user_name) VALUES (?1)", params![my_id])
                    .map_err(|e| e.to_string())?;
                conn.last_insert_rowid()
            }
        }
    };
    let next_local: i64 = conn
        .query_row(
            &format!("SELECT coalesce(max(local_id), 0) + 1 FROM \"{table}\""),
            [],
            |row| row.get(0),
        )
        .unwrap_or(1);
    let next_sort: i64 = conn
        .query_row(
            &format!("SELECT coalesce(max(sort_seq), 0) + 1 FROM \"{table}\""),
            [],
            |row| row.get(0),
        )
        .unwrap_or(1);
    let mut names = vec!["local_id", "server_id", "local_type", "sort_seq", "real_sender_id", "create_time", "status", "message_content"];
    let mut markers = vec!["?1", "0", "1", "?2", "?3", "?4", "?5", "?6"];
    let values: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(next_local),
        Box::new(next_sort),
        Box::new(sender_row_id),
        Box::new(created),
        Box::new(status),
        Box::new(content.to_string()),
    ];
    if has_column(&columns, "compress_content") {
        names.push("compress_content");
        markers.push("''");
    }
    let params_refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|value| value.as_ref()).collect();
    let sql = format!(
        "INSERT INTO \"{table}\" ({}) VALUES ({})",
        names.join(", "),
        markers.join(", ")
    );
    conn.execute(&sql, params_refs.as_slice())
        .map_err(|e| e.to_string())?;
    let session_updated = {
        let session_conn = acc
            .db_rw(&acc.session_db())
            .map_err(|code| format!("session db open failed: {code}"))?;
        update_session_row(&session_conn, session_id, created, Some(content), Some(&my_id))
    };
    Ok(json!({
        "success": true,
        "local_id": next_local,
        "create_time": created,
        "sort_seq": next_sort,
        "status": status,
        "sender_row_id": sender_row_id,
        "session_updated": session_updated,
        "db_path": target.to_string_lossy(),
        "table_name": table,
    }))
}

pub fn delete_message(
    acc: &Account,
    session_id: &str,
    local_id: i64,
    create_time: i64,
    db_path_hint: &str,
) -> Result<(), String> {
    let table = message_table_name(session_id);
    let hint = db_path_hint.trim();
    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    if !hint.is_empty() && Path::new(hint).is_file() {
        targets.push(std::path::PathBuf::from(hint));
    } else {
        targets.extend(
            features::open_shards(acc, &table)
                .into_iter()
                .filter(|shard| shard.has_table)
                .map(|shard| shard.path),
        );
    }
    let mut deleted = false;
    for path in targets {
        let conn = acc.db_rw(&path).map_err(|code| format!("db open failed: {code}"))?;
        if !table_exists(&conn, &table) {
            continue;
        }
        let sql = format!(
            "DELETE FROM \"{table}\" WHERE local_id = ?1 AND create_time = ?2"
        );
        let changed = conn
            .execute(&sql, params![local_id, create_time])
            .map_err(|e| e.to_string())?;
        if changed > 0 {
            deleted = true;
            if let Ok(mut stmt) = conn.prepare(&format!(
                "SELECT create_time, message_content FROM \"{table}\" \
                 ORDER BY create_time DESC, sort_seq DESC, local_id DESC LIMIT 1"
            )) {
                if let Ok(mut rows) = stmt.query([]) {
                    if let Ok(Some(row)) = rows.next() {
                        let last_time: i64 = row.get(0).unwrap_or(0);
                        let last_text: String = row.get(1).unwrap_or_default();
                        if let Ok(session_conn) = acc.db_rw(&acc.session_db()) {
                            update_session_row(&session_conn, session_id, last_time, Some(&last_text), None);
                        }
                    }
                }
            }
        }
    }
    if deleted {
        Ok(())
    } else {
        Err("message not found".to_string())
    }
}

pub fn mark_all_sessions_read(acc: &Account) -> Result<(), String> {
    let conn = acc
        .db_rw(&acc.session_db())
        .map_err(|code| format!("session db open failed: {code}"))?;
    let columns = session_columns(&conn);
    let column = if has_column(&columns, "unread_count") {
        "unread_count"
    } else if has_column(&columns, "unreadCount") {
        "unreadCount"
    } else {
        return Ok(());
    };
    conn.execute(&format!("UPDATE SessionTable SET {column} = 0"), [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn reorder_sessions_by_time(acc: &Account) -> Result<Value, String> {
    let conn = acc
        .db_rw(&acc.session_db())
        .map_err(|code| format!("session db open failed: {code}"))?;
    let columns = session_columns(&conn);
    let sort_column = if has_column(&columns, "sort_timestamp") {
        "sort_timestamp"
    } else if has_column(&columns, "sortTimestamp") {
        "sortTimestamp"
    } else {
        return Ok(json!({
            "success": false,
            "stage": "schema",
            "error": "sort timestamp column missing",
            "code": ERR_SCHEMA,
        }));
    };
    let source_column = if has_column(&columns, "last_timestamp") {
        "last_timestamp"
    } else if has_column(&columns, "lastTimestamp") {
        "lastTimestamp"
    } else {
        return Ok(json!({
            "success": false,
            "stage": "schema",
            "error": "last timestamp column missing",
            "code": ERR_SCHEMA,
        }));
    };
    let row_count: i64 = conn
        .query_row("SELECT count(*) FROM SessionTable", [], |row| row.get(0))
        .unwrap_or(0);
    let updated = conn
        .execute(
            &format!(
                "UPDATE SessionTable SET {sort_column} = {source_column} \
                 WHERE {sort_column} IS NOT {source_column}"
            ),
            [],
        )
        .unwrap_or(0);
    Ok(json!({
        "success": true,
        "table": "session",
        "sort_column": sort_column,
        "source_column": source_column,
        "row_count": row_count,
        "updated_rows": updated,
        "already_normalized": updated == 0,
    }))
}

fn trigger_hash(table: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(table.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

fn anti_revoke_names(table: &str) -> (String, String, String) {
    let hash = trigger_hash(table);
    (
        format!("weflow_ar_{hash}_shadow"),
        format!("weflow_ar_{hash}_update"),
        format!("weflow_ar_{hash}_delete"),
    )
}

fn install_anti_revoke_in(conn: &rusqlite::Connection, table: &str) -> Result<bool, String> {
    let (shadow, update_trigger, delete_trigger) = anti_revoke_names(table);
    let installed: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name IN (?1, ?2)",
            params![update_trigger, delete_trigger],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if installed >= 2 {
        return Ok(true);
    }
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS \"{shadow}\" (\
         local_id INTEGER, create_time INTEGER, sort_seq INTEGER, real_sender_id INTEGER, \
         message_content TEXT, compress_content TEXT, saved_at INTEGER);\
         CREATE TRIGGER IF NOT EXISTS \"{update_trigger}\" AFTER UPDATE ON \"{table}\" \
         BEGIN INSERT INTO \"{shadow}\" (local_id, create_time, sort_seq, real_sender_id, message_content, compress_content, saved_at) \
         VALUES (OLD.local_id, OLD.create_time, OLD.sort_seq, OLD.real_sender_id, OLD.message_content, OLD.compress_content, strftime('%s','now')); END;\
         CREATE TRIGGER IF NOT EXISTS \"{delete_trigger}\" AFTER DELETE ON \"{table}\" \
         BEGIN INSERT INTO \"{shadow}\" (local_id, create_time, sort_seq, real_sender_id, message_content, compress_content, saved_at) \
         VALUES (OLD.local_id, OLD.create_time, OLD.sort_seq, OLD.real_sender_id, OLD.message_content, OLD.compress_content, strftime('%s','now')); END;"
    ))
    .map_err(|e| e.to_string())?;
    Ok(false)
}

pub fn install_anti_revoke(acc: &Account, session_id: &str) -> Result<bool, String> {
    let table = message_table_name(session_id);
    let shards: Vec<_> = features::open_shards(acc, &table)
        .into_iter()
        .filter(|shard| shard.has_table)
        .collect();
    if shards.is_empty() {
        return Err("message table not found".to_string());
    }
    let mut already = true;
    for shard in shards {
        let conn = acc.db_rw(&shard.path).map_err(|code| format!("db open failed: {code}"))?;
        if !install_anti_revoke_in(&conn, &table)? {
            already = false;
        }
    }
    Ok(already)
}

pub fn uninstall_anti_revoke(acc: &Account, session_id: &str) -> Result<(), String> {
    let table = message_table_name(session_id);
    let (_, update_trigger, delete_trigger) = anti_revoke_names(&table);
    for shard in features::open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let conn = acc.db_rw(&shard.path).map_err(|code| format!("db open failed: {code}"))?;
        conn.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS \"{update_trigger}\";\
             DROP TRIGGER IF EXISTS \"{delete_trigger}\";"
        ))
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn check_anti_revoke(acc: &Account, session_id: &str) -> bool {
    let table = message_table_name(session_id);
    let (_, update_trigger, delete_trigger) = anti_revoke_names(&table);
    for shard in features::open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let conn = match acc.db(&shard.path) {
            Ok(conn) => conn,
            Err(_) => continue,
        };
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        let count: i64 = guard
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name IN (?1, ?2)",
                params![update_trigger, delete_trigger],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if count >= 2 {
            return true;
        }
    }
    false
}

const SNS_BLOCK_TRIGGER: &str = "weflow_sns_block_delete";

pub fn install_sns_block_delete(acc: &Account) -> Result<bool, String> {
    let conn = acc
        .db_rw(&acc.sns_db())
        .map_err(|code| format!("sns db open failed: {code}"))?;
    if !table_exists(&conn, "SnsTimeLine") {
        return Err("SnsTimeLine not found".to_string());
    }
    let existing: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name = ?1",
            params![SNS_BLOCK_TRIGGER],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if existing > 0 {
        return Ok(true);
    }
    conn.execute_batch(&format!(
        "CREATE TRIGGER \"{SNS_BLOCK_TRIGGER}\" BEFORE DELETE ON SnsTimeLine \
         BEGIN SELECT RAISE(IGNORE); END;"
    ))
    .map_err(|e| e.to_string())?;
    Ok(false)
}

pub fn uninstall_sns_block_delete(acc: &Account) -> Result<(), String> {
    let conn = acc
        .db_rw(&acc.sns_db())
        .map_err(|code| format!("sns db open failed: {code}"))?;
    conn.execute_batch(&format!("DROP TRIGGER IF EXISTS \"{SNS_BLOCK_TRIGGER}\";"))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn check_sns_block_delete(acc: &Account) -> bool {
    let Some(conn) = acc.db_opt(&acc.sns_db()) else {
        return false;
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name = ?1",
            params![SNS_BLOCK_TRIGGER],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false)
}

pub fn delete_sns_post(acc: &Account, post_id: &str) -> Result<(), String> {
    let conn = acc
        .db_rw(&acc.sns_db())
        .map_err(|code| format!("sns db open failed: {code}"))?;
    let was_blocked: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name = ?1",
            params![SNS_BLOCK_TRIGGER],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if was_blocked > 0 {
        conn.execute_batch(&format!("DROP TRIGGER \"{SNS_BLOCK_TRIGGER}\";"))
            .map_err(|e| e.to_string())?;
    }
    let trimmed = post_id.trim();
    let id_i64 = trimmed.parse::<i64>().ok();
    let mut deleted = 0usize;
    if let Some(id) = id_i64 {
        deleted += conn
            .execute("DELETE FROM SnsTimeLine WHERE tid = ?1", params![id])
            .unwrap_or(0);
        let _ = conn.execute(
            "DELETE FROM SnsMessage_tmp3 WHERE feed_id = ?1",
            params![id],
        );
    }
    if deleted == 0 {
        deleted += conn
            .execute(
                "DELETE FROM SnsTimeLine WHERE CAST(tid AS TEXT) = ?1",
                params![trimmed],
            )
            .unwrap_or(0);
        let _ = conn.execute(
            "DELETE FROM SnsMessage_tmp3 WHERE CAST(feed_id AS TEXT) = ?1",
            params![trimmed],
        );
    }
    if was_blocked > 0 {
        let _ = conn.execute_batch(&format!(
            "CREATE TRIGGER \"{SNS_BLOCK_TRIGGER}\" BEFORE DELETE ON SnsTimeLine \
             BEGIN SELECT RAISE(IGNORE); END;"
        ));
    }
    if deleted > 0 {
        Ok(())
    } else {
        Err("post not found".to_string())
    }
}
