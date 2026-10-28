//! Read-oriented feature implementations exposed through the C ABI.

use crate::core::*;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn rows_or_empty(
    result: rusqlite::Result<Vec<Value>>,
) -> Result<Vec<Value>, i32> {
    match result {
        Ok(rows) => Ok(rows),
        Err(rusqlite::Error::SqliteFailure(_, _)) => Err(ERR_SQL),
        Err(_) => Err(ERR_SQL),
    }
}

fn int_field(map: &Map<String, Value>, key: &str) -> i64 {
    match map.get(key) {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        Some(Value::String(text)) => text.parse::<i64>().unwrap_or(0),
        _ => 0,
    }
}

pub fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let quoted = table.replace('"', "\"\"");
    query_json(conn, &format!("PRAGMA table_info(\"{quoted}\")"), &[])
        .map(|rows| {
            rows.into_iter()
                .filter_map(|row| match row {
                    Value::Object(map) => {
                        let name = value_str(&map, &["name"]);
                        if name.is_empty() {
                            None
                        } else {
                            Some(name)
                        }
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn table_names(conn: &Connection) -> Vec<String> {
    query_json(
        conn,
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        &[],
    )
    .map(|rows| {
        rows.into_iter()
            .filter_map(|row| match row {
                Value::Object(map) => {
                    let name = value_str(&map, &["name"]);
                    if name.is_empty() {
                        None
                    } else {
                        Some(name)
                    }
                }
                _ => None,
            })
            .collect()
    })
    .unwrap_or_default()
}

/* ------------------------------- sessions ------------------------------- */

fn contact_local_types(acc: &Account) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    let Some(conn) = acc.db_opt(&acc.contact_db()) else {
        return map;
    };
    let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(rows) = query_json(&conn, "SELECT username, local_type FROM contact", &[]) {
        for row in rows {
            if let Value::Object(obj) = row {
                let username = value_str(&obj, &["username"]);
                if !username.is_empty() {
                    map.insert(username, int_field(&obj, "local_type"));
                }
            }
        }
    }
    map
}

pub fn get_sessions(acc: &Account) -> Result<Vec<Value>, i32> {
    let conn = acc.db(&acc.session_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows = rows_or_empty(query_json(
        &guard,
        "SELECT * FROM SessionTable ORDER BY sort_timestamp DESC, last_timestamp DESC",
        &[],
    ))?;
    drop(guard);

    let local_types = contact_local_types(acc);
    if !local_types.is_empty() {
        for row in rows.iter_mut() {
            if let Value::Object(map) = row {
                let username = value_str(map, &["username", "user_name"]);
                if let Some(local_type) = local_types.get(&username) {
                    map.insert("local_type".to_string(), Value::from(*local_type));
                }
            }
        }
    }
    Ok(rows)
}

/* ------------------------------- messages ------------------------------- */

pub const MESSAGE_BASE_COLUMNS: &str =
    "local_id, server_id, local_type, sort_seq, real_sender_id, create_time, status, message_content, compress_content";

pub struct ShardContext {
    pub path: PathBuf,
    pub conn: Arc<Mutex<Connection>>,
    pub name2id: Arc<HashMap<i64, String>>,
    pub has_table: bool,
}

impl ShardContext {
    pub fn query(&self, sql: &str) -> Vec<Value> {
        let guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        query_json(&guard, sql, &[]).unwrap_or_default()
    }

    pub fn scalar(&self, sql: &str, field: &str) -> i64 {
        let guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        query_json(&guard, sql, &[])
            .ok()
            .and_then(|rows| match rows.first() {
                Some(Value::Object(map)) => Some(int_field(map, field)),
                _ => None,
            })
            .unwrap_or(0)
    }

    pub fn columns_of(&self, table: &str) -> Vec<String> {
        let guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        table_columns(&guard, table)
    }

    /// Message columns plus `packed_info_data` when the table has it; newer
    /// WeChat image/video XML omits the md5 attribute and the app resolves
    /// media dat names from this blob.
    pub fn message_columns(&self, table: &str) -> String {
        let columns = self.columns_of(table);
        if columns.iter().any(|name| name == "packed_info_data") {
            format!("{MESSAGE_BASE_COLUMNS}, packed_info_data")
        } else {
            MESSAGE_BASE_COLUMNS.to_string()
        }
    }

    pub fn sender_name(&self, rowid: i64) -> Option<String> {
        self.name2id.get(&rowid).cloned()
    }
}

pub fn open_shards(acc: &Account, table: &str) -> Vec<ShardContext> {
    let mut shards = Vec::new();
    for path in acc.message_dbs() {
        let Some(conn) = acc.db_opt(&path) else {
            continue;
        };
        let has_table = {
            let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
            table_exists(&guard, table)
        };
        let name2id = if has_table {
            acc.name2id(&path)
        } else {
            Arc::new(HashMap::new())
        };
        shards.push(ShardContext {
            path,
            conn,
            name2id,
            has_table,
        });
    }
    shards
}

pub fn decorate_message(
    acc: &Account,
    shard: &ShardContext,
    table: &str,
    row: &mut Map<String, Value>,
) {
    let sender = shard
        .name2id
        .get(&int_field(row, "real_sender_id"))
        .cloned();
    let my_id = acc.my_account_id();
    let is_send = if let Some(sender) = sender.as_deref() {
        if my_id.is_empty() {
            0
        } else if sender == my_id {
            1
        } else {
            0
        }
    } else {
        0
    };
    row.insert(
        "sender_username".to_string(),
        sender.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert("is_send".to_string(), Value::from(is_send));
    if let Some(Value::Number(number)) = row.get("server_id") {
        let text = Value::String(number.to_string());
        row.insert("server_id".to_string(), text);
    }
    row.insert(
        "_db_path".to_string(),
        Value::String(shard.path.to_string_lossy().into_owned()),
    );
    row.insert(
        "_db_name".to_string(),
        Value::String(
            shard
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
        ),
    );
    row.insert("_table_name".to_string(), Value::String(table.to_string()));
}

fn order_key(row: &Map<String, Value>) -> (i64, i64, i64) {
    (
        int_field(row, "create_time"),
        int_field(row, "sort_seq"),
        int_field(row, "local_id"),
    )
}

pub fn get_messages(
    acc: &Account,
    session_id: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Value>, i32> {
    let table = message_table_name(session_id);
    let limit = if limit <= 0 { usize::MAX } else { limit as usize };
    let offset = offset.max(0) as usize;
    let shards = open_shards(acc, &table);
    let mut merged: Vec<Value> = Vec::new();
    for shard in &shards {
        if !shard.has_table {
            continue;
        }
        let columns = shard.message_columns(&table);
        let sql = format!(
            "SELECT {columns} FROM \"{table}\" \
             ORDER BY create_time DESC, sort_seq DESC, local_id DESC LIMIT {}",
            fetch_cap(limit, offset)
        );
        for mut row in shard.query(&sql) {
            if let Value::Object(map) = &mut row {
                decorate_message(acc, shard, &table, map);
            }
            merged.push(row);
        }
    }
    merged.sort_by(|a, b| match (a, b) {
        (Value::Object(x), Value::Object(y)) => order_key(y).cmp(&order_key(x)),
        _ => std::cmp::Ordering::Equal,
    });
    Ok(merged.into_iter().skip(offset).take(limit).collect())
}

fn fetch_cap(limit: usize, offset: usize) -> i64 {
    if limit == usize::MAX {
        -1
    } else {
        limit.saturating_add(offset).min(i64::MAX as usize) as i64
    }
}

pub fn get_message_count(acc: &Account, session_id: &str) -> Result<i64, i32> {
    let table = message_table_name(session_id);
    let mut total = 0i64;
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let sql = format!("SELECT count(*) AS c FROM \"{table}\"");
        total += shard.scalar(&sql, "c");
    }
    Ok(total)
}

fn find_message(
    acc: &Account,
    session_id: &str,
    predicate: &str,
) -> Result<Option<Value>, i32> {
    let table = message_table_name(session_id);
    let mut found: Option<(i64, Value)> = None;
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let columns = shard.message_columns(&table);
        let sql = format!(
            "SELECT {columns} FROM \"{table}\" WHERE {predicate} \
             ORDER BY create_time DESC, sort_seq DESC, local_id DESC LIMIT 1"
        );
        for mut row in shard.query(&sql) {
            let create_time = match &row {
                Value::Object(map) => int_field(map, "create_time"),
                _ => 0,
            };
            if let Value::Object(map) = &mut row {
                decorate_message(acc, &shard, &table, map);
            }
            if found.as_ref().map(|(time, _)| create_time > *time).unwrap_or(true) {
                found = Some((create_time, row));
            }
        }
    }
    Ok(found.map(|(_, row)| row))
}

pub fn get_message_by_server_id(
    acc: &Account,
    session_id: &str,
    server_id: &str,
) -> Result<Option<Value>, i32> {
    // server_id may exceed 2^53; compare as text to stay exact.
    let safe = escape_sql(server_id);
    find_message(acc, session_id, &format!("CAST(server_id AS TEXT) = '{safe}'"))
}

pub fn get_message_by_local_id(
    acc: &Account,
    session_id: &str,
    local_id: i32,
) -> Result<Option<Value>, i32> {
    find_message(acc, session_id, &format!("local_id = {}", local_id))
}

pub fn get_message_meta(
    acc: &Account,
    db_path: &str,
    table: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Value>, i32> {
    let path = if db_path.trim().is_empty() {
        acc.message_dbs().into_iter().next().ok_or(ERR_NOT_FOUND)?
    } else {
        std::path::PathBuf::from(db_path)
    };
    let conn = acc.db(&path)?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, table) {
        return Ok(Vec::new());
    }
    let name2id = load_name2id(&guard).unwrap_or_default();
    let columns = {
        let names = table_columns(&guard, table);
        if names.iter().any(|name| name == "packed_info_data") {
            format!("{MESSAGE_BASE_COLUMNS}, packed_info_data")
        } else {
            MESSAGE_BASE_COLUMNS.to_string()
        }
    };
    let sql = format!(
        "SELECT {columns} FROM \"{table}\" ORDER BY create_time DESC, sort_seq DESC, local_id DESC LIMIT {} OFFSET {}",
        limit.max(0),
        offset.max(0)
    );
    let mut rows = rows_or_empty(query_json(&guard, &sql, &[]))?;
    let my_id = acc.my_account_id();
    for row in rows.iter_mut() {
        if let Value::Object(map) = row {
            let sender = name2id
                .get(&int_field(map, "real_sender_id"))
                .cloned();
            let is_send = if let Some(sender) = sender.as_deref() {
                if !my_id.is_empty() && sender == my_id {
                    1
                } else {
                    0
                }
            } else {
                0
            };
            map.insert(
                "sender_username".to_string(),
                sender.map(Value::String).unwrap_or(Value::Null),
            );
            map.insert("is_send".to_string(), Value::from(is_send));
            map.insert(
                "_db_path".to_string(),
                Value::String(path.to_string_lossy().into_owned()),
            );
            map.insert(
                "_db_name".to_string(),
                Value::String(
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string(),
                ),
            );
            map.insert("_table_name".to_string(), Value::String(table.to_string()));
        }
    }
    Ok(rows)
}

pub fn get_messages_by_type(
    acc: &Account,
    session_id: &str,
    local_type: i64,
    ascending: bool,
    limit: i32,
    offset: i32,
) -> Result<Vec<Value>, i32> {
    let table = message_table_name(session_id);
    let direction = if ascending { "ASC" } else { "DESC" };
    let limit = if limit <= 0 { usize::MAX } else { limit as usize };
    let offset = offset.max(0) as usize;
    let mut merged: Vec<Value> = Vec::new();
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let columns = shard.message_columns(&table);
        let sql = format!(
            "SELECT {columns} FROM \"{table}\" WHERE local_type = {local_type} \
             ORDER BY create_time {direction}, sort_seq {direction}, local_id {direction} LIMIT {}",
            fetch_cap(limit, offset)
        );
        for mut row in shard.query(&sql) {
            if let Value::Object(map) = &mut row {
                decorate_message(acc, &shard, &table, map);
            }
            merged.push(row);
        }
    }
    merged.sort_by(|a, b| match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let ordering = order_key(x).cmp(&order_key(y));
            if ascending {
                ordering
            } else {
                ordering.reverse()
            }
        }
        _ => std::cmp::Ordering::Equal,
    });
    Ok(merged
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect())
}

pub fn search_messages(
    acc: &Account,
    session_id: &str,
    keyword: &str,
    limit: i32,
    offset: i32,
    begin: i64,
    end: i64,
) -> Result<Vec<Value>, i32> {
    let safe = escape_sql(keyword);
    let mut conditions = vec![format!(
        "(message_content LIKE '%{safe}%' ESCAPE '\\' OR compress_content LIKE '%{safe}%' ESCAPE '\\')"
    )];
    if begin > 0 {
        conditions.push(format!("create_time >= {begin}"));
    }
    if end > 0 {
        conditions.push(format!("create_time <= {end}"));
    }
    let where_clause = conditions.join(" AND ");
    let limit = if limit <= 0 { usize::MAX } else { limit as usize };
    let offset = offset.max(0) as usize;
    let tables: Vec<(String, bool)> = if session_id.trim().is_empty() {
        let mut all = Vec::new();
        for shard in open_shards(acc, "") {
            let names = {
                let guard = shard.conn.lock().unwrap_or_else(|e| e.into_inner());
                table_names(&guard)
            };
            for name in names {
                if name.starts_with("Msg_") {
                    all.push((name, true));
                }
            }
        }
        all
    } else {
        vec![(message_table_name(session_id), true)]
    };
    let mut merged: Vec<Value> = Vec::new();
    for (table, _) in tables {
        for shard in open_shards(acc, &table) {
            if !shard.has_table {
                continue;
            }
            let columns = shard.message_columns(&table);
            let sql = format!(
                "SELECT {columns} FROM \"{table}\" WHERE {where_clause} \
                 ORDER BY create_time DESC, sort_seq DESC, local_id DESC LIMIT {}",
                fetch_cap(limit, offset)
            );
            for mut row in shard.query(&sql) {
                if let Value::Object(map) = &mut row {
                    decorate_message(acc, &shard, &table, map);
                }
                merged.push(row);
            }
        }
    }
    merged.sort_by(|a, b| match (a, b) {
        (Value::Object(x), Value::Object(y)) => order_key(y).cmp(&order_key(x)),
        _ => std::cmp::Ordering::Equal,
    });
    Ok(merged
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect())
}

pub fn get_message_dates(acc: &Account, session_id: &str) -> Result<Vec<Value>, i32> {
    let table = message_table_name(session_id);
    let mut dates = BTreeSet::new();
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let sql = format!(
            "SELECT DISTINCT strftime('%Y-%m-%d', create_time, 'unixepoch', 'localtime') AS d FROM \"{table}\""
        );
        for row in shard.query(&sql) {
            if let Value::Object(map) = row {
                let day = value_str(&map, &["d"]);
                if !day.is_empty() {
                    dates.insert(day);
                }
            }
        }
    }
    Ok(dates.into_iter().map(Value::String).collect())
}

pub fn get_message_tables(acc: &Account, session_id: &str) -> Vec<Value> {
    let table = message_table_name(session_id);
    let mut out = Vec::new();
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        out.push(json!({
            "db_path": shard.path.to_string_lossy(),
            "db_name": shard.path.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
            "table_name": table,
            "message_count": shard.scalar(&format!("SELECT count(*) AS c FROM \"{table}\""), "c"),
        }));
    }
    out
}

pub fn get_message_table_stats(acc: &Account, session_id: &str) -> Vec<Value> {
    let table = message_table_name(session_id);
    let mut out = Vec::new();
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let sql = format!(
            "SELECT count(*) AS count, min(create_time) AS min_time, max(create_time) AS max_time, \
             min(sort_seq) AS min_sort_seq, max(sort_seq) AS max_sort_seq FROM \"{table}\""
        );
        if let Some(Value::Object(map)) = shard.query(&sql).first() {
            out.push(json!({
                "db_path": shard.path.to_string_lossy(),
                "db_name": shard.path.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
                "table_name": table,
                "count": int_field(map, "count"),
                "first_timestamp": int_field(map, "min_time"),
                "last_timestamp": int_field(map, "max_time"),
                "min_time": int_field(map, "min_time"),
                "max_time": int_field(map, "max_time"),
                "min_sort_seq": int_field(map, "min_sort_seq"),
                "max_sort_seq": int_field(map, "max_sort_seq"),
            }));
        }
    }
    out
}

pub fn get_message_table_time_range(
    acc: &Account,
    db_path: &str,
    table: &str,
) -> Result<Value, i32> {
    let path = if db_path.trim().is_empty() {
        acc.message_dbs().into_iter().next().ok_or(ERR_NOT_FOUND)?
    } else {
        std::path::PathBuf::from(db_path)
    };
    let conn = acc.db(&path)?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, table) {
        return Err(ERR_SCHEMA);
    }
    let sql = format!("SELECT min(create_time) AS min_time, max(create_time) AS max_time FROM \"{table}\"");
    let rows = rows_or_empty(query_json(&guard, &sql, &[]))?;
    let (min_time, max_time) = match rows.first() {
        Some(Value::Object(map)) => (int_field(map, "min_time"), int_field(map, "max_time")),
        _ => (0, 0),
    };
    Ok(json!({
        "column": "create_time",
        "first_ts": min_time,
        "last_ts": max_time,
        "db_path": path.to_string_lossy(),
        "table_name": table,
    }))
}

pub fn get_message_table_columns(
    acc: &Account,
    db_path: &str,
    table: &str,
) -> Result<Vec<Value>, i32> {
    let path = if db_path.trim().is_empty() {
        acc.message_dbs().into_iter().next().ok_or(ERR_NOT_FOUND)?
    } else {
        std::path::PathBuf::from(db_path)
    };
    let conn = acc.db(&path)?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, table) {
        return Err(ERR_SCHEMA);
    }
    Ok(table_columns(&guard, table)
        .into_iter()
        .map(Value::String)
        .collect())
}

pub fn list_message_dbs(acc: &Account) -> Vec<Value> {
    acc.message_dbs()
        .into_iter()
        .map(|path| Value::String(path.to_string_lossy().into_owned()))
        .collect()
}

pub fn list_media_dbs(acc: &Account) -> Vec<Value> {
    acc.media_dbs()
        .into_iter()
        .map(|path| Value::String(path.to_string_lossy().into_owned()))
        .collect()
}

/* ------------------------------- contacts ------------------------------- */

pub fn contacts_compact(acc: &Account, usernames: &[String]) -> Result<Vec<Value>, i32> {
    let conn = acc.db(&acc.contact_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sql = if usernames.is_empty() {
        "SELECT * FROM contact".to_string()
    } else {
        let list = usernames
            .iter()
            .map(|name| sql_string(name))
            .collect::<Vec<_>>()
            .join(",");
        format!("SELECT * FROM contact WHERE username IN ({list})")
    };
    rows_or_empty(query_json(&guard, &sql, &[]))
}

pub fn get_contact(acc: &Account, username: &str) -> Result<Option<Value>, i32> {
    let conn = acc.db(&acc.contact_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sql = format!("SELECT * FROM contact WHERE username={} LIMIT 1", sql_string(username));
    let rows = rows_or_empty(query_json(&guard, &sql, &[]))?;
    Ok(rows.into_iter().next())
}

pub fn contact_type_counts(acc: &Account) -> Value {
    let rows = contacts_compact(acc, &[]).unwrap_or_default();
    let mut private = 0;
    let mut group = 0;
    let mut official = 0;
    let mut former_friend = 0;
    let mut blocked = 0;
    for row in rows {
        let Value::Object(map) = row else { continue };
        let username = value_str(&map, &["username"]);
        if username.is_empty() {
            continue;
        }
        let local_type = int_field(&map, "local_type");
        let flag = int_field(&map, "flag");
        let quan_pin = value_str(&map, &["quan_pin"]);
        let alias = value_str(&map, &["alias"]);
        let remark = value_str(&map, &["remark"]);
        if username.ends_with("@chatroom") {
            group += 1;
        } else if username.starts_with("gh_") && local_type == 1 {
            official += 1;
        } else if !username.starts_with("gh_") && (flag & 8) == 8 {
            blocked += 1;
        } else if !username.starts_with("gh_")
            && ((local_type == 0 && !quan_pin.is_empty())
                || (local_type == 3 && flag != 4 && (!quan_pin.is_empty() || !alias.is_empty() || !remark.is_empty())))
        {
            former_friend += 1;
        } else if local_type == 1 {
            private += 1;
        }
    }
    json!({
        "private": private,
        "group": group,
        "official": official,
        "former_friend": former_friend,
        "blocked": blocked,
    })
}

pub fn contact_status(acc: &Account, usernames: &[String]) -> Result<Value, i32> {
    // isFolded: contact.flag bit 0x40000 (folded into "folded chat" group);
    // isMuted: SessionTable.status bit 0x2 (notification muted) when available.
    let contacts = contacts_compact(acc, usernames)?;
    let mut map = Map::new();
    let mute_map = session_status_map(acc);
    for row in contacts {
        let Value::Object(obj) = row else { continue };
        let username = value_str(&obj, &["username"]);
        if username.is_empty() {
            continue;
        }
        let flag = int_field(&obj, "flag");
        let status = mute_map.get(&username).copied().unwrap_or(0);
        map.insert(
            username,
            json!({
                "isFolded": (flag & 0x40000) != 0,
                "isMuted": (status & 0x2) != 0,
            }),
        );
    }
    Ok(Value::Object(map))
}

fn session_status_map(acc: &Account) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    let Some(conn) = acc.db_opt(&acc.session_db()) else {
        return map;
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(rows) = query_json(&guard, "SELECT username, status FROM SessionTable", &[]) {
        for row in rows {
            if let Value::Object(obj) = row {
                map.insert(value_str(&obj, &["username"]), int_field(&obj, "status"));
            }
        }
    }
    map
}

pub fn contact_alias_map(acc: &Account, usernames: &[String]) -> Result<Value, i32> {
    let rows = contacts_compact(acc, usernames)?;
    let mut map = Map::new();
    for row in rows {
        let Value::Object(obj) = row else { continue };
        let username = value_str(&obj, &["username"]);
        if username.is_empty() {
            continue;
        }
        let alias = value_str(&obj, &["alias"]);
        map.insert(username, Value::String(alias));
    }
    Ok(Value::Object(map))
}

pub fn contact_friend_flags(acc: &Account, usernames: &[String]) -> Result<Value, i32> {
    let rows = contacts_compact(acc, usernames)?;
    let mut map = Map::new();
    for row in rows {
        let Value::Object(obj) = row else { continue };
        let username = value_str(&obj, &["username"]);
        if username.is_empty() {
            continue;
        }
        let local_type = int_field(&obj, "local_type");
        let flag = int_field(&obj, "flag");
        let is_friend = !username.starts_with("gh_") && local_type == 1 && (flag & 8) != 8;
        map.insert(username, Value::Bool(is_friend));
    }
    Ok(Value::Object(map))
}

pub fn display_names(acc: &Account, usernames: &[String]) -> Result<Value, i32> {
    let rows = contacts_compact(acc, usernames)?;
    let mut map = Map::new();
    for row in rows {
        let Value::Object(obj) = row else { continue };
        let username = value_str(&obj, &["username"]);
        if username.is_empty() {
            continue;
        }
        let remark = value_str(&obj, &["remark"]);
        let nick_name = value_str(&obj, &["nick_name"]);
        let alias = value_str(&obj, &["alias"]);
        let display = if !remark.is_empty() {
            remark
        } else if !nick_name.is_empty() {
            nick_name
        } else if !alias.is_empty() {
            alias
        } else {
            username.clone()
        };
        map.insert(username, Value::String(display));
    }
    Ok(Value::Object(map))
}

pub fn avatar_urls(acc: &Account, usernames: &[String]) -> Result<Value, i32> {
    let rows = contacts_compact(acc, usernames)?;
    let mut map = Map::new();
    for row in rows {
        let Value::Object(obj) = row else { continue };
        let username = value_str(&obj, &["username"]);
        if username.is_empty() {
            continue;
        }
        let big = value_str(&obj, &["big_head_url"]);
        let small = value_str(&obj, &["small_head_url"]);
        let url = if !big.is_empty() { big } else { small };
        map.insert(username, Value::String(url));
    }
    Ok(Value::Object(map))
}

/* -------------------------------- groups -------------------------------- */

fn read_varint(buf: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut value = 0usize;
    let mut shift = 0u32;
    let mut index = start;
    while index < buf.len() && shift < 64 {
        let byte = buf[index];
        value |= ((byte & 0x7f) as usize) << shift;
        index += 1;
        if byte & 0x80 == 0 {
            return Some((value, index));
        }
        shift += 7;
    }
    None
}

/// `chat_room.ext_buffer` stores repeated `{ id, nickname }` protobuf pairs
/// (field 1 = wxid, field 2 = group nickname).
fn parse_group_nicknames(buf: &[u8]) -> Map<String, Value> {
    let mut map = Map::new();
    let mut index = 0usize;
    while index + 2 < buf.len() {
        if buf[index] != 0x0a {
            index += 1;
            continue;
        }
        let Some((id_len, id_start)) = read_varint(buf, index + 1) else {
            index += 1;
            continue;
        };
        if id_len == 0 || id_len > 96 || id_start + id_len > buf.len() {
            index += 1;
            continue;
        }
        let id = String::from_utf8_lossy(&buf[id_start..id_start + id_len])
            .trim()
            .to_string();
        let cursor = id_start + id_len;
        if cursor >= buf.len() || buf[cursor] != 0x12 {
            index = cursor;
            continue;
        }
        let Some((nick_len, nick_start)) = read_varint(buf, cursor + 1) else {
            index = cursor;
            continue;
        };
        if nick_len == 0 || nick_len > 512 || nick_start + nick_len > buf.len() {
            index = cursor;
            continue;
        }
        let nick = String::from_utf8_lossy(&buf[nick_start..nick_start + nick_len])
            .trim()
            .to_string();
        if !id.is_empty() && !nick.is_empty() {
            map.insert(id, Value::String(nick));
        }
        index = nick_start + nick_len;
    }
    map
}

fn group_nickname_map(acc: &Account, chatroom: &str) -> Map<String, Value> {
    let Some(conn) = acc.db_opt(&acc.contact_db()) else {
        return Map::new();
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sql = format!(
        "SELECT ext_buffer FROM chat_room WHERE username = {} LIMIT 1",
        sql_string(chatroom)
    );
    let hex_text = query_json(&guard, &sql, &[])
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|row| match row {
            Value::Object(map) => Some(value_str(&map, &["ext_buffer"])),
            _ => None,
        })
        .unwrap_or_default();
    if hex_text.is_empty() {
        return Map::new();
    }
    match hex::decode(&hex_text) {
        Ok(bytes) => parse_group_nicknames(&bytes),
        Err(_) => Map::new(),
    }
}

pub fn group_nicknames(acc: &Account, chatroom: &str) -> Value {
    Value::Object(group_nickname_map(acc, chatroom))
}

fn group_member_usernames(acc: &Account, chatroom: &str) -> Result<Vec<String>, i32> {
    let conn = acc.db(&acc.contact_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let canonical = format!(
        "SELECT n.username AS username FROM chatroom_member cm \
         JOIN chat_room cr ON cr.id = cm.room_id \
         JOIN name2id n ON n.rowid = cm.member_id \
         WHERE cr.username = {} ORDER BY cm.member_id",
        sql_string(chatroom)
    );
    let mut names = usernames_from_rows(rows_or_empty(query_json(&guard, &canonical, &[]))?);
    if names.is_empty() {
        let fallback = format!(
            "SELECT ct.username AS username FROM chatroom_member cm \
             JOIN chat_room cr ON cr.id = cm.room_id \
             JOIN contact ct ON ct.id = cm.member_id \
             WHERE cr.username = {} ORDER BY ct.id",
            sql_string(chatroom)
        );
        names = usernames_from_rows(rows_or_empty(query_json(&guard, &fallback, &[]))?);
    }
    let mut seen = BTreeSet::new();
    names.retain(|name| !name.is_empty() && seen.insert(name.clone()));
    Ok(names)
}

fn usernames_from_rows(rows: Vec<Value>) -> Vec<String> {
    rows.into_iter()
        .filter_map(|row| match row {
            Value::Object(map) => {
                let name = value_str(&map, &["username", "user_name"]);
                if name.is_empty() {
                    None
                } else {
                    Some(name)
                }
            }
            _ => None,
        })
        .collect()
}

pub fn group_members(acc: &Account, chatroom: &str) -> Result<Vec<Value>, i32> {
    let names = group_member_usernames(acc, chatroom)?;
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let owner = {
        let conn = acc.db(&acc.contact_db())?;
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        query_json(
            &guard,
            &format!(
                "SELECT owner FROM chat_room WHERE username = {} LIMIT 1",
                sql_string(chatroom)
            ),
            &[],
        )
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|row| match row {
            Value::Object(map) => Some(value_str(&map, &["owner"])),
            _ => None,
        })
        .unwrap_or_default()
    };
    let nicknames = group_nickname_map(acc, chatroom);
    let mut contact_info: HashMap<String, (String, String)> = HashMap::new();
    for row in contacts_compact(acc, &names).unwrap_or_default() {
        let Value::Object(map) = row else { continue };
        let username = value_str(&map, &["username"]);
        if username.is_empty() {
            continue;
        }
        let mut display = value_str(&map, &["nick_name", "nickName", "nickname"]);
        if display.is_empty() {
            display = value_str(&map, &["remark"]);
        }
        let avatar = {
            let big = value_str(&map, &["big_head_url", "big_head_img_url", "bigHeadImgUrl"]);
            if !big.is_empty() {
                big
            } else {
                value_str(
                    &map,
                    &["small_head_url", "small_head_img_url", "smallHeadImgUrl"],
                )
            }
        };
        contact_info.insert(username, (display, avatar));
    }
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let mut item = Map::new();
        let nickname = nicknames
            .get(&name)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|text| !text.is_empty());
        let info = contact_info.get(&name);
        let original = nickname
            .or_else(|| info.map(|(display, _)| display.clone()).filter(|text| !text.is_empty()));
        if let Some(original) = original {
            item.insert("originalName".to_string(), Value::String(original));
        }
        if let Some((_, avatar)) = info {
            if !avatar.is_empty() {
                item.insert("avatarUrl".to_string(), Value::String(avatar.clone()));
            }
        }
        item.insert("is_owner".to_string(), Value::Bool(!owner.is_empty() && owner == name));
        item.insert("username".to_string(), Value::String(name));
        out.push(Value::Object(item));
    }
    Ok(out)
}

pub fn group_member_count(acc: &Account, chatroom: &str) -> i64 {
    let Some(conn) = acc.db_opt(&acc.contact_db()) else {
        return 0;
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sql = format!(
        "SELECT count(*) AS c FROM chatroom_member cm \
         JOIN chat_room cr ON cr.id = cm.room_id WHERE cr.username = {}",
        sql_string(chatroom)
    );
    query_json(&guard, &sql, &[])
        .ok()
        .and_then(|rows| match rows.first() {
            Some(Value::Object(map)) => Some(int_field(map, "c")),
            _ => None,
        })
        .unwrap_or(0)
}

pub fn group_member_counts(acc: &Account, chatrooms: &[String]) -> Value {
    let mut map = Map::new();
    for chatroom in chatrooms {
        map.insert(chatroom.clone(), Value::from(group_member_count(acc, chatroom)));
    }
    Value::Object(map)
}

pub fn chat_room_ext_buffer(acc: &Account, chatroom: &str) -> Result<Value, i32> {
    let conn = acc.db(&acc.contact_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sql = format!(
        "SELECT ext_buffer FROM chat_room WHERE username = {} LIMIT 1",
        sql_string(chatroom)
    );
    let rows = rows_or_empty(query_json(&guard, &sql, &[]))?;
    let ext = match rows.first() {
        Some(Value::Object(map)) => value_str(map, &["ext_buffer"]),
        _ => String::new(),
    };
    Ok(json!({ "ext_buffer": ext }))
}

/* ---------------------------- db introspection -------------------------- */

fn resolve_kind_path(acc: &Account, kind: &str, path: &str) -> Result<std::path::PathBuf, i32> {
    let trimmed = path.trim();
    if !trimmed.is_empty() {
        return Ok(std::path::PathBuf::from(trimmed));
    }
    match kind.trim().to_lowercase().as_str() {
        "contact" => Ok(acc.contact_db()),
        "session" => Ok(acc.session_db()),
        "sns" => Ok(acc.sns_db()),
        "hardlink" => Ok(acc.hardlink_db()),
        "emoticon" => Ok(acc.emoticon_db()),
        "message" => acc.message_dbs().into_iter().next().ok_or(ERR_NOT_FOUND),
        "media" => acc.media_dbs().into_iter().next().ok_or(ERR_NOT_FOUND),
        _ => Err(ERR_ARG),
    }
}

pub fn exec_query(
    acc: &Account,
    kind: &str,
    path: &str,
    sql: &str,
) -> Result<Vec<Value>, i32> {
    let kind_lower = kind.trim().to_lowercase();
    // A `message` query without an explicit path scans every shard and merges.
    if kind_lower == "message" && path.trim().is_empty() {
        let mut out = Vec::new();
        for shard_path in acc.message_dbs() {
            if let Ok(conn) = acc.db(&shard_path) {
                let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
                let mut rows = query_json(&guard, sql, &[]).map_err(|_| ERR_SQL)?;
                let db_path = shard_path.to_string_lossy().into_owned();
                let db_name = shard_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                for row in rows.iter_mut() {
                    if let Value::Object(map) = row {
                        map.entry("_db_path".to_string())
                            .or_insert_with(|| Value::String(db_path.clone()));
                        map.entry("_db_name".to_string())
                            .or_insert_with(|| Value::String(db_name.clone()));
                    }
                }
                out.extend(rows);
            }
        }
        return Ok(out);
    }
    let db_path = resolve_kind_path(acc, kind, path)?;
    let conn = acc.db(&db_path)?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    rows_or_empty(query_json(&guard, sql, &[]))
}

pub fn list_tables(acc: &Account, kind: &str, path: &str) -> Result<Vec<Value>, i32> {
    let db_path = resolve_kind_path(acc, kind, path)?;
    let conn = acc.db(&db_path)?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    Ok(table_names(&guard)
        .into_iter()
        .map(Value::String)
        .collect())
}

pub fn get_table_schema(
    acc: &Account,
    kind: &str,
    path: &str,
    table: &str,
) -> Result<Value, i32> {
    let db_path = resolve_kind_path(acc, kind, path)?;
    let conn = acc.db(&db_path)?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sql = format!(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name={} LIMIT 1",
        sql_string(table)
    );
    let rows = rows_or_empty(query_json(&guard, &sql, &[]))?;
    let create_sql = match rows.first() {
        Some(Value::Object(map)) => value_str(map, &["sql"]),
        _ => return Err(ERR_SCHEMA),
    };
    Ok(json!({
        "schema": create_sql,
        "db_path": db_path.to_string_lossy(),
        "table_name": table,
        "columns": table_columns(&guard, table),
    }))
}

/* ------------------------------ message stats --------------------------- */

pub fn session_message_counts(acc: &Account, session_ids: &[String]) -> Value {
    let mut map = Map::new();
    for session_id in session_ids {
        let count = get_message_count(acc, session_id).unwrap_or(0);
        map.insert(session_id.clone(), Value::from(count));
    }
    Value::Object(map)
}

pub(crate) fn decode_blob_text(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.len() > 32
                && trimmed.len() % 2 == 0
                && trimmed.bytes().all(|b| b.is_ascii_hexdigit())
            {
                if let Ok(bytes) = hex::decode(trimmed) {
                    if let Ok(decoded) = String::from_utf8(bytes.clone()) {
                        return decoded;
                    }
                    return String::from_utf8_lossy(&bytes).into_owned();
                }
            }
            text.clone()
        }
        _ => String::new(),
    }
}

pub fn session_message_type_stats(
    acc: &Account,
    session_id: &str,
    begin: i64,
    end: i64,
    include_date_counts: bool,
) -> Value {
    let table = message_table_name(session_id);
    let mut total = 0i64;
    let mut voice = 0i64;
    let mut image = 0i64;
    let mut video = 0i64;
    let mut emoji = 0i64;
    let mut file = 0i64;
    let mut call = 0i64;
    let mut transfer = 0i64;
    let mut red_packet = 0i64;
    let mut mine = 0i64;
    let mut senders: BTreeSet<i64> = BTreeSet::new();
    let mut min_time = 0i64;
    let mut max_time = 0i64;
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let mut conditions = Vec::new();
        if begin > 0 {
            conditions.push(format!("create_time >= {begin}"));
        }
        if end > 0 {
            conditions.push(format!("create_time <= {end}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT local_type AS t, count(*) AS c, min(create_time) AS mn, max(create_time) AS mx \
             FROM \"{table}\" {where_clause} GROUP BY local_type"
        );
        for row in shard.query(&sql) {
            let Value::Object(map) = row else { continue };
            let local_type = int_field(&map, "t");
            let count = int_field(&map, "c");
            total += count;
            match local_type {
                3 => image += count,
                34 => voice += count,
                43 => video += count,
                47 => emoji += count,
                50 => call += count,
                8589934592049 => transfer += count,
                8594229559345 => red_packet += count,
                49 | 34359738417 | 103079215153 | 25769803825 => file += count,
                _ => {}
            }
            let mn = int_field(&map, "mn");
            let mx = int_field(&map, "mx");
            if min_time == 0 || (mn > 0 && mn < min_time) {
                min_time = mn;
            }
            if mx > max_time {
                max_time = mx;
            }
        }
        let mut sender_conditions = conditions.clone();
        sender_conditions.push(format!(
            "real_sender_id IN (SELECT rowid FROM Name2Id WHERE user_name = {})",
            sql_string(&acc.my_account_id())
        ));
        let mine_sql = format!(
            "SELECT count(*) AS c FROM \"{table}\" WHERE {}",
            sender_conditions.join(" AND ")
        );
        mine += shard.scalar(&mine_sql, "c");
        let ids = format!(
            "SELECT DISTINCT real_sender_id AS id FROM \"{table}\" {where_clause}"
        );
        for row in shard.query(&ids) {
            if let Value::Object(map) = row {
                senders.insert(int_field(&map, "id"));
            }
        }
    }
    // Type 49 is shared by links, files, transfers and red packets; inspect the
    // payload so exports do not fall back to a full cursor scan.
    let mut type49_file = 0i64;
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let mut conditions = vec!["local_type = 49".to_string()];
        if begin > 0 {
            conditions.push(format!("create_time >= {begin}"));
        }
        if end > 0 {
            conditions.push(format!("create_time <= {end}"));
        }
        let sql = format!(
            "SELECT message_content, compress_content FROM \"{table}\" WHERE {}",
            conditions.join(" AND ")
        );
        for row in shard.query(&sql) {
            let Value::Object(map) = row else { continue };
            let content = decode_blob_text(map.get("compress_content").unwrap_or(&Value::Null));
            let content = if content.trim().is_empty() {
                decode_blob_text(map.get("message_content").unwrap_or(&Value::Null))
            } else {
                content
            };
            if content.contains("<type>2000</type>") {
                transfer += 1;
            } else if content.contains("<type>2001</type>") {
                red_packet += 1;
            } else if content.contains("<type>6</type>") {
                type49_file += 1;
            }
        }
    }
    file += type49_file;
    let mut out = json!({
        "session_id": session_id,
        "total_messages": total,
        "voice_messages": voice,
        "image_messages": image,
        "video_messages": video,
        "emoji_messages": emoji,
        "file_messages": file,
        "call_messages": call,
        "transfer_messages": transfer,
        "red_packet_messages": red_packet,
        "first_timestamp": min_time,
        "last_timestamp": max_time,
        "group_my_messages": mine,
        "group_sender_count": senders.len(),
        "send": mine,
        "receive": (total - mine).max(0),
    });
    if include_date_counts {
        let counts = session_message_date_counts(acc, session_id);
        if let Value::Object(map) = &mut out {
            map.insert("date_counts".to_string(), counts);
        }
    }
    out
}

pub fn session_message_date_counts(acc: &Account, session_id: &str) -> Value {
    let table = message_table_name(session_id);
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let sql = format!(
            "SELECT strftime('%Y-%m-%d', create_time, 'unixepoch', 'localtime') AS d, count(*) AS c \
             FROM \"{table}\" GROUP BY d"
        );
        for row in shard.query(&sql) {
            let Value::Object(map) = row else { continue };
            let day = value_str(&map, &["d"]);
            if !day.is_empty() {
                *counts.entry(day).or_insert(0) += int_field(&map, "c");
            }
        }
    }
    Value::Object(
        counts
            .into_iter()
            .map(|(key, value)| (key, Value::from(value)))
            .collect(),
    )
}
