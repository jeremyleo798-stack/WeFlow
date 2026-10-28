//! SNS (Moments) timeline: XML payload parsing and interaction joins.

use crate::core::*;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};

fn unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

pub(crate) fn decode_content(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.len() > 32
                && trimmed.len() % 2 == 0
                && trimmed.bytes().all(|b| b.is_ascii_hexdigit())
            {
                if let Ok(bytes) = hex::decode(trimmed) {
                    return String::from_utf8_lossy(&bytes).into_owned();
                }
            }
            text.clone()
        }
        _ => String::new(),
    }
}

pub(crate) fn parse_attrs(block: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let chars: Vec<char> = block.chars().collect();
    let mut i = 0;
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] == '/' || chars[i] == '>' {
            break;
        }
        let start = i;
        while i < chars.len()
            && chars[i] != '='
            && !chars[i].is_whitespace()
            && chars[i] != '>'
            && chars[i] != '/'
        {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '=' {
            if !name.is_empty() {
                out.insert(name, String::new());
            }
            continue;
        }
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let quote = chars[i];
        if quote == '"' || quote == '\'' {
            i += 1;
            let value_start = i;
            while i < chars.len() && chars[i] != quote {
                i += 1;
            }
            let value: String = chars[value_start..i].iter().collect();
            if !name.is_empty() {
                out.insert(name, unescape(&value));
            }
            if i < chars.len() {
                i += 1;
            }
        } else {
            let value_start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
                i += 1;
            }
            let value: String = chars[value_start..i].iter().collect();
            if !name.is_empty() {
                out.insert(name, unescape(&value));
            }
        }
    }
    out
}

/// Locate every `<tag ...>...</tag>` / `<tag .../>` element, tolerating nesting.
pub(crate) fn find_elements(xml: &str, tag: &str) -> Vec<String> {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let mut out = Vec::new();
    let mut search = 0usize;
    while let Some(offset) = xml[search..].find(&open_pat) {
        let start = search + offset;
        let boundary = xml[start + open_pat.len()..].chars().next();
        let boundary_ok = matches!(boundary, Some(c) if c.is_whitespace() || c == '>' || c == '/');
        if !boundary_ok {
            search = start + open_pat.len();
            continue;
        }
        let Some(gt_offset) = xml[start..].find('>') else {
            break;
        };
        let tag_end = start + gt_offset;
        if xml[start..tag_end].ends_with('/') {
            out.push(xml[start..=tag_end].to_string());
            search = tag_end + 1;
            continue;
        }
        let mut depth = 1usize;
        let mut cursor = tag_end + 1;
        loop {
            let next_open = xml[cursor..].find(&open_pat).map(|p| cursor + p);
            let next_close = xml[cursor..].find(&close_pat).map(|p| cursor + p);
            match (next_open, next_close) {
                (Some(open_pos), Some(close_pos)) if open_pos < close_pos => {
                    depth += 1;
                    cursor = open_pos + open_pat.len();
                }
                (_, Some(close_pos)) => {
                    depth -= 1;
                    cursor = close_pos + close_pat.len();
                    if depth == 0 {
                        out.push(xml[start..cursor].to_string());
                        break;
                    }
                }
                _ => {
                    cursor = xml.len();
                    break;
                }
            }
        }
        if cursor >= xml.len() {
            break;
        }
        search = cursor;
    }
    out
}

fn inner(element: &str) -> &str {
    let Some(gt) = element.find('>') else {
        return "";
    };
    if element[..gt].ends_with('/') || element.len() <= gt + 1 {
        return "";
    }
    let Some(close) = element.rfind("</") else {
        return "";
    };
    if close <= gt {
        return "";
    }
    &element[gt + 1..close]
}

pub(crate) fn text_of(xml: &str, tag: &str) -> String {
    let Some(element) = find_elements(xml, tag).into_iter().next() else {
        return String::new();
    };
    let raw = inner(&element).trim();
    let raw = raw
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(raw);
    unescape(raw)
}

fn media_item(element: &str) -> Value {
    let size = find_elements(element, "size").into_iter().next().unwrap_or_default();
    let size_attrs = parse_attrs(&size);
    let thumb = find_elements(element, "thumb").into_iter().next().unwrap_or_default();
    let thumb_attrs = parse_attrs(&thumb);
    let url = find_elements(element, "url").into_iter().next().unwrap_or_default();
    let url_attrs = parse_attrs(&url);
    let enc = find_elements(element, "enc").into_iter().next().unwrap_or_default();
    let enc_attrs = parse_attrs(&enc);
    let live = find_elements(element, "liveMedia").into_iter().next();
    let live_photo = live.map(|block| {
        let live_thumb = find_elements(&block, "thumb").into_iter().next().unwrap_or_default();
        let live_thumb_attrs = parse_attrs(&live_thumb);
        let live_url = find_elements(&block, "url").into_iter().next().unwrap_or_default();
        let live_url_attrs = parse_attrs(&live_url);
        json!({
            "url": unescape(inner(&live_url).trim()),
            "thumb": unescape(inner(&live_thumb).trim()),
            "md5": live_url_attrs.get("md5").cloned().unwrap_or_default(),
            "token": live_url_attrs.get("token").cloned().unwrap_or_default(),
            "thumbToken": live_thumb_attrs.get("token").cloned().unwrap_or_default(),
            "key": live_url_attrs.get("key").cloned().unwrap_or_default(),
            "encIdx": live_url_attrs.get("enc_idx").cloned().unwrap_or_default(),
        })
    });
    json!({
        "id": text_of(element, "id"),
        "type": text_of(element, "type").parse::<i64>().unwrap_or(0),
        "description": text_of(element, "description"),
        "thumb": unescape(inner(&thumb).trim()),
        "url": unescape(inner(&url).trim()),
        "md5": url_attrs.get("md5").cloned().unwrap_or_default(),
        "token": url_attrs.get("token").cloned().unwrap_or_default(),
        "thumbToken": thumb_attrs.get("token").cloned().unwrap_or_default(),
        "key": url_attrs.get("key").cloned().unwrap_or_default(),
        "encIdx": url_attrs.get("enc_idx").cloned().unwrap_or_default(),
        "encKey": enc_attrs.get("key").cloned().unwrap_or_default(),
        "width": size_attrs.get("width").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
        "height": size_attrs.get("height").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
        "totalSize": size_attrs.get("totalSize").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
        "videoDuration": text_of(element, "videoDuration").parse::<f64>().unwrap_or(0.0),
        "videomd5": url_attrs.get("videomd5").cloned().unwrap_or_default(),
        "livePhoto": live_photo,
    })
}

fn location_of(xml: &str) -> Option<Value> {
    let element = find_elements(xml, "location").into_iter().next()?;
    let attrs = parse_attrs(&element);
    let body = inner(&element);
    let pick = |key: &str| -> String {
        attrs
            .get(key)
            .cloned()
            .or_else(|| {
                let value = text_of(body, key);
                if value.is_empty() {
                    None
                } else {
                    Some(value)
                }
            })
            .unwrap_or_default()
    };
    let latitude = pick("latitude");
    let longitude = pick("longitude");
    let poi_name = pick("poiName");
    if latitude.is_empty()
        && longitude.is_empty()
        && poi_name.is_empty()
        && pick("city").is_empty()
        && pick("poiAddress").is_empty()
    {
        return None;
    }
    Some(json!({
        "latitude": latitude.parse::<f64>().unwrap_or(0.0),
        "longitude": longitude.parse::<f64>().unwrap_or(0.0),
        "city": pick("city"),
        "country": pick("country"),
        "poiName": poi_name,
        "poiAddress": pick("poiAddress"),
        "poiAddressName": pick("poiAddressName"),
        "label": pick("label"),
    }))
}

fn tid_seconds(tid: i64) -> i64 {
    (((tid as u64) >> 23) / 1000) as i64
}

fn tid_for_seconds(seconds: i64) -> i64 {
    if seconds <= 0 {
        i64::MIN
    } else {
        ((seconds as u64).wrapping_mul(1000) << 23) as i64
    }
}

fn parse_post(xml: &str, tid: i64, nickname_hint: &str) -> Value {
    let timeline = find_elements(xml, "TimelineObject")
        .into_iter()
        .next()
        .unwrap_or_else(|| xml.to_string());
    let content_object = find_elements(&timeline, "ContentObject")
        .into_iter()
        .next()
        .unwrap_or_default();
    let media_list = find_elements(&content_object, "mediaList")
        .into_iter()
        .next()
        .unwrap_or_default();
    let media: Vec<Value> = find_elements(&media_list, "media")
        .iter()
        .map(|element| media_item(element))
        .collect();
    let local_extra = find_elements(xml, "LocalExtraInfo")
        .into_iter()
        .next()
        .unwrap_or_default();
    let mut nickname = text_of(&local_extra, "nickname");
    if nickname.is_empty() {
        nickname = nickname_hint.to_string();
    }
    let create_time: i64 = text_of(&timeline, "createTime")
        .parse()
        .unwrap_or_else(|_| tid_seconds(tid));
    let post_type: i64 = text_of(&content_object, "type").parse().unwrap_or(0);
    let mut post = Map::new();
    post.insert("id".into(), Value::String(text_of(&timeline, "id")));
    post.insert("tid".into(), Value::String(tid.to_string()));
    post.insert("username".into(), Value::String(text_of(&timeline, "username")));
    post.insert("nickname".into(), Value::String(nickname));
    post.insert("createTime".into(), Value::from(create_time));
    post.insert("contentDesc".into(), Value::String(text_of(&timeline, "contentDesc")));
    post.insert("type".into(), Value::from(post_type));
    post.insert("media".into(), Value::Array(media));
    post.insert("rawXml".into(), Value::String(xml.to_string()));
    post.insert("linkTitle".into(), Value::String(text_of(&content_object, "title")));
    post.insert("linkUrl".into(), Value::String(text_of(&content_object, "contentUrl")));
    post.insert("description".into(), Value::String(text_of(&content_object, "description")));
    if let Some(location) = location_of(&timeline) {
        post.insert("location".into(), location);
    }
    Value::Object(post)
}

fn comment_emojis(content: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for element in find_elements(content, "emoji") {
        let attrs = parse_attrs(&element);
        let url = attrs.get("url").cloned().unwrap_or_default();
        if url.is_empty() && attrs.get("md5").map(|v| v.is_empty()).unwrap_or(true) {
            continue;
        }
        out.push(json!({
            "url": url,
            "md5": attrs.get("md5").cloned().unwrap_or_default(),
            "width": attrs.get("width").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
            "height": attrs.get("height").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
            "encryptUrl": attrs.get("encrypturl").or_else(|| attrs.get("encrypt_url")).cloned().unwrap_or_default(),
            "aesKey": attrs.get("aeskey").or_else(|| attrs.get("aes_key")).cloned().unwrap_or_default(),
        }));
    }
    out
}

fn load_interactions(
    conn: &rusqlite::Connection,
    tids: &[i64],
) -> HashMap<i64, (Vec<Value>, Vec<String>)> {
    let mut result: HashMap<i64, (Vec<Value>, Vec<String>)> = HashMap::new();
    if tids.is_empty() {
        return result;
    }
    let list = tids
        .iter()
        .map(|tid| tid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT feed_id, type, from_username, from_nickname, to_username, to_nickname, \
         content, create_time, comment_id FROM SnsMessage_tmp3 \
         WHERE feed_id IN ({list}) ORDER BY create_time"
    );
    let rows = query_json(conn, &sql, &[]).unwrap_or_default();
    for row in rows {
        let Value::Object(map) = row else { continue };
        let feed = map
            .get("feed_id")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0);
        let kind = map.get("type").and_then(|v| v.as_i64()).unwrap_or(0);
        let entry = result.entry(feed).or_insert_with(|| (Vec::new(), Vec::new()));
        if kind == 1 {
            let mut name = value_str(&map, &["from_nickname"]);
            if name.is_empty() {
                name = value_str(&map, &["from_username"]);
            }
            if !name.is_empty() {
                entry.1.push(name);
            }
        } else if kind == 2 {
            let content = value_str(&map, &["content"]);
            let readable = if content.contains('<') {
                text_of(&content, "text")
            } else {
                content.clone()
            };
            let mut comment = Map::new();
            comment.insert(
                "id".into(),
                Value::String(value_str(&map, &["comment_id"])),
            );
            comment.insert(
                "nickname".into(),
                Value::String(value_str(&map, &["from_nickname", "from_username"])),
            );
            comment.insert("content".into(), Value::String(readable));
            comment.insert("refCommentId".into(), Value::String(String::new()));
            comment.insert(
                "refNickname".into(),
                Value::String(value_str(&map, &["to_nickname", "to_username"])),
            );
            let emojis = comment_emojis(&content);
            if !emojis.is_empty() {
                comment.insert("emojis".into(), Value::Array(emojis));
            }
            entry.0.push(Value::Object(comment));
        }
    }
    result
}

pub fn timeline(
    acc: &Account,
    limit: i32,
    offset: i32,
    username: &str,
    keyword: &str,
    start_time: i64,
    end_time: i64,
) -> Result<Vec<Value>, i32> {
    let conn = acc.db(&acc.sns_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, "SnsTimeLine") {
        return Ok(Vec::new());
    }
    let limit = limit.max(0) as usize;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let offset = offset.max(0) as usize;
    let mut conditions = Vec::new();
    if !username.trim().is_empty() {
        conditions.push(format!("user_name = {}", sql_string(username.trim())));
    }
    if start_time > 0 {
        conditions.push(format!("tid >= {}", tid_for_seconds(start_time)));
    }
    if end_time > 0 {
        conditions.push(format!("tid < {}", tid_for_seconds(end_time + 1)));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let mut rows: Vec<Value> = Vec::new();
    if keyword.trim().is_empty() {
        let sql = format!(
            "SELECT tid, user_name, content FROM SnsTimeLine {where_clause} \
             ORDER BY tid DESC LIMIT {limit} OFFSET {offset}"
        );
        rows = query_json(&guard, &sql, &[]).map_err(|_| ERR_SQL)?;
    } else {
        let needle = keyword.trim().to_lowercase();
        let mut skipped = 0usize;
        let mut page_offset = 0usize;
        'outer: loop {
            let sql = format!(
                "SELECT tid, user_name, content FROM SnsTimeLine {where_clause} \
                 ORDER BY tid DESC LIMIT 500 OFFSET {page_offset}"
            );
            let page = query_json(&guard, &sql, &[]).map_err(|_| ERR_SQL)?;
            if page.is_empty() {
                break;
            }
            let page_len = page.len();
            for row in page {
                let Value::Object(map) = &row else { continue };
                let content = decode_content(map.get("content").unwrap_or(&Value::Null));
                if content.to_lowercase().contains(&needle) {
                    if skipped < offset {
                        skipped += 1;
                        continue;
                    }
                    rows.push(row);
                    if rows.len() >= limit {
                        break 'outer;
                    }
                }
            }
            if page_len < 500 {
                break;
            }
            page_offset += page_len;
        }
    }

    let tids: Vec<i64> = rows
        .iter()
        .filter_map(|row| match row {
            Value::Object(map) => map
                .get("tid")
                .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))),
            _ => None,
        })
        .collect();
    let interactions = load_interactions(&guard, &tids);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let Value::Object(map) = row else { continue };
        let tid = map
            .get("tid")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0);
        let xml = decode_content(map.get("content").unwrap_or(&Value::Null));
        let column_user = value_str(&map, &["user_name"]);
        let nickname_hint = String::new();
        let mut post = parse_post(&xml, tid, &nickname_hint);
        if let Value::Object(obj) = &mut post {
            if value_str(obj, &["username"]).is_empty() {
                obj.insert("username".into(), Value::String(column_user));
            }
        }
        let (comments, likes) = interactions
            .get(&tid)
            .cloned()
            .unwrap_or_else(|| (Vec::new(), Vec::new()));
        if let Value::Object(obj) = &mut post {
            obj.insert("comments".into(), Value::Array(comments));
            obj.insert(
                "likes".into(),
                Value::Array(likes.into_iter().map(Value::String).collect()),
            );
        }
        out.push(post);
    }
    Ok(out)
}

pub fn usernames(acc: &Account) -> Result<Vec<Value>, i32> {
    let conn = acc.db(&acc.sns_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, "SnsTimeLine") {
        return Ok(Vec::new());
    }
    let rows = query_json(
        &guard,
        "SELECT DISTINCT user_name FROM SnsTimeLine WHERE user_name IS NOT NULL AND user_name <> ''",
        &[],
    )
    .map_err(|_| ERR_SQL)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match row {
            Value::Object(map) => {
                let name = value_str(&map, &["user_name"]);
                if name.is_empty() {
                    None
                } else {
                    Some(Value::String(name))
                }
            }
            _ => None,
        })
        .collect())
}

pub fn export_stats(acc: &Account, my_account_id: &str) -> Result<Value, i32> {
    let conn = acc.db(&acc.sns_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, "SnsTimeLine") {
        return Ok(json!({ "total_posts": 0, "total_friends": 0, "my_posts": 0 }));
    }
    let total = query_json(&guard, "SELECT count(*) AS c FROM SnsTimeLine", &[])
        .ok()
        .and_then(|rows| match rows.first() {
            Some(Value::Object(map)) => map.get("c").and_then(|v| v.as_i64()),
            _ => None,
        })
        .unwrap_or(0);
    let friends = query_json(
        &guard,
        "SELECT count(DISTINCT user_name) AS c FROM SnsTimeLine",
        &[],
    )
    .ok()
    .and_then(|rows| match rows.first() {
        Some(Value::Object(map)) => map.get("c").and_then(|v| v.as_i64()),
        _ => None,
    })
    .unwrap_or(0);
    let me = if my_account_id.trim().is_empty() {
        acc.my_account_id()
    } else {
        my_account_id.trim().to_string()
    };
    let mine = if me.is_empty() {
        0
    } else {
        query_json(
            &guard,
            &format!(
                "SELECT count(*) AS c FROM SnsTimeLine WHERE user_name = {}",
                sql_string(&me)
            ),
            &[],
        )
        .ok()
        .and_then(|rows| match rows.first() {
            Some(Value::Object(map)) => map.get("c").and_then(|v| v.as_i64()),
            _ => None,
        })
        .unwrap_or(0)
    };
    Ok(json!({
        "total_posts": total,
        "total_friends": friends,
        "my_posts": mine,
    }))
}

pub fn annual_stats(acc: &Account, begin: i64, end: i64) -> Result<Value, i32> {
    let conn = acc.db(&acc.sns_db())?;
    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    if !table_exists(&guard, "SnsTimeLine") {
        return Ok(json!({
            "totalPosts": 0,
            "typeCounts": {},
            "topLikers": [],
            "topLiked": [],
        }));
    }
    let mut conditions = Vec::new();
    if begin > 0 {
        conditions.push(format!("tid >= {}", tid_for_seconds(begin)));
    }
    if end > 0 {
        conditions.push(format!("tid < {}", tid_for_seconds(end + 1)));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!("SELECT tid, user_name, content FROM SnsTimeLine {where_clause}");
    let rows = query_json(&guard, &sql, &[]).map_err(|_| ERR_SQL)?;
    let mut type_counts: BTreeMap<String, i64> = BTreeMap::new();
    let mut author_of: HashMap<i64, String> = HashMap::new();
    let mut tids = Vec::new();
    for row in &rows {
        let Value::Object(map) = row else { continue };
        let tid = map
            .get("tid")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0);
        tids.push(tid);
        author_of.insert(tid, value_str(map, &["user_name"]));
        let xml = decode_content(map.get("content").unwrap_or(&Value::Null));
        let content_object = find_elements(&xml, "ContentObject")
            .into_iter()
            .next()
            .unwrap_or_default();
        let post_type = text_of(&content_object, "type");
        let key = if post_type.is_empty() {
            "0".to_string()
        } else {
            post_type
        };
        *type_counts.entry(key).or_insert(0) += 1;
    }
    let interactions = load_interactions(&guard, &tids);
    let mut liker_counts: BTreeMap<String, i64> = BTreeMap::new();
    let mut liked_counts: BTreeMap<String, i64> = BTreeMap::new();
    for (tid, (_, likes)) in &interactions {
        let author = author_of.get(tid).cloned().unwrap_or_default();
        for liker in likes {
            *liker_counts.entry(liker.clone()).or_insert(0) += 1;
            if !author.is_empty() {
                *liked_counts.entry(author.clone()).or_insert(0) += 1;
            }
        }
    }
    let top = |counts: BTreeMap<String, i64>| -> Vec<Value> {
        let mut items: Vec<(String, i64)> = counts.into_iter().collect();
        items.sort_by(|a, b| b.1.cmp(&a.1));
        items
            .into_iter()
            .take(10)
            .map(|(name, count)| json!({ "username": name, "count": count }))
            .collect()
    };
    Ok(json!({
        "totalPosts": rows.len(),
        "typeCounts": type_counts,
        "topLikers": top(liker_counts),
        "topLiked": top(liked_counts),
    }))
}
