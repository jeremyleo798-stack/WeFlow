//! Media resolution: hardlink lookup, voice payloads, emoticons, avatars.

use crate::core::*;
use crate::features::{self, MESSAGE_BASE_COLUMNS};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

fn dir_name(conn: &rusqlite::Connection, rowid: i64) -> String {
    let sql = format!("SELECT username FROM dir2id WHERE rowid = {rowid} LIMIT 1");
    query_json(conn, &sql, &[])
        .ok()
        .and_then(|rows| match rows.first() {
            Some(Value::Object(map)) => Some(value_str(map, &["username"])),
            _ => None,
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| rowid.to_string())
}

fn account_root(override_dir: &str, fallback: &Path) -> PathBuf {
    let trimmed = override_dir.trim();
    if !trimmed.is_empty() {
        let candidate = PathBuf::from(trimmed);
        if candidate.is_dir() {
            return candidate;
        }
    }
    fallback.to_path_buf()
}

fn xml_attr(xml: &str, tag: &str, attr: &str) -> String {
    crate::sns::find_elements(xml, tag)
        .into_iter()
        .next()
        .map(|element| {
            crate::sns::parse_attrs(&element)
                .get(attr)
                .cloned()
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

pub fn resolve_image(acc: &Account, md5: &str, account_dir: &str) -> Value {
    let hash = md5.trim();
    if hash.is_empty() {
        return json!({});
    }
    let Some(conn) = acc.db_opt(&acc.hardlink_db()) else {
        return json!({});
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sql = format!(
        "SELECT file_name, dir1, dir2, file_size FROM image_hardlink_info_v4 \
         WHERE md5 = {} COLLATE NOCASE LIMIT 1",
        sql_string(hash)
    );
    let Ok(rows) = query_json(&guard, &sql, &[]) else {
        return json!({});
    };
    let Some(Value::Object(map)) = rows.first() else {
        return json!({});
    };
    let file_name = value_str(map, &["file_name"]);
    if file_name.is_empty() {
        return json!({});
    }
    let d1 = dir_name(&guard, map.get("dir1").and_then(|v| v.as_i64()).unwrap_or(0));
    let d2 = dir_name(&guard, map.get("dir2").and_then(|v| v.as_i64()).unwrap_or(0));
    let root = account_root(account_dir, &acc.account_dir);
    let full_path = root
        .join("msg")
        .join("attach")
        .join(&d1)
        .join(&d2)
        .join("Img")
        .join(format!("{file_name}.dat"));
    json!({
        "file_name": format!("{file_name}.dat"),
        "full_path": full_path.to_string_lossy(),
        "file_size": map.get("file_size").and_then(|v| v.as_i64()).unwrap_or(0),
    })
}

pub fn resolve_video(acc: &Account, md5: &str, _db_path: &str) -> Value {
    let hash = md5.trim();
    if hash.is_empty() {
        return json!({ "resolved_md5": "" });
    }
    let Some(conn) = acc.db_opt(&acc.hardlink_db()) else {
        return json!({ "resolved_md5": "" });
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sql = format!(
        "SELECT md5, file_name, dir1, dir2 FROM video_hardlink_info_v4 \
         WHERE md5 = {} COLLATE NOCASE LIMIT 1",
        sql_string(hash)
    );
    let Ok(rows) = query_json(&guard, &sql, &[]) else {
        return json!({ "resolved_md5": "" });
    };
    match rows.first() {
        Some(Value::Object(map)) => json!({
            "resolved_md5": value_str(map, &["md5"]),
            "file_name": value_str(map, &["file_name"]),
            "dir": dir_name(&guard, map.get("dir1").and_then(|v| v.as_i64()).unwrap_or(0)),
        }),
        _ => json!({ "resolved_md5": "" }),
    }
}

fn request_str(item: &Value, keys: &[&str]) -> String {
    match item {
        Value::Object(map) => value_str(map, keys),
        Value::String(text) => text.trim().to_string(),
        _ => String::new(),
    }
}

pub fn resolve_image_batch(acc: &Account, requests: &[Value]) -> Vec<Value> {
    requests
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let md5 = request_str(item, &["md5"]);
            let account_dir = request_str(item, &["account_dir", "accountDir", "account"]);
            let data = resolve_image(acc, &md5, &account_dir);
            let has_path = data
                .get("full_path")
                .and_then(|v| v.as_str())
                .map(|value| !value.is_empty())
                .unwrap_or(false);
            json!({
                "index": index,
                "md5": md5,
                "success": has_path,
                "data": data,
                "error": if has_path { String::new() } else { "not found".to_string() },
            })
        })
        .collect()
}

pub fn resolve_video_batch(acc: &Account, requests: &[Value]) -> Vec<Value> {
    requests
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let md5 = request_str(item, &["md5"]);
            let db_path = request_str(item, &["db_path", "dbPath"]);
            let data = resolve_video(acc, &md5, &db_path);
            let resolved = data
                .get("resolved_md5")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            json!({
                "index": index,
                "md5": md5,
                "success": !resolved.is_empty(),
                "data": data,
            })
        })
        .collect()
}

fn table_with_prefix(conn: &rusqlite::Connection, prefix: &str) -> Option<String> {
    let rows = query_json(
        conn,
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        &[],
    )
    .ok()?;
    for row in rows {
        if let Value::Object(map) = row {
            let name = value_str(&map, &["name"]);
            if name.eq_ignore_ascii_case(prefix) || name.starts_with(prefix) {
                return Some(name);
            }
        }
    }
    None
}

pub fn voice_data(
    acc: &Account,
    session_id: &str,
    create_time: i64,
    local_id: i64,
    svr_id: i64,
    candidates: &[String],
) -> Option<String> {
    let mut dbs = acc.media_dbs();
    for candidate in candidates {
        let trimmed = candidate.trim();
        if trimmed.to_lowercase().ends_with(".db") && Path::new(trimmed).is_file() {
            let path = PathBuf::from(trimmed);
            if !dbs.iter().any(|existing| existing == &path) {
                dbs.push(path);
            }
        }
    }
    for db_path in dbs {
        let Some(conn) = acc.db_opt(&db_path) else {
            continue;
        };
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        let Some(voice_table) = table_with_prefix(&guard, "VoiceInfo") else {
            continue;
        };
        let Some(name_table) = table_with_prefix(&guard, "Name2Id") else {
            continue;
        };
        let name_sql = format!(
            "SELECT rowid FROM \"{name_table}\" WHERE user_name = {} LIMIT 1",
            sql_string(session_id)
        );
        let chat_name_id = query_json(&guard, &name_sql, &[])
            .ok()
            .and_then(|rows| match rows.first() {
                Some(Value::Object(map)) => map.get("rowid").and_then(|v| v.as_i64()),
                _ => None,
            });
        let Some(chat_name_id) = chat_name_id else {
            continue;
        };
        let quoted = voice_table.replace('"', "\"\"");
        let item_local_id: Option<i64> = if svr_id > 0 {
            let sql = format!(
                "SELECT local_id FROM \"{quoted}\" WHERE chat_name_id = {chat_name_id} \
                 AND svr_id = {svr_id} LIMIT 1"
            );
            query_json(&guard, &sql, &[]).ok().and_then(|rows| match rows.first() {
                Some(Value::Object(map)) => map.get("local_id").and_then(|v| v.as_i64()),
                _ => None,
            })
        } else {
            None
        };
        let item_local_id = match item_local_id {
            Some(value) => Some(value),
            None => {
                let sql = format!(
                    "SELECT local_id FROM \"{quoted}\" WHERE chat_name_id = {chat_name_id} \
                     AND create_time BETWEEN {} AND {} \
                     ORDER BY ABS(create_time - {create_time}) LIMIT 1",
                    create_time - 5,
                    create_time + 5
                );
                query_json(&guard, &sql, &[]).ok().and_then(|rows| match rows.first() {
                    Some(Value::Object(map)) => map.get("local_id").and_then(|v| v.as_i64()),
                    _ => None,
                })
            }
        };
        let resolved_local = match item_local_id {
            Some(value) => value,
            None if local_id > 0 => local_id,
            None => continue,
        };
        let sql = format!(
            "SELECT data_index, voice_data FROM \"{quoted}\" \
             WHERE chat_name_id = {chat_name_id} AND local_id = {resolved_local} \
             ORDER BY CAST(data_index AS INTEGER)"
        );
        let Ok(rows) = query_json(&guard, &sql, &[]) else {
            continue;
        };
        let mut hex_payload = String::new();
        for row in rows {
            if let Value::Object(map) = row {
                if let Some(Value::String(chunk)) = map.get("voice_data") {
                    hex_payload.push_str(chunk.trim());
                }
            }
        }
        if !hex_payload.is_empty() {
            return Some(hex_payload);
        }
    }
    None
}

pub fn voice_data_batch(acc: &Account, requests: &[Value]) -> Vec<Value> {
    requests
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let Value::Object(map) = item else {
                return json!({ "index": index, "hex": "" });
            };
            let session_id = value_str(map, &["session_id", "sessionId", "username"]);
            let create_time = map
                .get("create_time")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let local_id = map.get("local_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let svr_id = map
                .get("svr_id")
                .or_else(|| map.get("svrId"))
                .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0);
            let candidates: Vec<String> = map
                .get("candidates")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let hex = voice_data(acc, &session_id, create_time, local_id, svr_id, &candidates)
                .unwrap_or_default();
            json!({ "index": index, "hex": hex })
        })
        .collect()
}

fn emoticon_db(acc: &Account, db_path: &str) -> Option<std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>> {
    let trimmed = db_path.trim();
    if !trimmed.is_empty() {
        if let Ok(conn) = acc.db(Path::new(trimmed)) {
            return Some(conn);
        }
    }
    acc.db_opt(&acc.emoticon_db())
}

fn emotion_url(guard: &rusqlite::Connection, md5: &str) -> String {
    if !table_exists(guard, "kNonStoreEmoticonTable") {
        return String::new();
    }
    let sql = format!(
        "SELECT cdn_url, thumb_url, tp_url, encrypt_url FROM kNonStoreEmoticonTable \
         WHERE md5 = {} COLLATE NOCASE LIMIT 1",
        sql_string(md5)
    );
    let Ok(rows) = query_json(guard, &sql, &[]) else {
        return String::new();
    };
    match rows.first() {
        Some(Value::Object(map)) => {
            let mut url = value_str(map, &["cdn_url"]);
            if url.is_empty() {
                url = value_str(map, &["thumb_url"]);
            }
            if url.is_empty() {
                url = value_str(map, &["tp_url"]);
            }
            if url.is_empty() {
                url = value_str(map, &["encrypt_url"]);
            }
            url
        }
        _ => String::new(),
    }
}

fn emotion_caption(guard: &rusqlite::Connection, md5: &str) -> String {
    if table_exists(guard, "kNonStoreEmoticonTable") {
        let sql = format!(
            "SELECT caption FROM kNonStoreEmoticonTable WHERE md5 = {} COLLATE NOCASE LIMIT 1",
            sql_string(md5)
        );
        if let Ok(rows) = query_json(guard, &sql, &[]) {
            if let Some(Value::Object(map)) = rows.first() {
                let caption = value_str(map, &["caption"]);
                if !caption.is_empty() {
                    return caption;
                }
            }
        }
    }
    if table_exists(guard, "kStoreEmoticonCaptionsTable") {
        let sql = format!(
            "SELECT caption_ FROM kStoreEmoticonCaptionsTable \
             WHERE md5_ = {} COLLATE NOCASE AND caption_ <> '' LIMIT 1",
            sql_string(md5)
        );
        if let Ok(rows) = query_json(guard, &sql, &[]) {
            if let Some(Value::Object(map)) = rows.first() {
                return value_str(map, &["caption_"]);
            }
        }
    }
    String::new()
}

pub fn emoticon_cdn_url(acc: &Account, db_path: &str, md5: &str) -> String {
    let Some(conn) = emoticon_db(acc, db_path) else {
        return String::new();
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    emotion_url(&guard, md5.trim())
}

pub fn emoticon_caption(acc: &Account, db_path: &str, md5: &str) -> String {
    let Some(conn) = emoticon_db(acc, db_path) else {
        return String::new();
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    emotion_caption(&guard, md5.trim())
}

pub fn head_image_buffers(acc: &Account, usernames: &[String]) -> Value {
    let mut map = Map::new();
    if usernames.is_empty() {
        return Value::Object(map);
    }
    let head_db = acc.db_storage.join("head_image").join("head_image.db");
    let Some(conn) = acc.db_opt(&head_db) else {
        return Value::Object(map);
    };
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, "head_image") {
        return Value::Object(map);
    }
    let list = usernames
        .iter()
        .map(|name| sql_string(name))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT username, image_buffer FROM head_image WHERE username IN ({list})");
    if let Ok(rows) = query_json(&guard, &sql, &[]) {
        for row in rows {
            if let Value::Object(obj) = row {
                let username = value_str(&obj, &["username"]);
                if let Some(Value::String(hex_data)) = obj.get("image_buffer") {
                    if !username.is_empty() && !hex_data.is_empty() {
                        map.insert(username, Value::String(hex_data.clone()));
                    }
                }
            }
        }
    }
    Value::Object(map)
}

pub fn scan_media_stream(
    acc: &Account,
    session_ids: &[String],
    media_type: i32,
    begin: i64,
    end: i64,
    limit: i32,
    offset: i32,
) -> (Vec<Value>, bool) {
    let limit = if limit <= 0 { 500 } else { limit as usize };
    let offset = offset.max(0) as usize;
    let types = match media_type {
        1 => "3",
        2 => "43",
        _ => "3,43",
    };
    let mut merged: Vec<(i64, i64, Value)> = Vec::new();
    for session_id in session_ids {
        let table = message_table_name(session_id);
        for shard in features::open_shards(acc, &table) {
            if !shard.has_table {
                continue;
            }
            let columns = shard.columns_of(&table);
            let has_packed = columns.iter().any(|c| c == "packed_info_data");
            let packed = if has_packed { "packed_info_data" } else { "NULL AS packed_info_data" };
            let mut conditions = vec![format!("local_type IN ({types})")];
            if begin > 0 {
                conditions.push(format!("create_time >= {begin}"));
            }
            if end > 0 {
                conditions.push(format!("create_time <= {end}"));
            }
            let sql = format!(
                "SELECT {MESSAGE_BASE_COLUMNS}, {packed} FROM \"{table}\" WHERE {} \
                 ORDER BY create_time DESC, local_id DESC LIMIT {}",
                conditions.join(" AND "),
                (limit + offset + 1) as i64
            );
            for mut row in shard.query(&sql) {
                let mut create_time = 0i64;
                let mut local_id = 0i64;
                if let Value::Object(map) = &mut row {
                    create_time = map
                        .get("create_time")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    local_id = map.get("local_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let content = map
                        .get("compress_content")
                        .map(crate::sns::decode_content)
                        .filter(|text| !text.trim().is_empty())
                        .or_else(|| map.get("message_content").map(crate::sns::decode_content))
                        .unwrap_or_default();
                    let local_type = map
                        .get("local_type")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    if local_type == 3 {
                        let md5 = xml_attr(&content, "img", "md5");
                        if !md5.is_empty() {
                            map.insert("image_md5".to_string(), Value::String(md5));
                        }
                    } else if local_type == 43 {
                        let mut md5 = xml_attr(&content, "videomsg", "md5");
                        if md5.is_empty() {
                            md5 = xml_attr(&content, "videomsg", "newmd5");
                        }
                        if !md5.is_empty() {
                            map.insert("video_md5".to_string(), Value::String(md5));
                        }
                    }
                    map.insert("session_id".to_string(), Value::String(session_id.clone()));
                    if let Some(Value::Number(number)) = map.get("server_id") {
                        let text = Value::String(number.to_string());
                        map.insert("server_id".to_string(), text);
                    }
                    map.insert(
                        "db_path".to_string(),
                        Value::String(shard.path.to_string_lossy().into_owned()),
                    );
                    map.insert(
                        "_db_path".to_string(),
                        Value::String(shard.path.to_string_lossy().into_owned()),
                    );
                    map.insert("table_name".to_string(), Value::String(table.clone()));
                    map.insert("_table_name".to_string(), Value::String(table.clone()));
                    let real_sender = map
                        .get("real_sender_id")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let sender = shard.sender_name(real_sender);
                    let my_id = acc.my_account_id();
                    map.insert(
                        "sender_username".to_string(),
                        sender
                            .clone()
                            .map(Value::String)
                            .unwrap_or(Value::Null),
                    );
                    map.insert(
                        "is_send".to_string(),
                        Value::from(if !my_id.is_empty() && sender.as_deref() == Some(my_id.as_str()) {
                            1
                        } else {
                            0
                        }),
                    );
                }
                merged.push((create_time, local_id, row));
            }
        }
    }
    merged.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let has_more = merged.len() > offset + limit;
    let rows = merged
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(_, _, row)| row)
        .collect();
    (rows, has_more)
}

pub fn media_schema_summary(acc: &Account, db_path: &str) -> Result<Value, i32> {
    let path = if db_path.trim().is_empty() {
        acc.media_dbs().into_iter().next().ok_or(ERR_NOT_FOUND)?
    } else {
        PathBuf::from(db_path.trim())
    };
    let conn = acc.db(&path)?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let mut tables = Vec::new();
    let mut columns = Map::new();
    if let Ok(rows) = query_json(
        &guard,
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        &[],
    ) {
        for row in rows {
            if let Value::Object(map) = row {
                let name = value_str(&map, &["name"]);
                if name.is_empty() {
                    continue;
                }
                let cols: Vec<Value> = features::table_columns(&guard, &name)
                    .into_iter()
                    .map(Value::String)
                    .collect();
                columns.insert(name.clone(), Value::Array(cols));
                tables.push(Value::String(name));
            }
        }
    }
    Ok(json!({
        "db_path": path.to_string_lossy(),
        "tables": tables,
        "columns": columns,
    }))
}
