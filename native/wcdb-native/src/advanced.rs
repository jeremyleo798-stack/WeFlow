//! Advanced features: message cursors, aggregate/report statistics,
//! footprint scans, table snapshots and diagnostics helpers.

use crate::core::*;
use crate::features::{self, decorate_message, open_shards, table_columns, ShardContext};
use crate::sns;
use rusqlite::types::Value as SqlValue;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const PROJECTION_COLUMNS: &str =
    "local_id, server_id, local_type, create_time, sort_seq, real_sender_id, message_content, compress_content";

/* -------------------------------- cursors -------------------------------- */

struct CursorState {
    table: String,
    keys: Vec<(usize, i64, i64, i64)>,
    shard_paths: Vec<PathBuf>,
    pos: usize,
    batch_size: usize,
    projection: bool,
}

static CURSORS: Mutex<Option<HashMap<i64, CursorState>>> = Mutex::new(None);
static NEXT_CURSOR: Mutex<i64> = Mutex::new(1);

fn cursors() -> std::sync::MutexGuard<'static, Option<HashMap<i64, CursorState>>> {
    CURSORS.lock().unwrap_or_else(|e| e.into_inner())
}

fn time_where(begin: i64, end: i64) -> String {
    let mut conds = Vec::new();
    if begin > 0 {
        conds.push(format!("create_time >= {begin}"));
    }
    if end > 0 {
        conds.push(format!("create_time <= {end}"));
    }
    if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    }
}

pub fn open_message_cursor(
    acc: &Account,
    session_id: &str,
    batch_size: i32,
    ascending: bool,
    begin: i64,
    end: i64,
) -> Result<i64, i32> {
    let table = message_table_name(session_id);
    let shards = open_shards(acc, &table);
    let mut keys: Vec<(usize, i64, i64, i64)> = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut any_table = false;
    let where_clause = time_where(begin, end);
    for (index, shard) in shards.iter().enumerate() {
        paths.push(shard.path.clone());
        if !shard.has_table {
            continue;
        }
        any_table = true;
        let sql = format!(
            "SELECT local_id, sort_seq, create_time FROM \"{table}\" {where_clause}"
        );
        for row in shard.query(&sql) {
            let Value::Object(map) = row else { continue };
            let local_id = map.get("local_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let sort_seq = map.get("sort_seq").and_then(|v| v.as_i64()).unwrap_or(0);
            let create_time = map.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
            keys.push((index, local_id, sort_seq, create_time));
        }
    }
    if !any_table {
        return Err(ERR_NOT_FOUND);
    }
    keys.sort_by(|a, b| {
        let ordering = (a.3, a.2, a.1).cmp(&(b.3, b.2, b.1));
        if ascending {
            ordering
        } else {
            ordering.reverse()
        }
    });
    let cursor = {
        let mut next = NEXT_CURSOR.lock().unwrap_or_else(|e| e.into_inner());
        let id = *next;
        *next += 1;
        id
    };
    cursors().get_or_insert_with(HashMap::new).insert(
        cursor,
        CursorState {
            table,
            keys,
            shard_paths: paths,
            pos: 0,
            batch_size: if batch_size > 0 { batch_size as usize } else { 500 },
            projection: false,
        },
    );
    Ok(cursor)
}

pub fn set_message_cursor_projection(cursor: i64, projection: i32) -> bool {
    let mut guard = cursors();
    let Some(map) = guard.as_mut() else {
        return false;
    };
    match map.get_mut(&cursor) {
        Some(state) => {
            state.projection = projection != 0;
            true
        }
        None => false,
    }
}

pub fn fetch_message_batch(acc: &Account, cursor: i64) -> Result<(Vec<Value>, bool), i32> {
    let mut guard = cursors();
    let Some(map) = guard.as_mut() else {
        return Err(ERR_ARG);
    };
    let Some(state) = map.get_mut(&cursor) else {
        return Err(ERR_ARG);
    };
    let table = state.table.clone();
    let paths = state.shard_paths.clone();
    let projection = state.projection;
    let batch_size = state.batch_size.max(1);
    let total = state.keys.len();
    let mut pos = state.pos;
    let mut rows_out: Vec<Value> = Vec::new();
    while pos < total {
        let end = (pos + batch_size).min(total);
        let slice = &state.keys[pos..end];
        let mut grouped: BTreeMap<usize, Vec<i64>> = BTreeMap::new();
        let mut positions: HashMap<(usize, i64, i64, i64), usize> = HashMap::new();
        for (offset, key) in slice.iter().enumerate() {
            grouped.entry(key.0).or_default().push(key.1);
            positions.insert((key.0, key.1, key.2, key.3), offset);
        }
        let mut positioned: Vec<(usize, Value)> = Vec::new();
        for (shard_index, ids) in grouped {
            let Some(path) = paths.get(shard_index) else {
                continue;
            };
            let Some(conn) = acc.db_opt(path) else {
                continue;
            };
            let name2id = acc.name2id(path);
            let shard = ShardContext {
                path: path.clone(),
                conn,
                name2id,
                has_table: true,
            };
            let id_list = ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let columns = if projection {
                PROJECTION_COLUMNS.to_string()
            } else {
                shard.message_columns(&table)
            };
            let sql = format!(
                "SELECT {columns} FROM \"{table}\" WHERE local_id IN ({id_list})"
            );
            for mut row in shard.query(&sql) {
                if let Value::Object(obj) = &mut row {
                    decorate_message(acc, &shard, &table, obj);
                    let local_id = obj.get("local_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let sort_seq = obj.get("sort_seq").and_then(|v| v.as_i64()).unwrap_or(0);
                    let create_time =
                        obj.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
                    if let Some(offset) =
                        positions.get(&(shard_index, local_id, sort_seq, create_time))
                    {
                        positioned.push((*offset, row));
                    }
                }
            }
        }
        positioned.sort_by_key(|(offset, _)| *offset);
        rows_out.extend(positioned.into_iter().map(|(_, row)| row));
        pos = end;
        if !rows_out.is_empty() {
            break;
        }
    }
    state.pos = pos;
    Ok((rows_out, pos < total))
}

pub fn close_message_cursor(cursor: i64) {
    if let Some(map) = cursors().as_mut() {
        map.remove(&cursor);
    }
}

pub fn clear_cursors() {
    if let Some(map) = cursors().as_mut() {
        map.clear();
    }
}

fn scan_cursor<F: FnMut(&Map<String, Value>)>(
    acc: &Account,
    cursor: i64,
    mut visit: F,
) -> Result<(), i32> {
    loop {
        let (rows, has_more) = fetch_message_batch(acc, cursor)?;
        for row in &rows {
            if let Value::Object(map) = row {
                visit(map);
            }
        }
        if !has_more {
            break;
        }
    }
    Ok(())
}

/* ------------------------------- aggregates ------------------------------ */

pub fn resolve_session_ids(acc: &Account, session_ids: &[String]) -> Vec<String> {
    if !session_ids.is_empty() {
        return session_ids.to_vec();
    }
    features::get_sessions(acc)
        .unwrap_or_default()
        .into_iter()
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

fn sender_ids(shard: &ShardContext, my_id: &str) -> Vec<i64> {
    if my_id.is_empty() {
        return Vec::new();
    }
    shard
        .name2id
        .iter()
        .filter(|(_, name)| name.as_str() == my_id)
        .map(|(id, _)| *id)
        .collect()
}

fn sender_clause(ids: &[i64]) -> String {
    if ids.is_empty() {
        "0".to_string()
    } else {
        format!(
            "real_sender_id IN ({})",
            ids.iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

struct TypeScan {
    total: i64,
    sent: i64,
    first: i64,
    last: i64,
    counts: BTreeMap<i64, i64>,
}

impl TypeScan {
    fn new() -> Self {
        Self {
            total: 0,
            sent: 0,
            first: 0,
            last: 0,
            counts: BTreeMap::new(),
        }
    }
}

struct TimeScan {
    hourly: [i64; 24],
    weekday: [i64; 7],
    daily: BTreeMap<String, i64>,
    sent_daily: BTreeMap<String, i64>,
    monthly: BTreeMap<i64, i64>,
}

impl TimeScan {
    fn new() -> Self {
        Self {
            hourly: [0; 24],
            weekday: [0; 7],
            daily: BTreeMap::new(),
            sent_daily: BTreeMap::new(),
            monthly: BTreeMap::new(),
        }
    }
}

fn scan_types(shard: &ShardContext, table: &str, where_clause: &str, mine: &str) -> TypeScan {
    let mut out = TypeScan::new();
    let sql = format!(
        "SELECT local_type AS t, CASE WHEN {mine} THEN 1 ELSE 0 END AS s, count(*) AS c, \
                min(create_time) AS mn, max(create_time) AS mx \
         FROM \"{table}\" {where_clause} GROUP BY t, s"
    );
    for row in shard.query(&sql) {
        let Value::Object(map) = row else { continue };
        let count = map.get("c").and_then(|v| v.as_i64()).unwrap_or(0);
        let local_type = map.get("t").and_then(|v| v.as_i64()).unwrap_or(0);
        let sent = map.get("s").and_then(|v| v.as_i64()).unwrap_or(0);
        out.total += count;
        if sent == 1 {
            out.sent += count;
        }
        *out.counts.entry(local_type).or_insert(0) += count;
        let mn = map.get("mn").and_then(|v| v.as_i64()).unwrap_or(0);
        let mx = map.get("mx").and_then(|v| v.as_i64()).unwrap_or(0);
        if out.first == 0 || (mn > 0 && mn < out.first) {
            out.first = mn;
        }
        if mx > out.last {
            out.last = mx;
        }
    }
    out
}

fn scan_time(shard: &ShardContext, table: &str, where_clause: &str, mine: &str) -> TimeScan {
    let mut out = TimeScan::new();
    let sql = format!(
        "SELECT CAST(strftime('%H', create_time, 'unixepoch', 'localtime') AS INTEGER) AS h, \
                ((CAST(strftime('%w', create_time, 'unixepoch', 'localtime') AS INTEGER) + 6) % 7) AS w, \
                strftime('%Y-%m-%d', create_time, 'unixepoch', 'localtime') AS d, \
                CAST(strftime('%m', create_time, 'unixepoch', 'localtime') AS INTEGER) AS m, \
                CASE WHEN {mine} THEN 1 ELSE 0 END AS s, count(*) AS c \
         FROM \"{table}\" {where_clause} GROUP BY h, w, d, m, s"
    );
    for row in shard.query(&sql) {
        let Value::Object(map) = row else { continue };
        let count = map.get("c").and_then(|v| v.as_i64()).unwrap_or(0);
        let hour = map.get("h").and_then(|v| v.as_i64()).unwrap_or(0);
        let weekday = map.get("w").and_then(|v| v.as_i64()).unwrap_or(0);
        let month = map.get("m").and_then(|v| v.as_i64()).unwrap_or(0);
        let sent = map.get("s").and_then(|v| v.as_i64()).unwrap_or(0);
        let day = value_str(&map, &["d"]);
        if (0..24).contains(&hour) {
            out.hourly[hour as usize] += count;
        }
        if (0..7).contains(&weekday) {
            out.weekday[weekday as usize] += count;
        }
        if !day.is_empty() {
            *out.daily.entry(day.clone()).or_insert(0) += count;
            if sent == 1 {
                *out.sent_daily.entry(day).or_insert(0) += count;
            }
        }
        if (1..=12).contains(&month) {
            *out.monthly.entry(month).or_insert(0) += count;
        }
    }
    out
}

struct SessionScan {
    types: TypeScan,
    time: TimeScan,
    senders: BTreeMap<i64, i64>,
    names: HashMap<i64, String>,
}

fn scan_session(
    acc: &Account,
    session_id: &str,
    begin: i64,
    end: i64,
    include_senders: bool,
) -> SessionScan {
    let table = message_table_name(session_id);
    let where_clause = time_where(begin, end);
    let my_id = acc.my_account_id();
    let mut out = SessionScan {
        types: TypeScan::new(),
        time: TimeScan::new(),
        senders: BTreeMap::new(),
        names: HashMap::new(),
    };
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let mine = sender_clause(&sender_ids(&shard, &my_id));
        let types = scan_types(&shard, &table, &where_clause, &mine);
        out.types.total += types.total;
        out.types.sent += types.sent;
        if out.types.first == 0 || (types.first > 0 && types.first < out.types.first) {
            out.types.first = types.first;
        }
        if types.last > out.types.last {
            out.types.last = types.last;
        }
        for (key, count) in types.counts {
            *out.types.counts.entry(key).or_insert(0) += count;
        }
        let time = scan_time(&shard, &table, &where_clause, &mine);
        for (index, count) in time.hourly.iter().enumerate() {
            out.time.hourly[index] += *count;
        }
        for (index, count) in time.weekday.iter().enumerate() {
            out.time.weekday[index] += *count;
        }
        for (key, count) in time.daily {
            *out.time.daily.entry(key).or_insert(0) += count;
        }
        for (key, count) in time.sent_daily {
            *out.time.sent_daily.entry(key).or_insert(0) += count;
        }
        for (key, count) in time.monthly {
            *out.time.monthly.entry(key).or_insert(0) += count;
        }
        if include_senders {
            let sql = format!(
                "SELECT real_sender_id AS id, count(*) AS c FROM \"{table}\" {where_clause} GROUP BY id"
            );
            for row in shard.query(&sql) {
                let Value::Object(map) = row else { continue };
                let id = map.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                let count = map.get("c").and_then(|v| v.as_i64()).unwrap_or(0);
                if id == 0 {
                    continue;
                }
                *out.senders.entry(id).or_insert(0) += count;
                if let Some(name) = shard.name2id.get(&id) {
                    out.names.insert(id, name.clone());
                }
            }
        }
    }
    out
}

fn counters_to_value(map: &BTreeMap<String, i64>) -> Value {
    Value::Object(
        map.iter()
            .map(|(key, value)| (key.clone(), Value::from(*value)))
            .collect(),
    )
}

fn monthly_to_value(map: &BTreeMap<i64, i64>) -> Value {
    Value::Object(
        map.iter()
            .map(|(key, value)| (key.to_string(), Value::from(*value)))
            .collect(),
    )
}

fn string_array(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .collect()
}

pub fn aggregate_stats(
    acc: &Account,
    session_ids: &[String],
    begin: i64,
    end: i64,
) -> Result<Value, i32> {
    aggregate_impl(acc, session_ids, begin, end, false)
}

pub fn group_stats(acc: &Account, chatroom: &str, begin: i64, end: i64) -> Value {
    let ids = vec![chatroom.to_string()];
    aggregate_impl(acc, &ids, begin, end, true)
        .unwrap_or_else(|_| json!({ "total": 0, "sessions": {} }))
}

fn aggregate_impl(
    acc: &Account,
    session_ids: &[String],
    begin: i64,
    end: i64,
    include_senders: bool,
) -> Result<Value, i32> {
    let ids = resolve_session_ids(acc, session_ids);
    let mut total = 0i64;
    let mut sent = 0i64;
    let mut first = 0i64;
    let mut last = 0i64;
    let mut type_counts: BTreeMap<i64, i64> = BTreeMap::new();
    for key in [1i64, 3, 34, 43, 47] {
        type_counts.insert(key, 0);
    }
    let mut hourly = [0i64; 24];
    let mut weekday = [0i64; 7];
    let mut daily: BTreeMap<String, i64> = BTreeMap::new();
    let mut sent_daily: BTreeMap<String, i64> = BTreeMap::new();
    let mut monthly: BTreeMap<i64, i64> = BTreeMap::new();
    let mut sessions = Map::new();
    let mut id_map = Map::new();
    for session_id in &ids {
        let scan = scan_session(acc, session_id, begin, end, include_senders);
        total += scan.types.total;
        sent += scan.types.sent;
        if first == 0 || (scan.types.first > 0 && scan.types.first < first) {
            first = scan.types.first;
        }
        if scan.types.last > last {
            last = scan.types.last;
        }
        for (key, count) in scan.types.counts {
            *type_counts.entry(key).or_insert(0) += count;
        }
        for (index, count) in scan.time.hourly.iter().enumerate() {
            hourly[index] += *count;
        }
        for (index, count) in scan.time.weekday.iter().enumerate() {
            weekday[index] += *count;
        }
        for (key, count) in scan.time.daily {
            *daily.entry(key).or_insert(0) += count;
        }
        for (key, count) in scan.time.sent_daily {
            *sent_daily.entry(key).or_insert(0) += count;
        }
        for (key, count) in scan.time.monthly {
            *monthly.entry(key).or_insert(0) += count;
        }
        let mut entry = Map::new();
        entry.insert("total".to_string(), Value::from(scan.types.total));
        entry.insert("sent".to_string(), Value::from(scan.types.sent));
        entry.insert(
            "received".to_string(),
            Value::from((scan.types.total - scan.types.sent).max(0)),
        );
        entry.insert("lastTime".to_string(), Value::from(scan.types.last));
        if include_senders {
            let senders = Value::Object(
                scan.senders
                    .iter()
                    .map(|(id, count)| (id.to_string(), Value::from(*count)))
                    .collect(),
            );
            entry.insert("senders".to_string(), senders);
            for (id, name) in scan.names {
                id_map.insert(id.to_string(), Value::String(name));
            }
        }
        sessions.insert(session_id.clone(), Value::Object(entry));
    }
    let type_counts_value = Value::Object(
        type_counts
            .iter()
            .map(|(key, value)| (key.to_string(), Value::from(*value)))
            .collect(),
    );
    Ok(json!({
        "total": total,
        "sent": sent,
        "received": (total - sent).max(0),
        "firstTime": first,
        "lastTime": last,
        "typeCounts": type_counts_value,
        "hourly": hourly,
        "weekday": weekday,
        "daily": counters_to_value(&daily),
        "sentDaily": counters_to_value(&sent_daily),
        "monthly": monthly_to_value(&monthly),
        "sessions": sessions,
        "idMap": id_map,
    }))
}

pub fn annual_report_stats(
    acc: &Account,
    session_ids: &[String],
    begin: i64,
    end: i64,
) -> Result<Value, i32> {
    let ids = resolve_session_ids(acc, session_ids);
    let mut total = 0i64;
    let mut sessions = Map::new();
    let mut daily: BTreeMap<String, i64> = BTreeMap::new();
    for session_id in &ids {
        let table = message_table_name(session_id);
        let where_clause = time_where(begin, end);
        let my_id = acc.my_account_id();
        let mut sent = 0i64;
        let mut received = 0i64;
        let mut monthly: BTreeMap<i64, i64> = BTreeMap::new();
        for shard in open_shards(acc, &table) {
            if !shard.has_table {
                continue;
            }
            let mine = sender_clause(&sender_ids(&shard, &my_id));
            let sql = format!(
                "SELECT strftime('%Y-%m-%d', create_time, 'unixepoch', 'localtime') AS d, \
                        CAST(strftime('%m', create_time, 'unixepoch', 'localtime') AS INTEGER) AS m, \
                        CASE WHEN {mine} THEN 1 ELSE 0 END AS s, count(*) AS c \
                 FROM \"{table}\" {where_clause} GROUP BY d, m, s"
            );
            for row in shard.query(&sql) {
                let Value::Object(map) = row else { continue };
                let count = map.get("c").and_then(|v| v.as_i64()).unwrap_or(0);
                let month = map.get("m").and_then(|v| v.as_i64()).unwrap_or(0);
                let is_sent = map.get("s").and_then(|v| v.as_i64()).unwrap_or(0);
                let day = value_str(&map, &["d"]);
                if is_sent == 1 {
                    sent += count;
                } else {
                    received += count;
                }
                if (1..=12).contains(&month) {
                    *monthly.entry(month).or_insert(0) += count;
                }
                if !day.is_empty() {
                    *daily.entry(day).or_insert(0) += count;
                }
            }
        }
        total += sent + received;
        sessions.insert(
            session_id.clone(),
            json!({
                "sent": sent,
                "received": received,
                "monthly": monthly_to_value(&monthly),
            }),
        );
    }
    Ok(json!({
        "total": total,
        "sessions": sessions,
        "daily": counters_to_value(&daily),
    }))
}

/* ----------------------------- time helpers ------------------------------ */

fn day_number(day: &str) -> Option<i64> {
    let mut parts = day.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let date: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&date) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + date - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

fn session_days(acc: &Account, session_id: &str, begin: i64, end: i64) -> Vec<String> {
    let table = message_table_name(session_id);
    let where_clause = time_where(begin, end);
    let mut days: BTreeSet<String> = BTreeSet::new();
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let sql = format!(
            "SELECT DISTINCT strftime('%Y-%m-%d', create_time, 'unixepoch', 'localtime') AS d \
             FROM \"{table}\" {where_clause}"
        );
        for row in shard.query(&sql) {
            let Value::Object(map) = row else { continue };
            let day = value_str(&map, &["d"]);
            if !day.is_empty() {
                days.insert(day);
            }
        }
    }
    days.into_iter().collect()
}

fn best_streak(days: &[String]) -> (i64, String, String) {
    let mut best = (0i64, String::new(), String::new());
    let mut run_len = 0i64;
    let mut run_start = String::new();
    let mut previous: Option<i64> = None;
    for day in days {
        let Some(number) = day_number(day) else {
            continue;
        };
        if previous.map(|prev| number - prev == 1).unwrap_or(false) {
            run_len += 1;
        } else {
            run_len = 1;
            run_start = day.clone();
        }
        previous = Some(number);
        if run_len > best.0 {
            best = (run_len, run_start.clone(), day.clone());
        }
    }
    best
}

fn session_heatmap(acc: &Account, session_id: &str, begin: i64, end: i64) -> ([[i64; 24]; 7], [i64; 24]) {
    let table = message_table_name(session_id);
    let where_clause = time_where(begin, end);
    let mut heatmap = [[0i64; 24]; 7];
    let mut hourly = [0i64; 24];
    for shard in open_shards(acc, &table) {
        if !shard.has_table {
            continue;
        }
        let sql = format!(
            "SELECT ((CAST(strftime('%w', create_time, 'unixepoch', 'localtime') AS INTEGER) + 6) % 7) AS w, \
                    CAST(strftime('%H', create_time, 'unixepoch', 'localtime') AS INTEGER) AS h, \
                    count(*) AS c \
             FROM \"{table}\" {where_clause} GROUP BY w, h"
        );
        for row in shard.query(&sql) {
            let Value::Object(map) = row else { continue };
            let weekday = map.get("w").and_then(|v| v.as_i64()).unwrap_or(-1);
            let hour = map.get("h").and_then(|v| v.as_i64()).unwrap_or(-1);
            let count = map.get("c").and_then(|v| v.as_i64()).unwrap_or(0);
            if (0..7).contains(&weekday) && (0..24).contains(&hour) {
                heatmap[weekday as usize][hour as usize] += count;
                hourly[hour as usize] += count;
            }
        }
    }
    (heatmap, hourly)
}

fn merge_heatmap(target: &mut [[i64; 24]; 7], source: &[[i64; 24]; 7]) {
    for weekday in 0..7 {
        for hour in 0..24 {
            target[weekday][hour] += source[weekday][hour];
        }
    }
}

fn heatmap_value(map: &[[i64; 24]; 7]) -> Value {
    Value::Array(
        map.iter()
            .map(|row| Value::Array(row.iter().map(|count| Value::from(*count)).collect()))
            .collect(),
    )
}

/* ------------------------------ annual extras ---------------------------- */

pub fn annual_report_extras(
    acc: &Account,
    session_ids: &[String],
    begin: i64,
    end: i64,
    peak_begin: i64,
    peak_end: i64,
) -> Result<Value, i32> {
    let ids = resolve_session_ids(acc, session_ids);
    let mut heatmap = [[0i64; 24]; 7];
    let mut midnight: BTreeMap<String, i64> = BTreeMap::new();
    let mut conversation: BTreeMap<String, [i64; 2]> = BTreeMap::new();
    let mut response: BTreeMap<String, [i64; 3]> = BTreeMap::new();
    let mut peak_day: BTreeMap<String, i64> = BTreeMap::new();
    let mut phrases: HashMap<String, i64> = HashMap::new();
    let mut best: (i64, String, String, String) = (0, String::new(), String::new(), String::new());
    for session_id in &ids {
        let (session_hm, hourly) = session_heatmap(acc, session_id, begin, end);
        merge_heatmap(&mut heatmap, &session_hm);
        let night: i64 = hourly.iter().take(6).sum();
        if night > 0 {
            midnight.insert(session_id.clone(), night);
        }
        if peak_begin > 0 && peak_end >= peak_begin {
            let table = message_table_name(session_id);
            let mut count = 0i64;
            for shard in open_shards(acc, &table) {
                if !shard.has_table {
                    continue;
                }
                let sql = format!(
                    "SELECT count(*) AS c FROM \"{table}\" \
                     WHERE create_time >= {peak_begin} AND create_time <= {peak_end}"
                );
                count += shard.scalar(&sql, "c");
            }
            if count > 0 {
                peak_day.insert(session_id.clone(), count);
            }
        }
        let days = session_days(acc, session_id, begin, end);
        let (run, start_day, end_day) = best_streak(&days);
        if run > best.0 {
            best = (run, session_id.clone(), start_day, end_day);
        }

        let cursor = match open_message_cursor(acc, session_id, 1000, true, begin, end) {
            Ok(cursor) => cursor,
            Err(_) => continue,
        };
        let mut conv = [0i64; 2];
        let mut resp = [0i64; 3];
        let mut last_time = 0i64;
        let mut last_send = false;
        let mut first = true;
        let _ = scan_cursor(acc, cursor, |row| {
            let create_time = row.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
            if create_time <= 0 {
                return;
            }
            let is_send = row.get("is_send").and_then(|v| v.as_i64()).unwrap_or(0) == 1;
            if first || create_time - last_time > 3600 {
                if is_send {
                    conv[0] += 1;
                } else {
                    conv[1] += 1;
                }
            } else if is_send && !last_send {
                let delta = create_time - last_time;
                if delta > 0 && delta < 86_400 {
                    resp[0] += delta;
                    if resp[1] == 0 || delta < resp[1] {
                        resp[1] = delta;
                    }
                    resp[2] += 1;
                }
            }
            first = false;
            last_time = create_time;
            last_send = is_send;

            let local_type = row.get("local_type").and_then(|v| v.as_i64()).unwrap_or(0);
            if (local_type == 1 || local_type == 244_813_135_921) && is_send {
                let content = message_text(row);
                let text = content.trim();
                let length = text.chars().count();
                if (2..=20).contains(&length)
                    && !text.contains("http")
                    && !text.contains('<')
                    && !text.starts_with('[')
                {
                    *phrases.entry(text.to_string()).or_insert(0) += 1;
                }
            }
        });
        close_message_cursor(cursor);
        if conv[0] + conv[1] > 0 {
            conversation.insert(session_id.clone(), conv);
        }
        if resp[2] > 0 {
            response.insert(session_id.clone(), resp);
        }
    }
    let mut phrase_items: Vec<(String, i64)> = phrases.into_iter().collect();
    phrase_items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    phrase_items.truncate(30);
    let top_phrases = Value::Array(
        phrase_items
            .into_iter()
            .map(|(phrase, count)| json!({ "phrase": phrase, "count": count }))
            .collect(),
    );
    let conversation_value = Value::Object(
        conversation
            .iter()
            .map(|(session, values)| {
                (
                    session.clone(),
                    json!({ "initiated": values[0], "received": values[1] }),
                )
            })
            .collect(),
    );
    let response_value = Value::Object(
        response
            .iter()
            .map(|(session, values)| {
                let avg = if values[2] > 0 {
                    (values[0] as f64 / values[2] as f64 * 10.0).round() / 10.0
                } else {
                    0.0
                };
                (
                    session.clone(),
                    json!({ "avg": avg, "fastest": values[1], "count": values[2] }),
                )
            })
            .collect(),
    );
    let streak = if best.0 > 0 {
        json!({
            "sessionId": best.1,
            "days": best.0,
            "startDate": best.2,
            "endDate": best.3,
        })
    } else {
        json!({})
    };
    Ok(json!({
        "heatmap": heatmap_value(&heatmap),
        "midnight": counters_to_value(&midnight),
        "conversation": conversation_value,
        "response": response_value,
        "peakDay": counters_to_value(&peak_day),
        "topPhrases": top_phrases,
        "streak": streak,
    }))
}

fn message_text(row: &Map<String, Value>) -> String {
    let compress = features::decode_blob_text(row.get("compress_content").unwrap_or(&Value::Null));
    if !compress.trim().is_empty() {
        return compress;
    }
    features::decode_blob_text(row.get("message_content").unwrap_or(&Value::Null))
}

/* ------------------------------- dual report ----------------------------- */

fn emoji_md5(content: &str) -> (String, String) {
    let Some(element) = sns::find_elements(content, "emoji").into_iter().next() else {
        return (String::new(), String::new());
    };
    let attrs = sns::parse_attrs(&element);
    let md5 = attrs.get("md5").cloned().unwrap_or_default();
    let url = attrs
        .get("cdnurl")
        .or_else(|| attrs.get("cdnurl_thumb"))
        .cloned()
        .unwrap_or_default();
    (md5, url)
}

fn phrase_filter(text: &str) -> bool {
    let length = text.chars().count();
    (2..=20).contains(&length)
        && !text.contains("http")
        && !text.contains('<')
        && !text.starts_with('[')
}

pub fn dual_report_stats(
    acc: &Account,
    session_id: &str,
    begin: i64,
    end: i64,
) -> Result<Value, i32> {
    let cursor = match open_message_cursor(acc, session_id, 1000, true, begin, end) {
        Ok(cursor) => Some(cursor),
        Err(ERR_NOT_FOUND) => None,
        Err(code) => return Err(code),
    };
    let mut total = 0i64;
    let mut words = 0i64;
    let mut image = 0i64;
    let mut voice = 0i64;
    let mut emoji = 0i64;
    let mut emojis: HashMap<(String, bool), (i64, String)> = HashMap::new();
    let mut all_phrases: HashMap<String, i64> = HashMap::new();
    let mut my_phrases: HashMap<String, i64> = HashMap::new();
    let mut friend_phrases: HashMap<String, i64> = HashMap::new();
    let mut conv = [0i64; 2];
    let mut resp = [0i64; 3];
    let mut last_time = 0i64;
    let mut last_send = false;
    let mut first = true;
    let _ = cursor.map(|cursor| {
        let _ = scan_cursor(acc, cursor, |row| {
            let create_time = row.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
            let is_send = row.get("is_send").and_then(|v| v.as_i64()).unwrap_or(0) == 1;
            let local_type = row.get("local_type").and_then(|v| v.as_i64()).unwrap_or(0);
            total += 1;
            match local_type {
                3 => image += 1,
                34 => voice += 1,
                47 => {
                    emoji += 1;
                    let content = message_text(row);
                    let (md5, url) = emoji_md5(&content);
                    if !md5.is_empty() {
                        let entry = emojis.entry((md5, is_send)).or_insert((0, String::new()));
                        entry.0 += 1;
                        if entry.1.is_empty() {
                            entry.1 = url;
                        }
                    }
                }
                _ => {}
            }
            if local_type == 1 || local_type == 244_813_135_921 {
                let content = message_text(row);
                let text = content.trim();
                if !text.is_empty() {
                    words += text.chars().count() as i64;
                }
                if phrase_filter(text) {
                    *all_phrases.entry(text.to_string()).or_insert(0) += 1;
                    let target = if is_send {
                        &mut my_phrases
                    } else {
                        &mut friend_phrases
                    };
                    *target.entry(text.to_string()).or_insert(0) += 1;
                }
            }
            if create_time > 0 {
                if first || create_time - last_time > 3600 {
                    if is_send {
                        conv[0] += 1;
                    } else {
                        conv[1] += 1;
                    }
                } else if is_send && !last_send {
                    let delta = create_time - last_time;
                    if delta > 0 && delta < 86_400 {
                        resp[0] += delta;
                        if resp[1] == 0 || delta < resp[1] {
                            resp[1] = delta;
                        }
                        resp[2] += 1;
                    }
                }
                first = false;
                last_time = create_time;
                last_send = is_send;
            }
        });
        close_message_cursor(cursor);
    });

    let mut emoji_items: Vec<((String, bool), (i64, String))> = emojis.into_iter().collect();
    emoji_items.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0 .0.cmp(&b.0 .0)));
    emoji_items.truncate(60);
    let emoji_values = Value::Array(
        emoji_items
            .into_iter()
            .map(|((md5, is_send), (count, url))| {
                json!({ "md5": md5, "url": url, "isMe": is_send, "count": count })
            })
            .collect(),
    );
    let phrase_values = |map: HashMap<String, i64>, take: usize| {
        let mut items: Vec<(String, i64)> = map.into_iter().collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.truncate(take);
        Value::Array(
            items
                .into_iter()
                .map(|(phrase, count)| json!({ "phrase": phrase, "count": count }))
                .collect(),
        )
    };
    let (heatmap, _) = session_heatmap(acc, session_id, begin, end);
    let monthly = {
        let table = message_table_name(session_id);
        let where_clause = time_where(begin, end);
        let mut map: BTreeMap<i64, i64> = BTreeMap::new();
        for shard in open_shards(acc, &table) {
            if !shard.has_table {
                continue;
            }
            let sql = format!(
                "SELECT CAST(strftime('%m', create_time, 'unixepoch', 'localtime') AS INTEGER) AS m, count(*) AS c \
                 FROM \"{table}\" {where_clause} GROUP BY m"
            );
            for row in shard.query(&sql) {
                let Value::Object(obj) = row else { continue };
                let month = obj.get("m").and_then(|v| v.as_i64()).unwrap_or(0);
                let count = obj.get("c").and_then(|v| v.as_i64()).unwrap_or(0);
                if (1..=12).contains(&month) {
                    *map.entry(month).or_insert(0) += count;
                }
            }
        }
        monthly_to_value(&map)
    };
    let days = session_days(acc, session_id, begin, end);
    let (run, start_day, end_day) = best_streak(&days);
    let streak = if run > 0 {
        json!({ "days": run, "startDate": start_day, "endDate": end_day })
    } else {
        json!({})
    };
    let avg = if resp[2] > 0 {
        (resp[0] as f64 / resp[2] as f64 * 10.0).round() / 10.0
    } else {
        0.0
    };
    Ok(json!({
        "counts": {
            "total": total,
            "words": words,
            "image": image,
            "voice": voice,
            "emoji": emoji,
        },
        "emojis": emoji_values,
        "phrases": phrase_values(all_phrases, 20),
        "myPhrases": phrase_values(my_phrases, 20),
        "friendPhrases": phrase_values(friend_phrases, 20),
        "heatmap": heatmap_value(&heatmap),
        "initiative": { "initiated": conv[0], "received": conv[1] },
        "response": { "avg": avg, "fastest": resp[1], "count": resp[2] },
        "monthly": monthly,
        "streak": streak,
    }))
}

/* ------------------------------- footprint ------------------------------- */

fn opt_i64(map: &Map<String, Value>, keys: &[&str], default: i64) -> i64 {
    for key in keys {
        if let Some(value) = map.get(*key) {
            if let Some(number) = value.as_i64() {
                return number;
            }
            if let Some(text) = value.as_str() {
                if let Ok(number) = text.trim().parse::<i64>() {
                    return number;
                }
            }
        }
    }
    default
}

fn opt_strings(map: &Map<String, Value>, keys: &[&str]) -> Vec<String> {
    for key in keys {
        if let Some(Value::Array(items)) = map.get(*key) {
            return string_array(items);
        }
    }
    Vec::new()
}

struct FootprintSession {
    incoming: i64,
    outgoing: i64,
    first_incoming: i64,
    first_reply: i64,
    latest: i64,
    anchor_local_id: i64,
    anchor_create_time: i64,
}

impl FootprintSession {
    fn new() -> Self {
        Self {
            incoming: 0,
            outgoing: 0,
            first_incoming: 0,
            first_reply: 0,
            latest: 0,
            anchor_local_id: 0,
            anchor_create_time: 0,
        }
    }
}

struct FootprintSegment {
    segment_index: i64,
    start_ts: i64,
    end_ts: i64,
    incoming: i64,
    outgoing: i64,
    first_incoming: i64,
    first_reply: i64,
    anchor_local_id: i64,
    anchor_create_time: i64,
}

pub fn my_footprint_stats(acc: &Account, options_json: &str) -> Result<Value, i32> {
    let started = Instant::now();
    let options: Value = serde_json::from_str(options_json).unwrap_or_else(|_| json!({}));
    let map = options.as_object().cloned().unwrap_or_default();
    let begin = opt_i64(&map, &["begin", "beginTimestamp", "begin_timestamp"], 0);
    let end = opt_i64(&map, &["end", "endTimestamp", "end_timestamp"], 0);
    let my_account = {
        let value = opt_strings(&map, &["my_account_id", "myAccountId"]);
        value
            .first()
            .cloned()
            .unwrap_or_else(|| acc.my_account_id())
    };
    let mut private_ids = opt_strings(
        &map,
        &["private_session_ids", "privateSessionIds"],
    );
    let mut group_ids = opt_strings(&map, &["group_session_ids", "groupSessionIds"]);
    private_ids.retain(|id| !id.ends_with("@chatroom"));
    group_ids.retain(|id| id.ends_with("@chatroom"));
    let mention_limit = opt_i64(&map, &["mention_limit", "mentionLimit"], 0).max(0) as usize;
    let mut elapsed_dbs: BTreeSet<PathBuf> = BTreeSet::new();

    let mut private_sessions: Vec<Value> = Vec::new();
    let mut segments: Vec<Value> = Vec::new();
    let private_truncated = false;
    for session_id in &private_ids {
        for shard in open_shards(acc, &message_table_name(session_id)) {
            if shard.has_table {
                elapsed_dbs.insert(shard.path.clone());
            }
        }
        let cursor = match open_message_cursor(acc, session_id, 512, true, begin, end) {
            Ok(cursor) => cursor,
            Err(_) => continue,
        };
        let mut stat = FootprintSession::new();
        let mut session_segments: Vec<FootprintSegment> = Vec::new();
        let mut segment_cursor = 0i64;
        let mut active: Option<FootprintSegment> = None;
        let mut last_segment_ts = 0i64;
        let mut processed = 0i64;
        let _ = scan_cursor(acc, cursor, |row| {
            processed += 1;
            let create_time = row.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
            let local_id = row.get("local_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let is_send = row_is_send(row, &my_account);
            if create_time > 0 {
                let new_segment = active.is_none()
                    || (last_segment_ts > 0 && create_time - last_segment_ts > 600);
                if new_segment {
                    if let Some(segment) = active.take() {
                        commit_segment(&mut session_segments, segment);
                    }
                    segment_cursor += 1;
                    active = Some(FootprintSegment {
                        segment_index: segment_cursor,
                        start_ts: create_time,
                        end_ts: create_time,
                        incoming: 0,
                        outgoing: 0,
                        first_incoming: 0,
                        first_reply: 0,
                        anchor_local_id: local_id,
                        anchor_create_time: create_time,
                    });
                }
            } else if active.is_none() {
                segment_cursor += 1;
                active = Some(FootprintSegment {
                    segment_index: segment_cursor,
                    start_ts: 0,
                    end_ts: 0,
                    incoming: 0,
                    outgoing: 0,
                    first_incoming: 0,
                    first_reply: 0,
                    anchor_local_id: local_id,
                    anchor_create_time: 0,
                });
            }
            if is_send {
                stat.outgoing += 1;
                if create_time > 0
                    && stat.first_incoming > 0
                    && create_time >= stat.first_incoming
                    && stat.first_reply <= 0
                {
                    stat.first_reply = create_time;
                }
                if let Some(segment) = active.as_mut() {
                    segment.outgoing += 1;
                    if create_time > 0
                        && segment.first_incoming > 0
                        && create_time >= segment.first_incoming
                        && segment.first_reply <= 0
                    {
                        segment.first_reply = create_time;
                    }
                }
            } else {
                stat.incoming += 1;
                if stat.first_incoming <= 0 || (create_time > 0 && create_time < stat.first_incoming) {
                    stat.first_incoming = create_time;
                }
                if let Some(segment) = active.as_mut() {
                    segment.incoming += 1;
                    if segment.first_incoming <= 0
                        || (create_time > 0 && create_time < segment.first_incoming)
                    {
                        segment.first_incoming = create_time;
                    }
                }
            }
            if stat.latest <= 0
                || create_time > stat.latest
                || (create_time == stat.latest && local_id > stat.anchor_local_id)
            {
                stat.latest = create_time;
                stat.anchor_local_id = local_id;
                stat.anchor_create_time = create_time;
            }
            if let Some(segment) = active.as_mut() {
                if create_time > 0 {
                    segment.end_ts = create_time;
                    last_segment_ts = create_time;
                }
            }
        });
        close_message_cursor(cursor);
        if let Some(segment) = active.take() {
            commit_segment(&mut session_segments, segment);
        }
        let _ = processed;
        if stat.incoming + stat.outgoing > 0 || stat.latest > 0 {
            let replied = stat.incoming > 0 && stat.outgoing > 0;
            private_sessions.push(json!({
                "session_id": session_id,
                "incoming_count": stat.incoming,
                "outgoing_count": stat.outgoing,
                "replied": replied,
                "first_incoming_ts": stat.first_incoming,
                "first_reply_ts": stat.first_reply,
                "latest_ts": stat.latest,
                "anchor_local_id": stat.anchor_local_id,
                "anchor_create_time": stat.anchor_create_time,
            }));
        }
        segments.extend(session_segments.into_iter().map(|segment| {
            let start = if segment.start_ts > 0 {
                segment.start_ts
            } else {
                segment.anchor_create_time
            };
            let finish = if segment.end_ts > 0 { segment.end_ts } else { start };
            json!({
                "session_id": session_id,
                "segment_index": segment.segment_index,
                "start_ts": start,
                "end_ts": finish,
                "duration_sec": (finish - start).max(0),
                "incoming_count": segment.incoming,
                "outgoing_count": segment.outgoing,
                "message_count": segment.incoming + segment.outgoing,
                "replied": segment.incoming > 0 && segment.outgoing > 0,
                "first_incoming_ts": segment.first_incoming,
                "first_reply_ts": segment.first_reply,
                "latest_ts": finish,
                "anchor_local_id": segment.anchor_local_id,
                "anchor_create_time": start,
            })
        }));
    }
    private_sessions.sort_by(|a, b| {
        let left = a.get("latest_ts").and_then(|v| v.as_i64()).unwrap_or(0);
        let right = b.get("latest_ts").and_then(|v| v.as_i64()).unwrap_or(0);
        right.cmp(&left).then_with(|| {
            let left_id = a.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
            let right_id = b.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
            left_id.cmp(right_id)
        })
    });
    segments.sort_by(|a, b| {
        let left_start = a.get("start_ts").and_then(|v| v.as_i64()).unwrap_or(0);
        let right_start = b.get("start_ts").and_then(|v| v.as_i64()).unwrap_or(0);
        left_start.cmp(&right_start).then_with(|| {
            let left_id = a.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
            let right_id = b.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
            left_id.cmp(right_id).then_with(|| {
                let left_index = a.get("segment_index").and_then(|v| v.as_i64()).unwrap_or(0);
                let right_index = b.get("segment_index").and_then(|v| v.as_i64()).unwrap_or(0);
                left_index.cmp(&right_index)
            })
        })
    });

    let mut mentions: Vec<Value> = Vec::new();
    let mut mention_truncated = false;
    for session_id in &group_ids {
        let table = message_table_name(session_id);
        let where_clause = time_where(begin, end);
        let extra = if where_clause.is_empty() {
            "WHERE".to_string()
        } else {
            format!("{where_clause} AND")
        };
        for shard in open_shards(acc, &table) {
            if !shard.has_table {
                continue;
            }
            elapsed_dbs.insert(shard.path.clone());
            let columns = shard.columns_of(&table);
            let has_source = columns.iter().any(|column| column.eq_ignore_ascii_case("source"));
            let source_select = if has_source {
                "source"
            } else {
                "message_content AS source"
            };
            let conditions = if has_source {
                "(source LIKE '%atuserlist%' OR message_content LIKE '%atuserlist%')"
            } else {
                "message_content LIKE '%atuserlist%'"
            };
            let sql = format!(
                "SELECT local_id, create_time, real_sender_id, message_content, {source_select} \
                 FROM \"{table}\" {extra} {conditions} \
                 ORDER BY create_time DESC, local_id DESC"
            );
            for row in shard.query(&sql) {
                let Value::Object(obj) = row else { continue };
                let local_id = obj.get("local_id").and_then(|v| v.as_i64()).unwrap_or(0);
                let create_time = obj.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
                let sender_id = obj.get("real_sender_id").and_then(|v| v.as_i64()).unwrap_or(0);
                let sender = shard
                    .name2id
                    .get(&sender_id)
                    .cloned()
                    .unwrap_or_default();
                let message = features::decode_blob_text(
                    obj.get("message_content").unwrap_or(&Value::Null),
                );
                let mut source =
                    features::decode_blob_text(obj.get("source").unwrap_or(&Value::Null));
                if !source.to_lowercase().contains("atuserlist") {
                    source = message.clone();
                }
                mentions.push(json!({
                    "session_id": session_id,
                    "local_id": local_id,
                    "create_time": create_time,
                    "sender_username": sender,
                    "message_content": message,
                    "source": source,
                }));
            }
        }
    }
    mentions.sort_by(|a, b| {
        let left_time = a.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
        let right_time = b.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
        right_time.cmp(&left_time).then_with(|| {
            let left_id = a.get("local_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let right_id = b.get("local_id").and_then(|v| v.as_i64()).unwrap_or(0);
            right_id.cmp(&left_id)
        })
    });
    if mention_limit > 0 && mentions.len() > mention_limit {
        mentions.truncate(mention_limit);
        mention_truncated = true;
    }
    if mentions.len() > 50_000 {
        mentions.truncate(50_000);
        mention_truncated = true;
    }
    let mut groups: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for mention in &mentions {
        let session_id = mention
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let create_time = mention.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
        let entry = groups.entry(session_id).or_insert((0, 0));
        entry.0 += 1;
        if create_time > entry.1 {
            entry.1 = create_time;
        }
    }
    let mut mention_groups: Vec<(String, i64, i64)> = groups
        .into_iter()
        .map(|(session, (count, latest))| (session, count, latest))
        .collect();
    mention_groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)).then_with(|| a.0.cmp(&b.0)));
    let mention_groups_value = Value::Array(
        mention_groups
            .iter()
            .map(|(session, count, latest)| {
                json!({ "session_id": session, "count": count, "latest_ts": latest })
            })
            .collect(),
    );
    let inbound_people = private_sessions
        .iter()
        .filter(|item| item.get("incoming_count").and_then(|v| v.as_i64()).unwrap_or(0) > 0)
        .count();
    let replied_people = private_sessions
        .iter()
        .filter(|item| item.get("replied").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();
    let outbound_people = private_sessions
        .iter()
        .filter(|item| item.get("outgoing_count").and_then(|v| v.as_i64()).unwrap_or(0) > 0)
        .count();
    let reply_rate = if inbound_people > 0 {
        replied_people as f64 / inbound_people as f64
    } else {
        0.0
    };
    let mention_count: i64 = mention_groups.iter().map(|(_, count, _)| *count).sum();
    Ok(json!({
        "summary": {
            "private_inbound_people": inbound_people,
            "private_replied_people": replied_people,
            "private_outbound_people": outbound_people,
            "private_reply_rate": reply_rate,
            "mention_count": mention_count,
            "mention_group_count": mention_groups.len(),
        },
        "private_sessions": private_sessions,
        "private_segments": segments,
        "mentions": mentions,
        "mention_groups": mention_groups_value,
        "diagnostics": {
            "truncated": private_truncated || mention_truncated,
            "scanned_dbs": elapsed_dbs.len(),
            "elapsed_ms": started.elapsed().as_millis() as i64,
            "mention_truncated": mention_truncated,
            "private_truncated": private_truncated,
        },
    }))
}

fn row_is_send(row: &Map<String, Value>, my_account: &str) -> bool {
    if row.get("is_send").and_then(|v| v.as_i64()).unwrap_or(0) == 1 {
        return true;
    }
    if my_account.is_empty() {
        return false;
    }
    row.get("sender_username")
        .and_then(|v| v.as_str())
        .map(|sender| sender.eq_ignore_ascii_case(my_account))
        .unwrap_or(false)
}

fn commit_segment(segments: &mut Vec<FootprintSegment>, segment: FootprintSegment) {
    let start = if segment.start_ts > 0 {
        segment.start_ts
    } else {
        segment.anchor_create_time
    };
    let message_count = segment.incoming + segment.outgoing;
    if start > 0 && message_count > 0 {
        segments.push(segment);
    }
}

/* ---------------------------- misc statistics ---------------------------- */

pub fn available_years(acc: &Account, session_ids: &[String]) -> Vec<Value> {
    let ids = resolve_session_ids(acc, session_ids);
    let mut years: BTreeSet<i64> = BTreeSet::new();
    for session_id in ids {
        let table = message_table_name(&session_id);
        for shard in open_shards(acc, &table) {
            if !shard.has_table {
                continue;
            }
            let sql = format!(
                "SELECT DISTINCT CAST(strftime('%Y', create_time, 'unixepoch', 'localtime') AS INTEGER) AS y \
                 FROM \"{table}\""
            );
            for row in shard.query(&sql) {
                let Value::Object(map) = row else { continue };
                let year = map.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
                if year > 1970 {
                    years.insert(year);
                }
            }
        }
    }
    years.into_iter().map(Value::from).collect()
}

pub fn ai_query_session_candidates(acc: &Account, options_json: &str) -> Result<Value, i32> {
    let options: Value = serde_json::from_str(options_json).unwrap_or_else(|_| json!({}));
    let map = options.as_object().cloned().unwrap_or_default();
    let keyword = value_str(&map, &["keyword"]);
    if keyword.trim().is_empty() {
        return Ok(json!({ "candidates": [], "total": 0 }));
    }
    let session_id = value_str(&map, &["session_id", "sessionId"]);
    let limit = opt_i64(&map, &["limit"], 50).clamp(1, 500) as i32;
    let begin = opt_i64(&map, &["begin_timestamp", "beginTimestamp"], 0);
    let end = opt_i64(&map, &["end_timestamp", "endTimestamp"], 0);
    let rows = features::search_messages(
        acc,
        &session_id,
        &keyword,
        (limit * 10).min(2000),
        0,
        begin,
        end,
    )?;
    let mut grouped: HashMap<String, (i64, i64, String)> = HashMap::new();
    for row in rows {
        let Value::Object(obj) = row else { continue };
        let sender = value_str(&obj, &["sender_username"]);
        let create_time = obj.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
        let content = value_str(&obj, &["message_content"]);
        let entry = grouped
            .entry(sender)
            .or_insert((0, 0, String::new()));
        entry.0 += 1;
        if create_time >= entry.1 {
            entry.1 = create_time;
            entry.2 = content;
        }
    }
    let mut candidates: Vec<(String, i64, i64, String)> = grouped
        .into_iter()
        .map(|(sender, (count, latest, content))| (sender, count, latest, content))
        .collect();
    candidates.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.1.cmp(&a.1)));
    candidates.truncate(limit as usize);
    let total = candidates.len();
    Ok(json!({
        "candidates": candidates
            .into_iter()
            .map(|(username, count, latest_time, sample)| json!({
                "username": username,
                "count": count,
                "latest_time": latest_time,
                "sample": sample,
            }))
            .collect::<Vec<_>>(),
        "total": total,
    }))
}

/* ------------------------------- snapshots ------------------------------- */

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn sql_value_to_json(value: rusqlite::types::ValueRef<'_>) -> Value {
    match value {
        rusqlite::types::ValueRef::Null => Value::Null,
        rusqlite::types::ValueRef::Integer(number) => Value::from(number),
        rusqlite::types::ValueRef::Real(number) => serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        rusqlite::types::ValueRef::Text(text) => {
            Value::String(String::from_utf8_lossy(text).into_owned())
        }
        rusqlite::types::ValueRef::Blob(blob) => json!({ "__blob_hex": hex::encode(blob) }),
    }
}

fn json_to_sql(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(flag) => SqlValue::Integer(if *flag { 1 } else { 0 }),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                SqlValue::Integer(int)
            } else {
                SqlValue::Real(number.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(text) => SqlValue::Text(text.clone()),
        Value::Object(obj) => {
            if let Some(Value::String(hex_text)) = obj.get("__blob_hex") {
                if let Ok(bytes) = hex::decode(hex_text) {
                    return SqlValue::Blob(bytes);
                }
            }
            SqlValue::Text(serde_json::to_string(value).unwrap_or_default())
        }
        Value::Array(_) => SqlValue::Text(serde_json::to_string(value).unwrap_or_default()),
    }
}

pub fn export_table_snapshot(
    acc: &Account,
    kind: &str,
    db_path: &str,
    table: &str,
    output_path: &str,
) -> Result<Value, i32> {
    if output_path.trim().is_empty() || table.trim().is_empty() {
        return Err(ERR_ARG);
    }
    let path = resolve_snapshot_db(acc, kind, db_path)?;
    let conn = acc.db(&path)?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, table) {
        return Ok(json!({ "success": false, "error": "table not found" }));
    }
    let columns = table_columns(&guard, table);
    let sql = format!("SELECT * FROM {}", quote_ident(table));
    let mut stmt = guard.prepare(&sql).map_err(|_| ERR_SQL)?;
    let mut rows = stmt.query([]).map_err(|_| ERR_SQL)?;
    let mut payload_rows: Vec<Value> = Vec::new();
    while let Some(row) = rows.next().map_err(|_| ERR_SQL)? {
        let mut values = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            values.push(sql_value_to_json(
                row.get_ref(index).map_err(|_| ERR_SQL)?,
            ));
        }
        payload_rows.push(Value::Array(values));
    }
    let row_count = payload_rows.len();
    let column_count = columns.len();
    let payload = json!({
        "format": "weflow-table-snapshot",
        "version": 1,
        "table": table,
        "columns": columns,
        "rows": payload_rows,
    });
    if let Some(parent) = Path::new(output_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let bytes = serde_json::to_vec(&payload).map_err(|_| ERR_IO)?;
    std::fs::write(output_path, bytes).map_err(|_| ERR_IO)?;
    Ok(json!({
        "success": true,
        "rows": row_count,
        "columns": column_count,
        "path": output_path,
    }))
}

fn resolve_snapshot_db(acc: &Account, kind: &str, db_path: &str) -> Result<PathBuf, i32> {
    let trimmed = db_path.trim();
    if !trimmed.is_empty() {
        return Ok(PathBuf::from(trimmed));
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

pub fn import_table_snapshot(
    acc: &Account,
    kind: &str,
    db_path: &str,
    table: &str,
    input_path: &str,
    create_sql: Option<&str>,
) -> Result<Value, i32> {
    if input_path.trim().is_empty() || table.trim().is_empty() {
        return Err(ERR_ARG);
    }
    let path = resolve_snapshot_db(acc, kind, db_path)?;
    let raw = std::fs::read(input_path).map_err(|_| ERR_IO)?;
    let payload: Value = serde_json::from_slice(&raw).map_err(|_| ERR_IO)?;
    let Some(obj) = payload.as_object() else {
        return Ok(json!({ "success": false, "error": "invalid snapshot file" }));
    };
    let columns: Vec<String> = obj
        .get("columns")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|text| text.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if columns.is_empty() {
        return Ok(json!({ "success": false, "error": "snapshot has no columns" }));
    }
    let rows = obj
        .get("rows")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let conn = acc.db_rw(&path)?;
    if let Some(create) = create_sql {
        if !create.trim().is_empty() && !table_exists(&conn, table) {
            conn.execute_batch(create).map_err(|_| ERR_SQL)?;
        }
    }
    if !table_exists(&conn, table) {
        return Ok(json!({ "success": false, "error": "table not found" }));
    }
    let column_sql = columns
        .iter()
        .map(|name| quote_ident(name))
        .collect::<Vec<_>>()
        .join(",");
    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let insert_sql = format!(
        "INSERT OR IGNORE INTO {} ({column_sql}) VALUES ({placeholders})",
        quote_ident(table)
    );
    let tx = conn.unchecked_transaction().map_err(|_| ERR_SQL)?;
    let mut inserted = 0i64;
    let mut ignored = 0i64;
    let mut malformed = 0i64;
    {
        let mut stmt = tx.prepare(&insert_sql).map_err(|_| ERR_SQL)?;
        for row in &rows {
            let Some(values) = row.as_array() else {
                malformed += 1;
                continue;
            };
            if values.len() != columns.len() {
                malformed += 1;
                continue;
            }
            let params: Vec<SqlValue> = values.iter().map(json_to_sql).collect();
            match stmt.execute(rusqlite::params_from_iter(params.iter())) {
                Ok(0) => ignored += 1,
                Ok(_) => inserted += 1,
                Err(_) => malformed += 1,
            }
        }
    }
    tx.commit().map_err(|_| ERR_SQL)?;
    Ok(json!({
        "success": true,
        "rows": rows.len(),
        "inserted": inserted,
        "ignored": ignored,
        "malformed": malformed,
        "columns": columns.len(),
    }))
}

/* ------------------------------- diagnostics ----------------------------- */

static LOG_RING: Mutex<Option<Vec<String>>> = Mutex::new(None);

pub fn log(message: impl AsRef<str>) {
    let mut guard = LOG_RING.lock().unwrap_or_else(|e| e.into_inner());
    let ring = guard.get_or_insert_with(Vec::new);
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    ring.push(format!("[{}s] {}", seconds % 86_400, message.as_ref()));
    if ring.len() > 500 {
        ring.remove(0);
    }
}

pub fn logs() -> Vec<Value> {
    LOG_RING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|ring| ring.iter().cloned().map(Value::String).collect())
        .unwrap_or_default()
}

pub fn account_status(handle: i64, acc: &Account) -> Value {
    let paths = acc.open_db_paths();
    let dbs = Value::Array(
        paths
            .iter()
            .map(|path| json!({ "path": path, "open": true }))
            .collect(),
    );
    json!({
        "handle": handle,
        "account_dir": acc.account_dir.to_string_lossy(),
        "db_storage": acc.db_storage.to_string_lossy(),
        "open_db_count": paths.len(),
        "dbs": dbs,
    })
}

pub fn parse_string_list(json_text: &str) -> Vec<String> {
    match serde_json::from_str::<Value>(json_text) {
        Ok(Value::Array(items)) => string_array(&items),
        Ok(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        }
        _ => Vec::new(),
    }
}

pub fn parse_json_value(json_text: &str) -> Value {
    serde_json::from_str(json_text).unwrap_or_else(|_| json!({}))
}
