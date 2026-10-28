//! Core database layer: account state, SQLCipher open scheme, JSON marshalling.

use rusqlite::types::Value as SqlValue;
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{Map, Value};
use sha2::Sha512;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ERR_GENERIC: i32 = -1;
pub const ERR_ARG: i32 = -2;
pub const ERR_NOT_FOUND: i32 = -3;
pub const ERR_IO: i32 = -4;
pub const ERR_SQL: i32 = -5;
pub const ERR_SCHEMA: i32 = -7;

pub struct Account {
    pub account_dir: PathBuf,
    pub db_storage: PathBuf,
    pub master: Vec<u8>,
    pub my_account_id: Mutex<String>,
    conns: Mutex<HashMap<String, Arc<Mutex<Connection>>>>,
    name2id: Mutex<HashMap<String, Arc<HashMap<i64, String>>>>,
}

impl Account {
    pub fn new(account_dir: PathBuf, db_storage: PathBuf, master: Vec<u8>) -> Self {
        Self {
            account_dir,
            db_storage,
            master,
            my_account_id: Mutex::new(String::new()),
            conns: Mutex::new(HashMap::new()),
            name2id: Mutex::new(HashMap::new()),
        }
    }

    /// Cached `Name2Id` map for a message shard.
    pub fn name2id(&self, path: &Path) -> Arc<HashMap<i64, String>> {        let key = path.to_string_lossy().to_lowercase();
        if let Some(cached) = self
            .name2id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned()
        {
            return cached;
        }
        let map = match self.db(path) {
            Ok(conn) => {
                let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
                load_name2id(&guard).unwrap_or_default()
            }
            Err(_) => HashMap::new(),
        };
        let arc = Arc::new(map);
        self.name2id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, arc.clone());
        arc
    }

    pub fn my_account_id(&self) -> String {
        self.my_account_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_my_account_id(&self, value: &str) {
        *self.my_account_id.lock().unwrap_or_else(|e| e.into_inner()) = value.to_string();
    }

    /// Open (or reuse) a read-only SQLCipher connection for a database file.
    pub fn db(&self, path: &Path) -> Result<Arc<Mutex<Connection>>, i32> {
        let cache_key = path.to_string_lossy().to_lowercase();
        if let Some(existing) = self
            .conns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Ok(existing);
        }
        if !path.is_file() {
            return Err(ERR_NOT_FOUND);
        }
        let conn = open_wechat_db(path, &self.master).map_err(|_| ERR_SCHEMA)?;
        let arc = Arc::new(Mutex::new(conn));
        self.conns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(cache_key, arc.clone());
        Ok(arc)
    }

    /// Like [`Account::db`] but silently returns None on any failure.
    pub fn db_opt(&self, path: &Path) -> Option<Arc<Mutex<Connection>>> {
        self.db(path).ok()
    }

    /// Open a read-write connection (write features only; bypasses the cache).
    pub fn db_rw(&self, path: &Path) -> Result<Connection, i32> {
        if !path.is_file() {
            return Err(ERR_NOT_FOUND);
        }
        open_wechat_db_with(path, &self.master, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|_| ERR_SCHEMA)
    }

    pub fn session_db(&self) -> PathBuf {
        self.db_storage.join("session").join("session.db")
    }

    pub fn contact_db(&self) -> PathBuf {
        self.db_storage.join("contact").join("contact.db")
    }

    pub fn sns_db(&self) -> PathBuf {
        self.db_storage.join("sns").join("sns.db")
    }

    pub fn hardlink_db(&self) -> PathBuf {
        self.db_storage.join("hardlink").join("hardlink.db")
    }

    pub fn emoticon_db(&self) -> PathBuf {
        self.db_storage.join("emoticon").join("emoticon.db")
    }

    /// `message/message_N.db` shards, sorted by numeric suffix.
    /// Falls back to the legacy location directly under `db_storage`.
    pub fn message_dbs(&self) -> Vec<PathBuf> {
        let mut found = list_numbered_dbs(&self.db_storage.join("message"), "message_");
        if found.is_empty() {
            found = list_numbered_dbs(&self.db_storage, "message_");
        }
        found
    }

    /// `message/media_N.db` databases.
    pub fn media_dbs(&self) -> Vec<PathBuf> {
        let mut found = list_numbered_dbs(&self.db_storage.join("message"), "media_");
        if found.is_empty() {
            found = list_numbered_dbs(&self.db_storage, "media_");
        }
        found
    }

    /// Paths of currently cached read-only connections.
    pub fn open_db_paths(&self) -> Vec<String> {
        self.conns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Drop cached connections and their page caches.
    pub fn purge_idle(&self) {
        self.conns.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.name2id.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// Parse the account key. 64 hex chars are a raw WCDB master key; anything else
/// is treated as a passphrase and fed to PBKDF2 verbatim.
pub fn parse_master_key(key: &str) -> Vec<u8> {
    let trimmed = key.trim();
    let candidate = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if candidate.len() == 64 && candidate.bytes().all(|b| b.is_ascii_hexdigit()) {
        if let Ok(bytes) = hex::decode(candidate) {
            return bytes;
        }
    }
    trimmed.as_bytes().to_vec()
}

/// WeChat 4.x / WCDB open sequence: derive a 48-byte raw key outside SQLCipher.
pub fn open_wechat_db(path: &Path, master: &[u8]) -> Result<Connection, String> {
    open_wechat_db_with(path, master, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub fn open_wechat_db_with(
    path: &Path,
    master: &[u8],
    flags: OpenFlags,
) -> Result<Connection, String> {
    let mut salt = [0u8; 16];
    let mut file = File::open(path).map_err(|e| format!("open: {e}"))?;
    file.read_exact(&mut salt).map_err(|e| format!("salt: {e}"))?;

    let mut derived = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha512>(master, &salt, 256_000, &mut derived);
    let mut raw = Vec::with_capacity(48);
    raw.extend_from_slice(&derived);
    raw.extend_from_slice(&salt);
    let key_hex = hex::encode(raw);

    let conn = Connection::open_with_flags(path, flags)
        .map_err(|e| format!("connect: {e}"))?;
    let _ = conn.busy_timeout(Duration::from_millis(4000));
    conn.execute_batch(&format!(
        "PRAGMA cipher_page_size = 4096;\n\
         PRAGMA cipher_hmac_algorithm = HMAC_SHA512;\n\
         PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;\n\
         PRAGMA kdf_iter = 2;\n\
         PRAGMA key = \"x'{key_hex}'\";"
    ))
    .map_err(|e| format!("pragma: {e}"))?;
    // Activation: forces SQLCipher to verify the first page.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get::<_, i64>(0))
        .map_err(|e| format!("auth: {e}"))?;
    Ok(conn)
}

fn list_numbered_dbs(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<(u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name,
            None => continue,
        };
        let lower = name.to_lowercase();
        if !lower.starts_with(prefix) || !lower.ends_with(".db") || lower.contains("-") {
            continue;
        }
        let middle = &lower[prefix.len()..lower.len() - 3];
        let Ok(index) = middle.parse::<u64>() else {
            continue;
        };
        items.push((index, path));
    }
    items.sort_by_key(|(index, _)| *index);
    items.into_iter().map(|(_, path)| path).collect()
}

/// Convert every result row into a JSON object. BLOBs become lowercase hex.
pub fn query_json(
    conn: &Connection,
    sql: &str,
    params: &[SqlValue],
) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(|name| name.to_string())
        .collect();
    let mut rows = stmt.query(rusqlite::params_from_iter(params.iter()))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut map = Map::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            map.insert(name.clone(), cell_to_json(row.get_ref(index)?));
        }
        out.push(Value::Object(map));
    }
    Ok(out)
}

pub fn cell_to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(number) => Value::from(number),
        ValueRef::Real(number) => serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(text) => Value::String(String::from_utf8_lossy(text).into_owned()),
        ValueRef::Blob(blob) => Value::String(hex::encode(blob)),
    }
}

pub fn escape_sql(value: &str) -> String {
    value.replace('\'', "''")
}

pub fn sql_string(value: &str) -> String {
    format!("'{}'", escape_sql(value))
}

/// Extract a string field from a JSON object with tolerant aliases.
pub fn value_str(map: &Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(Value::String(text)) = map.get(*key) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    String::new()
}

/// Message table name: `Msg_<md5(peer_wxid)>`.
pub fn message_table_name(peer: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(peer.as_bytes());
    format!("Msg_{}", hex::encode(hasher.finalize()))
}

pub fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

/// Load `Name2Id` (rowid -> user_name) for a message shard.
pub fn load_name2id(conn: &Connection) -> rusqlite::Result<HashMap<i64, String>> {
    let mut map = HashMap::new();
    if !table_exists(conn, "Name2Id") {
        return Ok(map);
    }
    let mut stmt = conn.prepare("SELECT rowid, user_name FROM Name2Id")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let name: Option<String> = row.get(1)?;
        if let Some(name) = name {
            map.insert(id, name);
        }
    }
    Ok(map)
}
