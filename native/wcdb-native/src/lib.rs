//! C ABI surface for `wcdb_api.dll`.
//!
//! Every export mirrors the contract in `ABI-SPEC.md`. JSON payloads are
//! allocated here and must be released with [`wcdb_free_string`].

mod advanced;
mod core;
mod features;
mod media;
mod sns;
mod write;

use crate::core::Account;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const ERR_INVALID_HANDLE: i32 = -3003;
const ERR_NO_DB_STORAGE: i32 = -3001;
const ERR_NO_SESSION_DB: i32 = -3002;

static ACCOUNTS: Mutex<Option<HashMap<i64, Arc<Account>>>> = Mutex::new(None);
static NEXT_HANDLE: Mutex<i64> = Mutex::new(1);

fn accounts() -> std::sync::MutexGuard<'static, Option<HashMap<i64, Arc<Account>>>> {
    ACCOUNTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn account(handle: i64) -> Result<Arc<Account>, i32> {
    accounts()
        .as_ref()
        .and_then(|map| map.get(&handle))
        .cloned()
        .ok_or(ERR_INVALID_HANDLE)
}

unsafe fn cstr<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok()
}

unsafe fn text(ptr: *const c_char) -> String {
    cstr(ptr).unwrap_or("").to_string()
}

fn to_c_ptr(value: &str) -> *mut c_void {
    match CString::new(value.replace('\0', "")) {
        Ok(c) => c.into_raw() as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe fn emit_json(out: *mut *mut c_void, value: &Value) -> i32 {
    if out.is_null() {
        return core::ERR_ARG;
    }
    *out = to_c_ptr(&value.to_string());
    0
}

unsafe fn emit_text(out: *mut *mut c_void, value: &str) -> i32 {
    if out.is_null() {
        return core::ERR_ARG;
    }
    *out = to_c_ptr(value);
    0
}

unsafe fn emit_error(out: *mut *mut c_void, message: &str) -> i32 {
    if !out.is_null() {
        *out = to_c_ptr(message);
    }
    core::ERR_GENERIC
}

unsafe fn write_i32(out: *mut i32, value: i32) {
    if !out.is_null() {
        *out = value;
    }
}

fn value_array(items: Vec<Value>) -> Value {
    Value::Array(items)
}

fn clean_account_dir_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_lowercase();
    if let Some(rest) = lower.strip_prefix("accountid_") {
        let keep = rest.split('_').next().unwrap_or("").len();
        return trimmed[.."accountid_".len() + keep].to_string();
    }
    if let Some((base, suffix)) = trimmed.rsplit_once('_') {
        if suffix.len() == 4 && suffix.chars().all(|c| c.is_ascii_alphanumeric()) && !base.is_empty()
        {
            return base.to_string();
        }
    }
    trimmed.to_string()
}

/* ------------------------------ lifecycle ------------------------------- */

#[no_mangle]
pub extern "C" fn wcdb_init() -> i32 {
    advanced::log("wcdb_init");
    0
}

#[no_mangle]
pub extern "C" fn wcdb_shutdown() -> i32 {
    advanced::clear_cursors();
    if let Some(map) = accounts().as_mut() {
        map.clear();
    }
    advanced::log("wcdb_shutdown");
    0
}

#[no_mangle]
pub extern "C" fn wcdb_purge_memory() -> i32 {
    if let Some(map) = accounts().as_ref() {
        for acc in map.values() {
            acc.purge_idle();
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_open_account(
    path: *const c_char,
    key: *const c_char,
    out_handle: *mut i64,
) -> i32 {
    if out_handle.is_null() {
        return core::ERR_ARG;
    }
    *out_handle = 0;
    let path = match cstr(path) {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => return core::ERR_ARG,
    };
    let key = text(key);
    let session_db = PathBuf::from(&path);
    let Some(session_dir) = session_db.parent() else {
        return ERR_NO_SESSION_DB;
    };
    let Some(db_storage) = session_dir.parent() else {
        return ERR_NO_SESSION_DB;
    };
    let Some(account_dir) = db_storage.parent() else {
        return ERR_NO_SESSION_DB;
    };
    if !db_storage.is_dir() {
        advanced::log(format!("open_account: db_storage missing ({})", db_storage.display()));
        return ERR_NO_DB_STORAGE;
    }
    if !session_db.is_file() {
        advanced::log(format!("open_account: session.db missing ({})", session_db.display()));
        return ERR_NO_SESSION_DB;
    }
    let master = core::parse_master_key(&key);
    if core::open_wechat_db(&session_db, &master).is_err() {
        advanced::log("open_account: session.db cannot be opened with the provided key");
        return core::ERR_IO;
    }
    let acc = Arc::new(Account::new(
        account_dir.to_path_buf(),
        db_storage.to_path_buf(),
        master,
    ));
    if let Some(name) = account_dir.file_name().and_then(|name| name.to_str()) {
        acc.set_my_account_id(&clean_account_dir_name(name));
    }
    let mut guard = accounts();
    let map = guard.get_or_insert_with(HashMap::new);
    let mut next = NEXT_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
    let handle = *next;
    *next += 1;
    map.insert(handle, acc);
    *out_handle = handle;
    advanced::log(format!("open ok handle={handle}"));
    0
}

#[no_mangle]
pub extern "C" fn wcdb_close_account(handle: i64) -> i32 {
    if let Some(map) = accounts().as_mut() {
        map.remove(&handle);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_set_my_account_id(handle: i64, account_id: *const c_char) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    acc.set_my_account_id(&text(account_id));
    0
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_set_my_wxid(handle: i64, wxid: *const c_char) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    acc.set_my_account_id(&text(wxid));
    0
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_free_string(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    drop(CString::from_raw(ptr as *mut c_char));
}

/* ------------------------------- sessions -------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_sessions(handle: i64, out: *mut *mut c_void) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_sessions(&acc) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_mark_all_sessions_read(
    handle: i64,
    out_error: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::mark_all_sessions_read(&acc) {
        Ok(()) => 0,
        Err(message) => emit_error(out_error, &message),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_reorder_sessions_by_time(
    handle: i64,
    out_json: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::reorder_sessions_by_time(&acc) {
        Ok(value) => emit_json(out_json, &value),
        Err(message) => {
            let payload = json!({ "success": false, "error": message });
            emit_json(out_json, &payload);
            core::ERR_GENERIC
        }
    }
}

/* -------------------------------- messages ------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_messages(
    handle: i64,
    session_id: *const c_char,
    limit: i32,
    offset: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_messages(&acc, &text(session_id), limit, offset) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_messages_by_type(
    handle: i64,
    session_id: *const c_char,
    local_type: i64,
    ascending: i32,
    limit: i32,
    offset: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_messages_by_type(
        &acc,
        &text(session_id),
        local_type,
        ascending != 0,
        limit,
        offset,
    ) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_count(
    handle: i64,
    session_id: *const c_char,
    out_count: *mut i32,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_message_count(&acc, &text(session_id)) {
        Ok(count) => {
            write_i32(out_count, count.min(i32::MAX as i64) as i32);
            0
        }
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_by_id(
    handle: i64,
    session_id: *const c_char,
    local_id: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_message_by_local_id(&acc, &text(session_id), local_id) {
        Ok(Some(row)) => emit_json(out, &row),
        Ok(None) => emit_json(out, &json!({})),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_by_svrid(
    handle: i64,
    session_id: *const c_char,
    svr_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_message_by_server_id(&acc, &text(session_id), &text(svr_id)) {
        Ok(Some(row)) => emit_json(out, &row),
        Ok(None) => emit_json(out, &json!({})),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_session_message_counts(
    handle: i64,
    session_ids_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(session_ids_json));
    emit_json(out, &features::session_message_counts(&acc, &ids))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_session_message_type_stats(
    handle: i64,
    session_id: *const c_char,
    begin: i32,
    end: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let stats = features::session_message_type_stats(
        &acc,
        &text(session_id),
        begin as i64,
        end as i64,
        true,
    );
    emit_json(out, &stats)
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_session_message_type_stats_batch(
    handle: i64,
    session_ids_json: *const c_char,
    options_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(session_ids_json));
    let options = advanced::parse_json_value(&text(options_json));
    let begin = options.get("begin").and_then(Value::as_i64).unwrap_or(0);
    let end = options.get("end").and_then(Value::as_i64).unwrap_or(0);
    let include_date_counts = options
        .get("include_date_counts")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut map = Map::new();
    for id in ids {
        let stats =
            features::session_message_type_stats(&acc, &id, begin, end, include_date_counts);
        map.insert(id, stats);
    }
    emit_json(out, &Value::Object(map))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_session_message_date_counts(
    handle: i64,
    session_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(
        out,
        &features::session_message_date_counts(&acc, &text(session_id)),
    )
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_session_message_date_counts_batch(
    handle: i64,
    session_ids_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(session_ids_json));
    let mut map = Map::new();
    for id in ids {
        let counts = features::session_message_date_counts(&acc, &id);
        map.insert(id, counts);
    }
    emit_json(out, &Value::Object(map))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_open_message_cursor(
    handle: i64,
    session_id: *const c_char,
    batch_size: i32,
    ascending: i32,
    begin: i32,
    end: i32,
    out_cursor: *mut i64,
) -> i32 {
    if out_cursor.is_null() {
        return core::ERR_ARG;
    }
    *out_cursor = 0;
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match advanced::open_message_cursor(
        &acc,
        &text(session_id),
        batch_size,
        ascending != 0,
        begin as i64,
        end as i64,
    ) {
        Ok(cursor) => {
            *out_cursor = cursor;
            0
        }
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_fetch_message_batch(
    handle: i64,
    cursor: i64,
    out: *mut *mut c_void,
    out_has_more: *mut i32,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match advanced::fetch_message_batch(&acc, cursor) {
        Ok((rows, has_more)) => {
            write_i32(out_has_more, if has_more { 1 } else { 0 });
            emit_json(out, &value_array(rows))
        }
        Err(code) => code,
    }
}

#[no_mangle]
pub extern "C" fn wcdb_close_message_cursor(_handle: i64, cursor: i64) -> i32 {
    advanced::close_message_cursor(cursor);
    0
}

#[no_mangle]
pub extern "C" fn wcdb_set_message_cursor_projection(
    _handle: i64,
    cursor: i64,
    projection: i32,
) -> i32 {
    if advanced::set_message_cursor_projection(cursor, projection) {
        0
    } else {
        core::ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_search_messages(
    handle: i64,
    session_id: *const c_char,
    keyword: *const c_char,
    limit: i32,
    offset: i32,
    begin: i32,
    end: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::search_messages(
        &acc,
        &text(session_id),
        &text(keyword),
        limit,
        offset,
        begin as i64,
        end as i64,
    ) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_ai_query_session_candidates(
    handle: i64,
    options_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match advanced::ai_query_session_candidates(&acc, &text(options_json)) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

/* -------------------------------- contacts ------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_contact(
    handle: i64,
    username: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_contact(&acc, &text(username)) {
        Ok(Some(row)) => emit_json(out, &row),
        Ok(None) => emit_json(out, &json!({})),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_contact_status(
    handle: i64,
    usernames_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let usernames = advanced::parse_string_list(&text(usernames_json));
    match features::contact_status(&acc, &usernames) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_contact_type_counts(
    handle: i64,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(out, &features::contact_type_counts(&acc))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_contacts_compact(
    handle: i64,
    usernames_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let usernames = advanced::parse_string_list(&text(usernames_json));
    match features::contacts_compact(&acc, &usernames) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_contact_alias_map(
    handle: i64,
    usernames_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let usernames = advanced::parse_string_list(&text(usernames_json));
    match features::contact_alias_map(&acc, &usernames) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_contact_friend_flags(
    handle: i64,
    usernames_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let usernames = advanced::parse_string_list(&text(usernames_json));
    match features::contact_friend_flags(&acc, &usernames) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_chat_room_ext_buffer(
    handle: i64,
    chatroom_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::chat_room_ext_buffer(&acc, &text(chatroom_id)) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_display_names(
    handle: i64,
    usernames_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let usernames = advanced::parse_string_list(&text(usernames_json));
    match features::display_names(&acc, &usernames) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_avatar_urls(
    handle: i64,
    usernames_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let usernames = advanced::parse_string_list(&text(usernames_json));
    match features::avatar_urls(&acc, &usernames) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_head_image_buffers(
    handle: i64,
    usernames_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let usernames = advanced::parse_string_list(&text(usernames_json));
    emit_json(out, &media::head_image_buffers(&acc, &usernames))
}

/* --------------------------------- groups -------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_group_member_count(
    handle: i64,
    chatroom_id: *const c_char,
    out_count: *mut i32,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let count = features::group_member_count(&acc, &text(chatroom_id));
    write_i32(out_count, count.min(i32::MAX as i64) as i32);
    0
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_group_member_counts(
    handle: i64,
    chatroom_ids_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(chatroom_ids_json));
    emit_json(out, &features::group_member_counts(&acc, &ids))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_group_members(
    handle: i64,
    chatroom_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::group_members(&acc, &text(chatroom_id)) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_group_nicknames(
    handle: i64,
    chatroom_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(
        out,
        &features::group_nicknames(&acc, &text(chatroom_id)),
    )
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_group_stats(
    handle: i64,
    chatroom_id: *const c_char,
    begin: i32,
    end: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(
        out,
        &advanced::group_stats(&acc, &text(chatroom_id), begin as i64, end as i64),
    )
}

/* ------------------------------ write operations -------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_update_message(
    handle: i64,
    session_id: *const c_char,
    local_id: i64,
    create_time: i32,
    new_content: *const c_char,
    out_error: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::update_message(
        &acc,
        &text(session_id),
        local_id,
        create_time as i64,
        &text(new_content),
    ) {
        Ok(()) => 0,
        Err(message) => emit_error(out_error, &message),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_insert_text_message(
    handle: i64,
    session_id: *const c_char,
    content: *const c_char,
    status: i64,
    create_time: i64,
    out_json: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::insert_text_message(
        &acc,
        &text(session_id),
        &text(content),
        status,
        create_time,
    ) {
        Ok(value) => emit_json(out_json, &value),
        Err(message) => emit_error(out_json, &message),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_delete_message(
    handle: i64,
    session_id: *const c_char,
    local_id: i64,
    create_time: i32,
    db_path_hint: *const c_char,
    out_error: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::delete_message(
        &acc,
        &text(session_id),
        local_id,
        create_time as i64,
        &text(db_path_hint),
    ) {
        Ok(()) => 0,
        Err(message) => emit_error(out_error, &message),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_install_message_anti_revoke_trigger(
    handle: i64,
    session_id: *const c_char,
    out_error: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::install_anti_revoke(&acc, &text(session_id)) {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(message) => emit_error(out_error, &message),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_uninstall_message_anti_revoke_trigger(
    handle: i64,
    session_id: *const c_char,
    out_error: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::uninstall_anti_revoke(&acc, &text(session_id)) {
        Ok(()) => 0,
        Err(message) => emit_error(out_error, &message),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_check_message_anti_revoke_trigger(
    handle: i64,
    session_id: *const c_char,
    out_installed: *mut i32,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let installed = write::check_anti_revoke(&acc, &text(session_id));
    write_i32(out_installed, if installed { 1 } else { 0 });
    0
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_install_sns_block_delete_trigger(
    handle: i64,
    out_error: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::install_sns_block_delete(&acc) {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(message) => emit_error(out_error, &message),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_uninstall_sns_block_delete_trigger(
    handle: i64,
    out_error: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::uninstall_sns_block_delete(&acc) {
        Ok(()) => 0,
        Err(message) => emit_error(out_error, &message),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_check_sns_block_delete_trigger(
    handle: i64,
    out_installed: *mut i32,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let installed = write::check_sns_block_delete(&acc);
    write_i32(out_installed, if installed { 1 } else { 0 });
    0
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_delete_sns_post(
    handle: i64,
    post_id: *const c_char,
    out_error: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match write::delete_sns_post(&acc, &text(post_id)) {
        Ok(()) => 0,
        Err(message) => emit_error(out_error, &message),
    }
}

/* ---------------------------------- media -------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_scan_media_stream(
    handle: i64,
    session_ids_json: *const c_char,
    media_type: i32,
    begin: i32,
    end: i32,
    limit: i32,
    offset: i32,
    out: *mut *mut c_void,
    out_has_more: *mut i32,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(session_ids_json));
    let (rows, has_more) = media::scan_media_stream(
        &acc,
        &ids,
        media_type,
        begin as i64,
        end as i64,
        limit,
        offset,
    );
    write_i32(out_has_more, if has_more { 1 } else { 0 });
    emit_json(out, &value_array(rows))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_resolve_image_hardlink(
    handle: i64,
    md5: *const c_char,
    account_dir: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(
        out,
        &media::resolve_image(&acc, &text(md5), &text(account_dir)),
    )
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_resolve_image_hardlink_batch(
    handle: i64,
    requests_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let requests = advanced::parse_json_value(&text(requests_json));
    let items = requests.as_array().cloned().unwrap_or_default();
    emit_json(out, &value_array(media::resolve_image_batch(&acc, &items)))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_resolve_video_hardlink_md5(
    handle: i64,
    md5: *const c_char,
    db_path: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(
        out,
        &media::resolve_video(&acc, &text(md5), &text(db_path)),
    )
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_resolve_video_hardlink_md5_batch(
    handle: i64,
    requests_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let requests = advanced::parse_json_value(&text(requests_json));
    let items = requests.as_array().cloned().unwrap_or_default();
    emit_json(out, &value_array(media::resolve_video_batch(&acc, &items)))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_voice_data(
    handle: i64,
    session_id: *const c_char,
    create_time: i32,
    local_id: i32,
    svr_id: i64,
    candidates_json: *const c_char,
    out_hex: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let candidates = advanced::parse_string_list(&text(candidates_json));
    let hex = media::voice_data(
        &acc,
        &text(session_id),
        create_time as i64,
        local_id as i64,
        svr_id,
        &candidates,
    )
    .unwrap_or_default();
    emit_text(out_hex, &hex)
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_voice_data_batch(
    handle: i64,
    requests_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let requests = advanced::parse_json_value(&text(requests_json));
    let items = requests.as_array().cloned().unwrap_or_default();
    emit_json(out, &value_array(media::voice_data_batch(&acc, &items)))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_list_message_dbs(
    handle: i64,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(out, &value_array(features::list_message_dbs(&acc)))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_list_media_dbs(handle: i64, out: *mut *mut c_void) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(out, &value_array(features::list_media_dbs(&acc)))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_media_schema_summary(
    handle: i64,
    db_path: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match media::media_schema_summary(&acc, &text(db_path)) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_emoticon_cdn_url(
    handle: i64,
    db_path: *const c_char,
    md5: *const c_char,
    out_url: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let url = media::emoticon_cdn_url(&acc, &text(db_path), &text(md5));
    emit_text(out_url, &url)
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_emoticon_caption(
    handle: i64,
    db_path: *const c_char,
    md5: *const c_char,
    out_caption: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let caption = media::emoticon_caption(&acc, &text(db_path), &text(md5));
    emit_text(out_caption, &caption)
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_emoticon_caption_strict(
    handle: i64,
    md5: *const c_char,
    out_caption: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let caption = media::emoticon_caption(&acc, "", &text(md5));
    emit_text(out_caption, &caption)
}

/* ----------------------------------- sns --------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_sns_timeline(
    handle: i64,
    limit: i32,
    offset: i32,
    username: *const c_char,
    keyword: *const c_char,
    start_time: i32,
    end_time: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match sns::timeline(
        &acc,
        limit,
        offset,
        &text(username),
        &text(keyword),
        start_time as i64,
        end_time as i64,
    ) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_sns_annual_stats(
    handle: i64,
    begin: i32,
    end: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match sns::annual_stats(&acc, begin as i64, end as i64) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_sns_usernames(
    handle: i64,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match sns::usernames(&acc) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_sns_export_stats(
    handle: i64,
    my_account_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let my_id = text(my_account_id);
    let my_id = if my_id.trim().is_empty() {
        acc.my_account_id()
    } else {
        my_id
    };
    match sns::export_stats(&acc, &my_id) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

/* ------------------------------- aggregates ------------------------------ */

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_aggregate_stats(
    handle: i64,
    session_ids_json: *const c_char,
    begin: i32,
    end: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(session_ids_json));
    match advanced::aggregate_stats(&acc, &ids, begin as i64, end as i64) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_available_years(
    handle: i64,
    session_ids_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(session_ids_json));
    emit_json(out, &value_array(advanced::available_years(&acc, &ids)))
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_annual_report_stats(
    handle: i64,
    session_ids_json: *const c_char,
    begin: i32,
    end: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(session_ids_json));
    match advanced::annual_report_stats(&acc, &ids, begin as i64, end as i64) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_annual_report_extras(
    handle: i64,
    session_ids_json: *const c_char,
    begin: i32,
    end: i32,
    peak_begin: i32,
    peak_end: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let ids = advanced::parse_string_list(&text(session_ids_json));
    match advanced::annual_report_extras(
        &acc,
        &ids,
        begin as i64,
        end as i64,
        peak_begin as i64,
        peak_end as i64,
    ) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_dual_report_stats(
    handle: i64,
    session_id: *const c_char,
    begin: i32,
    end: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match advanced::dual_report_stats(&acc, &text(session_id), begin as i64, end as i64) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_my_footprint_stats(
    handle: i64,
    options_json: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match advanced::my_footprint_stats(&acc, &text(options_json)) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_dates(
    handle: i64,
    session_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_message_dates(&acc, &text(session_id)) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

/* --------------------------- db introspection ---------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_tables(
    handle: i64,
    session_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(
        out,
        &value_array(features::get_message_tables(&acc, &text(session_id))),
    )
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_table_stats(
    handle: i64,
    session_id: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(
        out,
        &value_array(features::get_message_table_stats(&acc, &text(session_id))),
    )
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_meta(
    handle: i64,
    db_path: *const c_char,
    table_name: *const c_char,
    limit: i32,
    offset: i32,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_message_meta(&acc, &text(db_path), &text(table_name), limit, offset) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_table_columns(
    handle: i64,
    db_path: *const c_char,
    table_name: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_message_table_columns(&acc, &text(db_path), &text(table_name)) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_message_table_time_range(
    handle: i64,
    db_path: *const c_char,
    table_name: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_message_table_time_range(&acc, &text(db_path), &text(table_name)) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_list_tables(
    handle: i64,
    kind: *const c_char,
    db_path: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::list_tables(&acc, &text(kind), &text(db_path)) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_table_schema(
    handle: i64,
    kind: *const c_char,
    db_path: *const c_char,
    table_name: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::get_table_schema(&acc, &text(kind), &text(db_path), &text(table_name)) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_exec_query(
    handle: i64,
    kind: *const c_char,
    path: *const c_char,
    sql: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match features::exec_query(&acc, &text(kind), &text(path), &text(sql)) {
        Ok(rows) => emit_json(out, &value_array(rows)),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_export_table_snapshot(
    handle: i64,
    kind: *const c_char,
    db_path: *const c_char,
    table_name: *const c_char,
    output_path: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match advanced::export_table_snapshot(
        &acc,
        &text(kind),
        &text(db_path),
        &text(table_name),
        &text(output_path),
    ) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_import_table_snapshot(
    handle: i64,
    kind: *const c_char,
    db_path: *const c_char,
    table_name: *const c_char,
    input_path: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    match advanced::import_table_snapshot(
        &acc,
        &text(kind),
        &text(db_path),
        &text(table_name),
        &text(input_path),
        None,
    ) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_import_table_snapshot_with_schema(
    handle: i64,
    kind: *const c_char,
    db_path: *const c_char,
    table_name: *const c_char,
    input_path: *const c_char,
    create_table_sql: *const c_char,
    out: *mut *mut c_void,
) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    let schema = text(create_table_sql);
    let schema = if schema.trim().is_empty() {
        None
    } else {
        Some(schema.as_str())
    };
    match advanced::import_table_snapshot(
        &acc,
        &text(kind),
        &text(db_path),
        &text(table_name),
        &text(input_path),
        schema,
    ) {
        Ok(value) => emit_json(out, &value),
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_db_status(handle: i64, out: *mut *mut c_void) -> i32 {
    let acc = match account(handle) {
        Ok(acc) => acc,
        Err(code) => return code,
    };
    emit_json(out, &advanced::account_status(handle, &acc))
}

/* -------------------------------- misc ----------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_logs(out: *mut *mut c_void) -> i32 {
    emit_json(out, &value_array(advanced::logs()))
}

#[no_mangle]
pub extern "C" fn wcdb_start_monitor_pipe() -> i32 {
    advanced::log("wcdb_start_monitor_pipe: not supported");
    core::ERR_GENERIC
}

#[no_mangle]
pub extern "C" fn wcdb_stop_monitor_pipe() {}

#[no_mangle]
pub unsafe extern "C" fn wcdb_get_monitor_pipe_name(out_name: *mut *mut c_void) -> i32 {
    emit_text(out_name, r"\\.\pipe\weflow_monitor")
}

#[no_mangle]
pub extern "C" fn wcdb_cloud_init(_interval_seconds: i32) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn wcdb_cloud_report(_stats_json: *const c_char) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn wcdb_cloud_stop() {}

#[no_mangle]
pub unsafe extern "C" fn VerifyUser(
    _hwnd: i64,
    _message: *const c_char,
    out_result: *mut c_char,
    max_len: i32,
) {
    if out_result.is_null() || max_len <= 0 {
        return;
    }
    let payload = "{\"success\":false,\"error\":\"VerifyUser is not supported by this native component\"}";
    let bytes = payload.as_bytes();
    let limit = (max_len as usize).saturating_sub(1).min(bytes.len());
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_result as *mut u8, limit);
    *out_result.add(limit) = 0;
}
