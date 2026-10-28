# WeChat 4.x (Windows) local database schema and SQLCipher opening reference

Evidence collected read-only on 2026-10-28 from this machine. No WeChat database was decrypted during this pass. Runtime facts come from a working local toolchain that already opens these databases, from the offline documentation that toolchain was validated against, and from the WeFlow repository. The master database key is deliberately redacted everywhere in this document.

Cite convention: `file:line` always refers to the source map below. Line ranges are from the copies present on this machine on 2026-10-28.

Schema source note: none of the local sources contain literal `CREATE TABLE` DDL for WeChat's tables. The inventories below are the exact column names observed through `PRAGMA table_info` by `doc_validate.py`, plus the offline field tables that were validated against live databases. Inferred meanings are marked as inferred.

## Source map

| Short name | Absolute path |
|---|---|
| `SKILL.md` | `C:\Users\Administrator\.qoder-cn\skills\wxchat-archive\SKILL.md` |
| `config.json` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\config.json` |
| `keyutil.py` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\keyutil.py` |
| `local_dump.py` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\local_dump.py` |
| `media_local.py` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\media_local.py` |
| `doc_validate.py` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\doc_validate.py` |
| `dirmap.py` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\dirmap.py` |
| `scan_sessions.py` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\scan_sessions.py` |
| `sns_local.py` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\sns_local.py` |
| `wechat_cli.py` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\wechat_cli.py` |
| `_docs\*.md` | `C:\Users\Administrator\Documents\workbudd\态势分析\聊天记录分析\wechat_local\work\_docs\*.md` (offline mirror of doc.weflow.top, 21 articles) |
| `wcdbCore.ts` | `C:\Users\Administrator\Documents\开发\WeFlow\WeFlow_tree0\electron\services\wcdbCore.ts` |
| `chatService.ts` | `C:\Users\Administrator\Documents\开发\WeFlow\WeFlow_tree0\electron\services\chatService.ts` |
| `videoService.ts` | `C:\Users\Administrator\Documents\开发\WeFlow\WeFlow_tree0\electron\services\videoService.ts` |
| `groupAnalyticsService.ts` | `C:\Users\Administrator\Documents\开发\WeFlow\WeFlow_tree0\electron\services\groupAnalyticsService.ts` |
| `bizService.ts` | `C:\Users\Administrator\Documents\开发\WeFlow\WeFlow_tree0\electron\services\bizService.ts` |
| `probe.rs` / `lib.rs` | `C:\Users\Administrator\Documents\开发\WeFlow\WeFlow_tree0\native\wcdb-native\src\bin\probe.rs`, `...\src\lib.rs` |

---

## 1. On-disk layout (measured, not inferred)

### 1.1 Root and account directories

The data root is `D:\xwechat_files` on this machine. Several accounts with a `db_storage` directory exist; directory names follow the `wxid_xxxxxxxx_4hex` pattern, and the account used by the working toolchain is referred to below as `<account_dir>`.

The root also contains `Backup`, `old_backup`, and `all_users/{config,head_imgs,login,sqlite,whatsnew.dat}`. This matches the documented model of `xwechat_files/{account}/db_storage` plus a shared `all_users` tree (`dirmap.py:6-9`). The account directory name rule is `wxid_xxxxxxxx_4hex` or `custom-id_4hex`; the reliable test is the presence of a `db_storage` child (`dirmap.py:19-20`).

The current account directory contains `apm_record`, `business`, `cache`, `config`, `db_storage`, `msg`, `resource`, `temp` (directory listing, 2026-10-28).

### 1.2 `db_storage` inventory and sizes

Direct children observed: `MMKV`, `bizchat`, `chatbot`, `contact`, `emoticon`, `favorite`, `general`, `hardlink`, `head_image`, `message`, `migrate`, `session`, `sns`, `solitaire`, `third_app_icon`, plus `message_0.db`, `message_1.db`, `message_2.db` at the root.

Sizes below are allocated file sizes in bytes (WeChat preallocates; size is not row count). DB files generally have `-wal`/`-shm` siblings; the stub root shards are the exception (`-wal` is 0 bytes).

| Relative path | Bytes | Notes |
|---|---:|---|
| `session/session.db` | 1,077,248 | session index only |
| `contact/contact.db` | 8,044,544 | contacts, groups, official accounts |
| `contact/contact_fts.db` | 3,338,240 | contact search index |
| `message/message_0.db` | 220,200,960 | 210 MiB preallocated message shard |
| `message/message_1.db` | 104,857,600 | 100 MiB preallocated |
| `message/message_2.db` | 125,829,120 | 120 MiB preallocated |
| `message/media_0.db` | 167,772,160 | 160 MiB; `VoiceInfo` lives here |
| `message/media_1.db` | 115,343,360 | 110 MiB |
| `message/message_fts.db` | 94,371,840 | FTS5; `MMFtsTokenizer` |
| `message/message_resource.db` | 41,943,040 | not queried by any local source (see gaps) |
| `message/biz_message_0.db` | 199,229,440 | 190 MiB official-account messages; not queried |
| `message/weclaw.db` | 8,192 | present, not queried |
| `hardlink/hardlink.db` | 9,560,064 | image/video/file resolution |
| `head_image/head_image.db` | 21,823,488 | avatar blobs |
| `sns/sns.db` | 47,349,760 | moments timeline and interactions |
| `emoticon/emoticon.db` | 1,298,432 | emoticon metadata/captions |
| `favorite/favorite.db` | 389,120 | not queried |
| `general/general.db` | 3,473,408 | not queried |
| `bizchat/bizchat.db` | 28,672 | not queried |
| `migrate/unspportmsg.db` | 1,142,784 | migration leftovers |
| `solitaire/solitaire.db` | 53,248 | not queried |
| `chatbot/chatbot_message.db` | 143,360 | not queried |
| `third_app_icon/third_app_icon.db` | 12,288 | not queried |
| `message_0.db`, `message_1.db`, `message_2.db` (root) | 4,096 each | stub/decoy files; `-wal` is 0 bytes |

Two layout traps matter for a native reader:

1. The real message shards are `db_storage/message/message_*.db`; the 4 KiB files directly under `db_storage` are empty/stub databases with a valid 16-byte random salt but no data. The working toolchain only ever reads `DB_ROOT/message/<name>` (`local_dump.py:74-80`, `local_dump.py:139`), and its `DB_ROOT` is the `db_storage` path (`local_dump.py:29`).
2. `media_*.db` lives in `db_storage/message/`, not `db_storage/Media/` as the public documentation claims; this deviation is recorded in `dirmap.py:11-16` and confirmed by the listing above. Contact, emoticon, hardlink, head_image, sns, and session are lowercase directory names on this machine (`dirmap.py:12-13`).

### 1.3 Message media asset tree

Under the account directory:

```
msg/
  attach/<dir1>/<YYYY-MM>/Img/<file_name>.dat      # images, encrypted
  video/<YYYY-MM>/<file_name>                       # videos, plaintext + .jpg cover
  file/<YYYY-MM>/<original file name>               # files, plaintext
  migrate/                                          # config only; migration DB is db_storage/migrate/unspportmsg.db
cache/<YYYY-MM>/Emoticon/<md5 first 2 hex>/<md5>    # encrypted emoticon bodies
business/migrate/local/p/                           # migrated business documents
```

`msg/attach` has 406 peer directories named as `md5(peer_wxid)`; each contains `YYYY-MM` subdirectories and then `Img/*.dat`. `msg/video` and `msg/file` are flat `YYYY-MM` directories. The emoticon cache depth is exactly `<md5[:2]>/<md5>` (`dirmap.py:16-18`, `SKILL.md:61-63`).

---

## 2. Database key: provenance, form, and exact SQLCipher open sequence

### 2.1 What the key is

The database key is a 32-byte (64 lowercase hex) WCDB master key. It is not a user passphrase and it is not the image `.dat` key. The toolchain materializes it as `decrypt_key` in `config.json` and converts it with `bytes.fromhex(...)` (`local_dump.py:29-30`). A hardcoded fallback copy of the same 64-hex value exists at `keyutil.py:22` (value redacted here). `keyutil.py:2` labels the format "WeChat 4.0 WCDB (SQLCipher4-compatible)".

Each database is encrypted independently. The first 16 bytes of every DB file are that database's random salt; a 2026-10-28 hexdump of `message/message_0.db` starts `23 71 6f ed 02 38 f6 02 a5 c2 d3 11 85 d8 98 d9` and `session/session.db` starts `71 bc 66 68 ...`, i.e. neither file begins with `SQLite format 3\0`. `salt_of()` simply reads the first 16 bytes (`keyutil.py:35-37`, `local_dump.py:61`).

### 2.2 How the key is obtained today

The end-to-end chain documented by the skill (`SKILL.md:48`):

1. Read `os_crypt.encrypted_key` (base64) from the Chromium-style `Local State` file.
2. Strip the `DPAPI` prefix from the decoded bytes.
3. Call Windows `CryptUnprotectData` with `entropy=None`; this yields an AES-256-GCM key.
4. Use that key to decrypt a `safe:` value stored in `config.json`; the encoding is `v10` with `iv` = first 12 bytes and the body starting at offset 3 (skip `b"v10"`).
5. The plaintext result is the 64-hex master key.

If WeChat's application lock is enabled, the stored value is `lock:` (AES-GCM + PBKDF2 passphrase) instead of `safe:`. The skill states the lock must be turned off in WeChat once so the value is re-encrypted as `safe:` (`SKILL.md:48`).

Verified on this machine (read-only): `C:\Users\Administrator\AppData\Roaming\Tencent\xwechat\radium\web\Local State` exists (625 bytes, mtime 2025-08-16) and contains an `os_crypt` object with `"encrypted_key"` beginning with base64 `RFBBUEk` (`DPAPI`). The `%APPDATA%\Tencent\xwechat\config\51a1fffea11325a1e4104c6b3de47af7.ini` file is 3 bytes and is not the key source.

What is not present on this machine: the toolchain does not contain code that performs steps 1-4. `keyutil.py` only validates a key that is already known; it reads no `Local State` and calls no DPAPI. The `safe:`/`v10` blob location referenced by `SKILL.md:48` was not found in the WeChat config files inspected (`all_users/config/*`, `%APPDATA%\Tencent\xwechat\config\*`), so that step needs empirical re-verification (see gaps).

Static alternative (location only, not implemented as a key dumper): `wcdb_hook.py` parses the PE, anchors on the string `com.tencent.WCDB.Config.Cipher`, follows `lea rdx`/`lea rcx` references, and computes a hook RVA; the skill records RVA `0x5defb0` for WeChat 4.1.12.55 (`wcdb_hook.py:39`, `SKILL.md:78`). This finds the cipher-config function, not the key bytes.

### 2.3 Key form passed to SQLCipher

SQLCipher is given a raw 48-byte key, not the 32-byte master key and not a passphrase:

```
raw_key_blob = PBKDF2-HMAC-SHA512(password = master_key_32_bytes,
                                  salt     = db_file_first_16_bytes,
                                  iterations = 256000,
                                  dklen    = 32) || salt
PRAGMA key = "x'<raw_key_blob as 96 hex chars>'";
```

That is 32 derived bytes + 16 salt bytes = 48 bytes = 96 hex characters inside `x'...'` (`SKILL.md:49`, `local_dump.py:61-69`, `keyutil.py:72-74`). Because the derivation is done outside SQLCipher, `kdf_iter` is set to `2`, not 256000.

`keyutil.py:41-50` is the empirical trial matrix. It lists the alternatives that were tried: raw key with `kdf_iter=2` and HMAC on, raw key with `kdf_iter=2` and `hmac_size=0`, external PBKDF2-SHA512 at 256000, external PBKDF2-SHA256 at 256000, external PBKDF2-SHA512 at 64008, and passphrase modes at 256000/64008/4000 iterations. The shipped, working path is the external SHA-512/256000 derivation (`SKILL.md:49`, `local_dump.py:60-69`); the others are rejected alternatives, not parameters to try at runtime.

### 2.4 Exact open sequence

The working Python sequence, in order (`local_dump.py:60-71`; identical core in `keyutil.py:58-78`):

```python
salt = open(path, "rb").read(16)
derived = hashlib.pbkdf2_hmac("sha512", MASTER, salt, 256000, 32)
conn = sqlcipher3.connect(path)
cur = conn.cursor()
cur.execute("PRAGMA cipher_page_size = 4096;")
cur.execute("PRAGMA cipher_hmac_algorithm = HMAC_SHA512;")
cur.execute("PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;")
cur.execute("PRAGMA kdf_iter = 2;")
cur.execute("PRAGMA key = \"x'%s%s'\";" % (derived.hex(), salt.hex()))
cur.execute("SELECT count(*) FROM sqlite_master;")   # activation; must succeed
```

There is no `PRAGMA cipher_compatibility` in the working path. The algorithm family is SQLCipher 4 (4096-byte pages, HMAC-SHA512, KDF-SHA512), but the key is pre-derived outside SQLCipher and `kdf_iter` is pinned to 2; the code does not rely on a compatibility level (`local_dump.py:65-69`, `keyutil.py:58-74`; a search of the toolchain finds no `cipher_compatibility`).

After activation, `keyutil.py` optionally runs `PRAGMA cipher_integrity_check = 100;` as a 100-page integrity smoke test (`keyutil.py:82-85`). This is not a full-database verification.

### 2.5 Verified SQLCipher parameters

| Parameter | Value used | Evidence |
|---|---|---|
| `cipher_page_size` | 4096 | `local_dump.py:65`, `keyutil.py:58`, `config.json` kdf block |
| `kdf_iter` | 2 | `local_dump.py:68`, `keyutil.py:73` |
| External KDF | PBKDF2-HMAC-SHA512, 256000 iterations, 32-byte output | `local_dump.py:62`, `SKILL.md:49` |
| `cipher_kdf_algorithm` | `PBKDF2_HMAC_SHA512` | `local_dump.py:67`, `keyutil.py:60` |
| `cipher_hmac_algorithm` | `HMAC_SHA512` | `local_dump.py:66`, `keyutil.py:59` |
| Key input | raw 48-byte key blob (`derived || salt`) as `x'...'` | `local_dump.py:69`, `SKILL.md:49` |
| HMAC | ON (SQLCipher default; `hmac_size` not set) | no `hmac_size` in winning path; `hmac_size=0` exists only as a rejected trial (`keyutil.py:43,61-62`) |
| Plaintext header size | 0; bytes 0-15 are salt, there is no plaintext SQLite header | `keyutil.py:35-37`, `local_dump.py:61`, hexdump above; `cipher_plaintext_header_size` never set |
| Compatibility level | not set | no such pragma in `local_dump.py:60-71` or `keyutil.py:53-78` |
| WAL | ON in production; copy `.db`, `-wal`, `-shm` together before reading | `local_dump.py:74-80` |

The `-wal` files are up to 4 MiB and `-shm` 32 KiB in `message/`. Copying only the `.db` can miss recent commits.

Warning for the native implementation: the in-repo Rust probe at `native/wcdb-native/src/bin/probe.rs:7-13` opens a *synthetic* database with a 32-byte `PRAGMA key = "x'<64 hex>'"` and `PRAGMA cipher_compatibility = 4`. That is a self-test of the bundled SQLCipher, not the WeChat scheme. It will not open WeChat databases. Do not copy those two lines into the real reader.

### 2.6 Key-file summary

| Item | Value |
|---|---|
| Key kind | 32-byte WCDB master key, 64 hex |
| Materialized at | `config.json` field `decrypt_key`; hardcoded fallback `keyutil.py:22` (redacted) |
| Per-DB salt | first 16 bytes of each DB file |
| Per-DB SQLCipher key | 48 bytes: `PBKDF2-HMAC-SHA512(master, salt, 256000)[32]` plus `salt[16]` |
| Image `.dat` key | separate; see section 7.6 |
| Key rotation on reinstall | rerun `keyutil.py` with a newly extracted master key (`SKILL.md:48`, `keyutil.py:2-4`) |

---

## 3. `message_*.db`: shards, `Msg_<md5>`, `Name2Id`, auxiliary tables

### 3.1 Shards and table naming

One table per peer conversation, named by MD5 of the peer's wxid string:

```python
def msg_table(peer_wxid):
    return "Msg_" + hashlib.md5(peer_wxid.encode()).hexdigest()
```

`local_dump.py:83-84`. The table name is always `Msg_` + 32 lowercase hex characters, with no truncation. The same table name can exist in more than one shard; the working dumper explicitly loops over all shards (`local_dump.py:139`) and concatenates the rows, because one conversation may be split across databases (`SKILL.md:50`).

The public schema documentation states the same rule: `Msg_*` table name is the MD5 of the wxid or chatroom id, and a conversation may be spread across every `message_*.db` while retaining the same table name (`_docs\wechat-db-message.md:17-19`).

### 3.2 `Msg_*` columns

Exact column names and their roles (`_docs\wechat-db-message.md:23-39`):

| Column | Type/role | Meaning and use |
|---|---|---|
| `local_id` | INTEGER PK | Per-conversation monotonic message id; not globally unique across shards or peers |
| `server_id` | INTEGER | Server message id; may exceed 2^53, so keep as 64-bit/string in JS (`chatService.ts:6326`) |
| `local_type` | INTEGER | Message kind; see section 11 |
| `sort_seq` | INTEGER | Global sort sequence in milliseconds; use for cross-shard ordering |
| `real_sender_id` | INTEGER | Rowid into `Name2Id`; resolves actual sender in group chats |
| `create_time` | INTEGER | Unix seconds |
| `status` | INTEGER | Message lifecycle state; enum not decoded (gap) |
| `upload_status` | INTEGER | Upload state; not used by the working dumper |
| `download_status` | INTEGER | Download state; not used |
| `server_seq` | INTEGER | Server sync sequence; not used |
| `origin_source` | INTEGER | Source bitfield; not used |
| `source` | TEXT | Source context text |
| `message_content` | TEXT | Primary content; may be a BLOB at runtime despite the declared TEXT affinity |
| `compress_content` | TEXT | Compressed/rich payload; WCDB may place the real content here |
| `packed_info_data` | BLOB | Internal extension blob; only WeFlow uses it, for video filenames (`chatService.ts:6550`) |
| `WCDB_CT_message_content` | INTEGER, nullable | WCDB column type tag for `message_content` (0=text, 4=blob under current observations) |
| `WCDB_CT_source` | INTEGER, nullable | WCDB column type tag for `source` |

The working dumper's exact projection (`local_dump.py:145-147`):

```sql
SELECT local_id, server_id, local_type, sort_seq, real_sender_id, create_time,
       status, message_content, compress_content, WCDB_CT_message_content FROM <Msg_md5>
```

No `WHERE` and no `ORDER BY`; the caller sorts afterwards by `(create_time, local_id)` (`local_dump.py:167`). Table existence is checked per shard with `SELECT count(*) FROM sqlite_master WHERE name=?` (`local_dump.py:143`). To enumerate conversations, the tool lists tables and fetches all names before iterating, to avoid reusing a cursor while iterating (`local_dump.py:188-204`):

```sql
SELECT name FROM sqlite_master WHERE type='table' AND name GLOB 'Msg_*'
```

Do not hardcode three shards in the native reader. The Python tool does (`local_dump.py:139`), but that is a deployment-specific assumption; WeFlow discovers database paths dynamically (`chatService.ts:8099-8133`).

### 3.3 `Name2Id` and `isSend`

`Name2Id` maps integer rowids to wxid strings (`_docs\wechat-db-message.md:42-47`):

| Column | Role |
|---|---|
| `user_name` | TEXT primary key; wxid / chatroom id / official account id |
| `is_session` | INTEGER; distinguishes conversation keys from ordinary user keys |

Working resolution (`local_dump.py:142`, `local_dump.py:159-160`):

```python
n2i = {r[0]: r[1] for r in cur.execute("SELECT rowid, user_name FROM Name2Id")}
isSend = 1 if n2i.get(real_sender_id) == MY_WXID else 0
senderUsername = n2i.get(real_sender_id)
```

For group messages, `real_sender_id` points at the member, not the chatroom; this is how per-message senders are recovered. `MY_WXID` comes from `config.json` `self_wxid` (`local_dump.py:31`).

### 3.4 Auxiliary tables in message shards

The offline schema documents these tables; the validator opens the shard and asserts the required columns and that the table can be counted (`doc_validate.py:69-93`). The dumper does not read them, but a complete reader should know they exist:

| Table | Columns | Purpose |
|---|---|---|
| `TimeStamp` | `timestamp` | Message-database time cursor |
| `SendInfo` | `chat_name_id`, `msg_local_id` | Send bookkeeping |
| `DeleteInfo` | `chat_name_id`, `delete_table_name` | Deleted conversation/table marker |
| `DeleteResInfo` | `local_id`, `session_name_id`, `msg_create_time`, `msg_local_id`, `res_path` | Deleted resource record and path |
| `HistoryAddMsgInfo` | `session_name_id`, `history_id`, `server_id`, `is_revoke` | Historical message batch/revoke flag |
| `HistorySysMsgInfo` | `session_name_id`, `history_id`, `server_id`, `is_revoke` | Historical system-message batch |
| `wcdb_builtin_compression_record` | `tableName`, `columns`, `rowid` | WCDB built-in compression registry |

Sources: `_docs\wechat-db-message.md:50-108`; required-column sets at `doc_validate.py:73-79`.

### 3.5 `sort_seq`, `create_time`, and local id semantics

- `create_time` is seconds, not milliseconds (`local_dump.py:10` header comment, `local_dump.py:157`).
- `sort_seq` is the millisecond sequence; if absent or non-positive, both docs and WeFlow fall back to `create_time * 1000` (`_docs\wechat-db-message.md:138-139`, `chatService.ts:6323`).
- `local_id` is per conversation and per database lineage. WeFlow's promote step renumbers `localId` when merging sources and stores the original as `_dbLocalId` (`wechat_cli.py:220-231`). A native reader should expose the raw DB `local_id` and not assume it is globally unique.

---

## 4. `session/session.db`

### 4.1 `SessionTable`

Columns (`_docs\wechat-db-session.md:21-38`):

| Column | Role |
|---|---|
| `username` | TEXT PK; wxid, `xxx@chatroom`, or `gh_xxx` |
| `type` | Conversation type enum (not decoded) |
| `unread_count` | Unread count |
| `unread_first_msg_srv_id` | Server id of first unread message |
| `is_hidden` | Hidden flag |
| `summary` | Last-message preview text |
| `draft` | Draft text |
| `status` | Status bitfield (pin etc.) |
| `last_timestamp` | Last message time |
| `sort_timestamp` | Primary list ordering key (descending) |
| `last_clear_unread_timestamp` | Last clear-unread time |
| `last_msg_locald_id` | Local id of last message; note the exact spelling `locald_id` |
| `last_msg_type` | Last message main type |
| `last_msg_sub_type` | Last message subtype |
| `last_msg_sender` | Last sender identifier |
| `last_sender_display_name` | Cached display name |
| `last_msg_ext_type` | Extended type |
| `unread_first_pat_msg_local_id` | First unread pat local id |
| `unread_first_pat_msg_sort_seq` | First unread pat sort sequence |

The type values documented for `last_msg_type` are the same base taxonomy as messages: 1 text, 3 image, 34 voice, 43 video, 47 emoji, 49 appmsg, 50 call, 10000 system (`_docs\wechat-db-session.md:104-111`).

### 4.2 Other session tables

| Table | Columns | Source |
|---|---|---|
| `Name2Id` | `user_name` | `_docs\wechat-db-session.md:42-47` |
| `SessionUnreadListTable_1` | `username_id`, `server_id`, `create_time` | `_docs\wechat-db-session.md:49-55` |
| `SessionUnreadStatTable_1` | `username_id`, `unread_stat` | `_docs\wechat-db-session.md:58-63` |
| `SessionDeleteTable` | `username`, `delete_time` | `_docs\wechat-db-session.md:66-72` |
| `SessionNoContactInfoTable` | `username`, `session_title` | `_docs\wechat-db-session.md:75-80` |

### 4.3 Queries actually used

The validator's exact checks (`doc_validate.py:96-112`):

```sql
SELECT count(*) FROM SessionTable
SELECT username,type,last_msg_type,unread_count FROM SessionTable WHERE username=?
SELECT count(*) FROM SessionTable WHERE username LIKE '%@chatroom'
SELECT count(*) FROM SessionTable WHERE username LIKE 'gh_%'
```

`session.db` does not store message bodies. Conversation message counts come from the per-peer `Msg_*` tables across shards, not from session rows.

---

## 5. `contact/contact.db` and group semantics

### 5.1 `contact` and `stranger`

`contact` columns (`_docs\wechat-db-contact.md:18-43`):

`id`, `username`, `local_type`, `alias`, `encrypt_username`, `flag`, `delete_flag`, `verify_flag`, `remark`, `remark_quan_pin`, `remark_pin_yin_initial`, `nick_name`, `pin_yin_initial`, `quan_pin`, `big_head_url`, `small_head_url`, `head_img_md5`, `chat_room_notify`, `is_in_chat_room`, `description`, `extra_buffer`, `chat_room_type`.

`stranger` mirrors the same shape (`_docs\wechat-db-contact.md:46-70`).

Display-name semantics, as documented: `remark` non-empty wins; when it is empty, `nick_name` is the fallback; `alias` is the WeChat id, not a display-name priority (`_docs\wechat-db-contact.md:256-259`). The working scanner only exposes the raw fields and leaves priority to the caller (`scan_sessions.py:44-47`):

```python
pick = [c for c in ("username", "nick_name", "remark", "alias", "local_type") if c in cols]
for row in cur.execute("SELECT %s FROM contact" % ",".join(pick)):
```

WeFlow queries contacts with `SELECT *` and resolves the flag across possible column spellings: `row.flag ?? row.contact_flag ?? row.contactFlag ?? row.WCDB_CT_flag` (`wcdbCore.ts:759-761`, `wcdbCore.ts:781`). Its path resolver looks under `db_storage/Contact` for `contact.db` or `Contact.db` (`wcdbCore.ts:702-721`); on Windows the case difference is harmless.

### 5.2 Group tables

| Table | Columns | Notes |
|---|---|---|
| `chat_room` | `id` INTEGER PK, `username` TEXT, `owner` TEXT, `ext_buffer` BLOB | `_docs\wechat-db-contact.md:73-80` |
| `chatroom_member` | `room_id` INTEGER, `member_id` INTEGER | Both are rowids into `name2id`; unique indexes (`_docs\wechat-db-contact.md:83-89`) |
| `name2id` | `username` TEXT PK | `_docs\wechat-db-contact.md:91-96` |
| `encrypt_name2id` | `username` | Encrypted-name mapping (`_docs\wechat-db-contact.md:98-103`) |
| `contact_label` | `label_id_`, `label_name_`, `sort_order_` | `_docs\wechat-db-contact.md:105-110` |
| `biz_info` | see 5.4 | `_docs\wechat-db-contact.md:130-148` |

No local source contains a `chatroom_member` SQL query. The schema implies the canonical join:

```sql
SELECT n.username
FROM chatroom_member m
JOIN name2id n ON n.rowid = m.member_id
JOIN chat_room r ON r.id = m.room_id
WHERE r.username = ?
```

This join is inferred from the foreign-key semantics in `_docs\wechat-db-contact.md:87-95`; it was not executed by the working scripts. WeFlow delegates group member enumeration to the native DLL (`wcdbCore.ts:1037-1038`, `groupAnalyticsService.ts:1285-1291`).

### 5.3 `ext_buffer` encoding

`contact.extra_buffer` and `chat_room.ext_buffer` are protobuf blobs. Only one field is documented: protobuf field 12, when non-zero, means the conversation is muted (`_docs\wechat-db-contact.md:268-269`). The group member list and owner encoding in `chat_room.ext_buffer` are not decoded anywhere in the Python toolchain. WeFlow calls `wcdb_get_chat_room_ext_buffer` and then tries to resolve an owner by generic traversal of the returned member JSON (`groupAnalyticsService.ts:241-243`, `groupAnalyticsService.ts:470-479`), but the DLL is not in this repository, so the wire format remains unverified. Treat this as a gap.

`groupAnalyticsService.ts:1128` and `:1285` show the intended member API shape (`getGroupMembers`), again implemented outside the repo.

### 5.4 Official-account metadata (`biz_info`)

Columns (`_docs\wechat-db-contact.md:130-148`): `id`, `username`, `type`, `accept_type`, `child_type`, `version`, `external_info`, `brand_info`, `brand_icon_url`, `brand_list`, `brand_flag`, `belong`, `ext_buffer`, `home_url`, `sync_version`.

WeFlow uses only `username` and `type`:

```sql
SELECT username, type FROM biz_info
```

`bizService.ts:203`; type mapping is 0=subscription, 1=service, 2/3=enterprise (`chatService.ts:4214-4219`).

`biz_profile` adds `service_type`, `article_count`, `friend_sub_count`, `is_subscribe`, `offset`, `time_stamp`, `is_end`, `resp_buffer` (`_docs\wechat-db-contact.md:151-164`), but no local reader queries it.

---

## 6. `media_*.db` and `VoiceInfo` (voice messages)

### 6.1 Location and schema

Voice payloads live in `db_storage/message/media_0.db` and `media_1.db`, table `VoiceInfo` (`SKILL.md:56`, `doc_validate.py:190-200`). Columns (`_docs\wechat-db-media.md:21-26`; required set at `doc_validate.py:197`):

| Column | Role |
|---|---|
| `chat_name_id` | INTEGER; rowid of the peer in `Name2Id` |
| `create_time` | Unix seconds |
| `local_id` | Voice-item sequence inside the conversation (not the message `local_id`) |
| `svr_id` | Server id of the voice message; may be 0 for messages sent by the local account |
| `voice_data` | BLOB; framed SILK payload |
| `data_index` | TEXT default `'0'`; shard index for multi-part payloads |

The same DB has `Name2Id` (`user_name`) and `TimeStamp` (`timestamp`) (`_docs\wechat-db-media.md:29-42`).

### 6.2 Lookup algorithm actually used

Exact SQL, in order (`media_local.py:269-301`):

```sql
SELECT rowid FROM Name2Id WHERE user_name=?
SELECT rowid, svr_id, data_index, voice_data FROM VoiceInfo
 WHERE chat_name_id=? AND svr_id IN (?, ...)
-- fallback when svr_id misses (sent messages have svr_id=0):
SELECT rowid, local_id, create_time FROM VoiceInfo
 WHERE chat_name_id=? AND create_time BETWEEN ? AND ?
 ORDER BY ABS(create_time-?) LIMIT 1
SELECT rowid, data_index, voice_data FROM VoiceInfo
 WHERE chat_name_id=? AND local_id=?
```

Rules and pitfalls:

- `chat_name_id` is the `Name2Id` rowid of the *peer* wxid, not the account directory name (`media_local.py:269`, `SKILL.md:56`).
- Prefer `svr_id` exact match. It is reliable for inbound (`isSend=0`) voice.
- For messages sent by the local account, `VoiceInfo.svr_id` is `0` at write time; `svr_id` matching misses them. The fallback is `chat_name_id + create_time` within plus/minus 5 seconds, ordered by absolute time delta (`media_local.py:279-303`, `SKILL.md:56`).
- Once one row of a voice is found, the full group is fetched by the voice item's own `local_id`, and shards are merged by `data_index` ascending (`'0'` first) (`media_local.py:293-315`).
- A `(db, rowid)` claimed set prevents cross-shard and cross-message double claims (`media_local.py:266`, `:277`).
- Multiple media DBs are scanned via `media_*.db` glob (`media_local.py:267`); WeFlow additionally introspects `VoiceInfo%` and `Name2Id%` table names to tolerate version suffixes (`chatService.ts:10517-10525`).

### 6.3 SILK payload and WAV

`voice_data` is a framed SILK stream: `\x02#!SILK_V3` followed by chunks, each prefixed by a 2-byte little-endian length (`SKILL.md:56`). The working decoder writes the blob to a temp file and calls `pysilk.decode(fileobj_in, fileobj_out, 16000)` to get raw PCM, then writes a 16 kHz mono 16-bit WAV (`media_local.py:312-326`, `SKILL.md:59`). The required `pysilk` is the real package with `_silk.pyd`, not the empty PyPI stub (`SKILL.md:59`).

Media coverage is incomplete by nature: if the row is absent or `voice_data` is empty, the exporter records `not_in_db` (`media_local.py:310`).

---

## 7. `hardlink/hardlink.db` and media resolution

### 7.1 Schema

Three resolution tables share the same shape (`_docs\wechat-db-hardlink.md:17-63`):

`md5_hash` INTEGER, `md5` TEXT, `type` INTEGER, `file_name` TEXT, `file_size` INTEGER, `modify_time` INTEGER, `dir1` INTEGER, `dir2` INTEGER, `_rowid_` INTEGER PK, `extra_buffer` BLOB.

Table names: `image_hardlink_info_v4`, `video_hardlink_info_v4`, `file_hardlink_info_v4` (`doc_validate.py:203-215`). `dir2id` maps a rowid to a path segment string: `username` TEXT PK (`_docs\wechat-db-hardlink.md:65-70`). `md5_hash` is an integer hash for indexing; all working queries use `md5` with `COLLATE NOCASE` (`media_local.py:519-522`).

`dir1`/`dir2` are rowids, not literal directory names. Translate through `dir2id`:

```sql
SELECT username FROM dir2id WHERE rowid=?
```

`media_local.py:555-556`, `media_local.py:615-617`. If lookup fails, the code falls back to the numeric string.

### 7.2 Images

Working query (`media_local.py:519-522`):

```sql
SELECT file_name, dir1, dir2, file_size FROM image_hardlink_info_v4
 WHERE md5=? COLLATE NOCASE
```

Path layout: `{account}/msg/attach/<dir1>/<dir2>/Img/<file_name>.dat`, where `dir1 = md5(peer_wxid)` for images and `dir2` resolves to `YYYY-MM` (`media_local.py:548`, `media_local.py:557-558`, `SKILL.md:61`; the video case below reverses the meaning of `dir1`, `SKILL.md:62`).

If the row is missing, the exporter tries a glob under the peer's md5 directory and then a global glob (`media_local.py:559-563`). A missing row or file is reported as `dat_not_local`, meaning the media was never downloaded to this machine (`media_local.py:565-567`, `SKILL.md:61`).

Image messages carry the per-file key in XML: `<img md5="..." aeskey="...">` (`media_local.py:535-537`). Attribute names are often written with spaces around `=`, so the parser must tolerate them (`media_local.py:201-207`; this was a real bug fixed on 2026-09-09, `SKILL.md:63`).

### 7.3 Videos

Working query (`media_local.py:653-654`):

```sql
SELECT file_name, dir1, dir2, file_size FROM video_hardlink_info_v4
 WHERE md5=? COLLATE NOCASE
```

Important differences from images (`SKILL.md:62`, `media_local.py:640-658`):

- The XML tag is `<videomsg ...>`, not `<video>`; attributes include `md5`, `newmd5`, `aeskey`, `playlength`, `cdnvideourl`.
- `video_hardlink_info_v4.md5` stores the XML `md5` attribute, not `newmd5` (the code tries `md5` first, then `newmd5` as a fallback).
- Directory semantics are reversed: `dir1` is the `YYYY-MM` segment (resolved through `dir2id`), `dir2` is unused (0).
- Physical path is `{account}/msg/video/<YYYY-MM>/<file_name>`, and the video is **plaintext**; no decryption is needed. A same-named `.jpg` cover may exist.
- If no hardlink row exists, the code globs `msg/video/*/<md5>.*`; otherwise it reports `video_not_local`.

### 7.4 Files

Working query (`media_local.py:863-864`):

```sql
SELECT file_name, dir1, file_size FROM file_hardlink_info_v4
 WHERE md5=? COLLATE NOCASE
```

`dir1` is the `YYYY-MM` segment; the physical path is `{account}/msg/file/<YYYY-MM>/<file_name>` and the file is **plaintext**, retaining its original name (including Chinese characters and spaces) (`media_local.py:865-871`, `SKILL.md:63`). The MD5 comes from `<md5>` in the appmsg XML, with `<emoticonmd5>` as fallback (`media_local.py:852-857`).

### 7.5 `.dat` container formats (V1/V2/V3)

Images stored as `.dat` are encrypted containers. The decoder inspects the first 6 bytes (`media_local.py:494-515`):

- `07 08 56 31 08 07` = V1; the AES key is the fixed 16 bytes `cfcd208495d565ef`.
- `07 08 56 32 08 07` = V2; the AES key is the per-file/account AES key.
- Anything else = V3; the whole file is XORed with a one-byte key.

V1/V2 layout (`media_local.py:506-515`):

```
offset 0..5   magic
offset 6..9   aes_size  int32 little-endian
offset 10..13 xor_size  int32 little-endian
offset 14     unknown/header byte
offset 15..   payload
```

The payload has three parts: an AES-128-ECB segment of `aes_size` bytes, raw middle bytes, and a trailing XOR segment of `xor_size` bytes. The AES segment is aligned with PKCS#7 rules where a size that is already a multiple of 16 still gets a full 16-byte pad: `aligned = aes_size + 16 - (aes_size % 16)` (`media_local.py:509`). The code decrypts that segment, strictly unpads it, copies the middle verbatim, and XORs the tail with `xorKey` (`media_local.py:510-515`). V1/V2 without a key raise an error; V3 needs only `xorKey`.

Key selection for images (`media_local.py:569-585`): per-file XML `aeskey` first 16 characters, then the account-level AES key, then `None` (V3 XOR-only). A successful result must start with a known image magic or `wxgf`.

### 7.6 Image key derivation (`kvcomm`)

The account-level image key is derived without memory scanning (`SKILL.md:64`, `media_local.py:340-451`):

1. Enumerate codes from `%APPDATA%\Tencent\xwechat\net\kvcomm\key_*.statistic` file names; take integers in `[1, 4294967295]` (`media_local.py:351-360`).
2. Normalize the wxid: for `wxid_xxx_yyyy`, keep `wxid_xxx` (strip the 4-hex account suffix); leave custom ids unchanged (`media_local.py:340-348`).
3. `xorKey = code & 0xFF`; `aesKey = MD5(str(code) + cleaned_wxid)[:16]` (`media_local.py:366-369`).
4. Verify against a V2 sample: read bytes `0x0F..0x1F` (16 bytes), AES-128-ECB decrypt with the candidate key, and require a JPG/PNG/GIF/RIFF/wxgf magic (`media_local.py:402-415`, `_docs\wechat-key-acquisition-main.md:41-52`).
5. Cache the verified `{code, aeskey, xorkey}` in `datkey.json`; if verification fails, fall back to estimating `xorKey` from `.dat` tail bytes (JPEG EOI) and mark `xor_only` (`media_local.py:418-451`, `media_local.py:372-399`).

### 7.7 `wxgf` (HEVC) wrapper

A decrypted image beginning with `wxgf` (`77 78 67 66`) is not necessarily a failure. The code searches the first 8 KiB for an embedded JPEG/PNG header and extracts it; if none is found, it extracts the HEVC NAL stream (start code `00 00 00 01`) and converts with ffmpeg (`media_local.py:473-491`, `SKILL.md:64`). Animated emoticons may need `ffmpeg` palette generation to become a usable GIF (`media_local.py:761-795`).

### 7.8 Emoticon entities and keys

Chat emoticon bodies are not in `emoticon.db`; they are in the account-level cache `cache/<YYYY-MM>/Emoticon/<md5[:2]>/<md5>` (`media_local.py:684-691`, `doc_validate.py:186`, `SKILL.md:63`).

Decryption (`media_local.py:702-725`):

```
key = bytes.fromhex(MD5(f"{seed}{clean_wxid(wxid)}EMOTICON").hexdigest())[:16]
AES-128-CBC(key, iv=key), PKCS#7 unpad
```

`seed` is the same `code` used for image keys, cached in `datkey.json`; no memory scan is needed when that cache exists (`media_local.py:702-712`, `SKILL.md:63`). Decrypted magic determines `png`, `gif`, `jpg`, `webp`, or `wxgf` (`media_local.py:698-699`, `media_local.py:728-734`).

Emoticon captions are also available without decrypting anything: the `<emoji desc="...">` XML attribute is base64 of a protobuf whose field 2 (`\x12 <len> <utf8>`) carries the official caption. `emoji_caption()` collects printable UTF-8 candidates, prefers CJK, and returns the longest (`media_local.py:210-240`, `SKILL.md:65`). Caption coverage is partial (shop emoticons, not custom ones).

---

## 8. `emoticon/emoticon.db`

This DB is metadata/caption lookup, not the entity store. Queried tables (`doc_validate.py:170-187`):

`kNonStoreEmoticonTable` columns (`_docs\wechat-db-emoticon.md:21-32`): `type`, `md5`, `caption`, `product_id`, `aes_key`, `thumb_url`, `tp_url`, `auth_key`, `cdn_url`, `extern_url`, `extern_md5`, `encrypt_url`.

`kStoreEmoticonCaptionsTable` columns (`_docs\wechat-db-emoticon.md:39-43`): `package_id_`, `md5_`, `language_`, `caption_`.

Exact queries used (`doc_validate.py:172-181`):

```sql
SELECT count(*) FROM kNonStoreEmoticonTable
SELECT count(*) FROM kNonStoreEmoticonTable WHERE cdn_url!='' AND cdn_url IS NOT NULL
SELECT count(*) FROM kNonStoreEmoticonTable WHERE caption IS NOT NULL AND caption!=''
SELECT md5_,caption_ FROM kStoreEmoticonCaptionsTable WHERE caption_!='' LIMIT 1
```

The DB also has `kStoreEmoticonFilesTable` and `kStoreEmoticonPackageTable` (`_docs\wechat-db-emoticon.md:45-70`), but no local reader queries them. `cdn_url`/`caption` form the fallback path for emoticons whose entity is missing locally; the entity cache path remains the primary route (`doc_validate.py:184-187`).

---

## 9. `sns/sns.db`

### 9.1 `SnsTimeLine`

Columns (`_docs\wechat-db-sns.md:17-24`): `tid` INTEGER PK, `user_name` TEXT, `content` TEXT XML, `pack_info_buf` TEXT.

`tid` is a signed 64-bit snowflake-style key. Interpret it as unsigned, shift right 23 bits to get milliseconds, then divide by 1000 (`sns_local.py:44-47`, `_docs\wechat-db-sns.md:173-174`):

```python
u = tid & 0xFFFFFFFFFFFFFFFF
seconds = (u >> 23) / 1000.0
```

The timeline `content` is XML rooted at `<TimelineObject>`. Fields parsed: `id`, `username`, `createTime`, `contentDesc`, nested `<media>` elements with `type`, `url`, `thumb`, `md5`, `token`, `key`, `enc_idx`, and a nested `<liveMedia>` for Live Photos (`sns_local.py:70-97`, `sns_local.py:106-132`). `norm_url()` upgrades `http` to `https`, rewrites `/150` to `/0`, and appends `token`/`idx=1` when needed (`sns_local.py:58-66`).

### 9.2 `SnsMessage_tmp3` (likes and comments)

Columns (`_docs\wechat-db-sns.md:46-64`): `local_id`, `create_time`, `type`, `feed_id`, `is_unread`, `from_username`, `from_nickname`, `to_username`, `to_nickname`, `content`, `serialized_comment`, `serialized_ref`, `comment_id`, `client_id`, `comment64_id`, `comment_flag`, `serialized_comment_buf`, `serialized_ref_buf`, `del_status`, `is_relative_me`.

`type=1` is a like, `type=2` is a comment (`sns_local.py:151-158`). Exact query (`sns_local.py:142-146`):

```sql
SELECT feed_id,type,from_username,from_nickname,to_username,to_nickname,content,create_time,comment_id
FROM SnsMessage_tmp3 WHERE feed_id IN (?, ...) ORDER BY create_time
```

`feed_id` joins to `SnsTimeLine.tid` (`sns_local.py:137`, `doc_validate.py:274-295`). The validator notes that interactions are in `SnsMessage_tmp3`, not in the `LocalExtraInfo` table claimed by older docs (`doc_validate.py:293`).

---

## 10. Official-account (biz) databases

What is actually used:

- `contact.db::biz_info(username, type)` for subscription/service/enterprise classification (`bizService.ts:203`, `chatService.ts:4214-4219`).
- `biz_profile` and `biz_session_feeds` exist in the schema docs (`_docs\wechat-db-contact.md:151-180`) but are not queried by any local source.

What exists but is not queried anywhere in the toolchain or WeFlow:

- `db_storage/message/biz_message_0.db` (199,229,440 bytes): name suggests official-account message shards. No table/column evidence was found. `dirmap.py:14` explicitly lists `biz_message` as an undocumented extra DB.
- `db_storage/bizchat/bizchat.db` (28,672 bytes): presumably enterprise/WeCom chat metadata. No schema evidence.
- `db_storage/message/message_resource.db` (41,943,040 bytes): only used as an activity/freshness timestamp in archive logs; no query was found (`dirmap.py:14`; no hits in `media_local.py`/`local_dump.py`).

A native reader that needs official-account message bodies or enterprise chats must reverse these tables empirically; nothing on this machine describes them.

---

## 11. Message taxonomy and content decoding

### 11.1 `local_type` base map

Exact mapping used by the working decoder (`media_local.py:87-90`, mirrored in `_docs\wechat-db-message.md:121-130`):

| `local_type` | Kind |
|---:|---|
| 1 | text |
| 3 | image |
| 34 | voice |
| 42 | contact card |
| 43 | video |
| 47 | animated emoji |
| 48 | location |
| 49 | appmsg container; inspect XML `<type>` |
| 50 | voice/video call |
| 10000 | system message |

### 11.2 Large-integer encodings

The verified rule is `large_value = appmsg_type << 32 | 49` (`media_local.py:74-75`, validated on 4107 messages). The values actually mapped (`media_local.py:76-86`, `_docs\wechat-db-message.md:131-137`):

| Value | Kind |
|---:|---|
| 244813135921 (`57 << 32 \| 49`) | quote/reply |
| 266287972401 | pat |
| 81604378673 | forwarded chat log |
| 154618822705 | mini program |
| 8594229559345 | red packet |
| 8589934592049 | transfer |
| 34359738417, 103079215153, 25769803825 | file |
| 21474836529 (`5 << 32 \| 49`) | link (seen in the comment) |
| 12884901937 (`3 << 32 \| 49`) | music (seen in the comment) |

`classify()` handles an unknown large value generically: take `base = lt & 0xFFFF`, `sub = lt >> 32`; if `base == 49`, look up `sub` in `APPMSG_SUB` (`media_local.py:126-142`).

`APPMSG_SUB` (`media_local.py:91-95`): 3 music, 5 link, 6 file, 19 chatlog, 33 mini program, 36 mini program, 49 link, 57 quote, 87 announce, 2000 transfer, 2001 red packet.

### 11.3 Appmsg XML parsing pitfalls

`_appmsg_type()` first HTML-unescapes the text, rejects content without `appmsg`, removes nested `<refermsg>...</refermsg>` blocks, and only then scans `<type>`. This prevents a quoted message's inner type from being mistaken for the outer card type (`media_local.py:108-123`). Attribute parsing must tolerate `key = "value"` with spaces, not just `key="value"` (`media_local.py:201-207`).

### 11.4 Per-type payload details

| Kind | Raw payload | Extraction |
|---|---|---|
| text (1) | plain string or zstd blob | passthrough/decompress |
| image (3) | `<img md5 aeskey cdnurl ...>` | md5+aeskey feed the image pipeline; `aeskey[:16]` |
| voice (34) | `<voicemsg voicelength=... ...>` | `voicelength` in ms; locate via `VoiceInfo` |
| video (43) | `<videomsg md5 newmd5 aeskey playlength cdnvideourl ...>` | md5 -> video hardlink |
| emoji (47) | `<emoji md5 cdnurl aeskey desc ...>` | `desc` base64 caption; entity via cache |
| location (48) | `<location ... label="...">` | display label from `label` |
| card (42) | XML card payload | displayed as `[名片]` |
| call (50) | CDATA duration text, e.g. `通话时长 ...` | strip the prefix; display `[语音通话] ...` |
| appmsg (49) | `<appmsg><type>N</type><title>...</title>...` | subtype table above; title from `<title>` |
| quote (57) | appmsg with nested `<refermsg>` | strip refermsg for type; title is the visible quote text; the inner refermsg fields are not parsed |
| system (10000) | `sysmsg` XML; revoke has `<revokemsg>`/`<replacemsg>` | see below |

Sources: `media_local.py:108-123`, `media_local.py:244-257`, `media_local.py:640`, `media_local.py:210-240`, `local_dump.py:87-132`, `SKILL.md:61-65`.

VoIP is not decoded anywhere in the local sources. There is no `voip` or `voipmsg` handler in the Python toolchain; type 50 is rendered only from a CDATA duration string (`local_dump.py:127-131`). Richer call events (invite, cancel, missed, video/voice distinction) are an open item, not something the existing code can guide.

System/revoke handling in the Python dumper deserves a warning: `parse_display()` returns raw content immediately for `t == 10000` (`local_dump.py:92-93`), so the later revoke/`<replacemsg>` branch at `local_dump.py:113-122` is unreachable. The XML is preserved in `rawContent`, but the display string is not cleaned. A new reader should parse it explicitly:

```xml
<sysmsg type="revokemsg">
  <revokemsg>
    <session>...</session>
    <newmsgid>...</newmsgid>
    <replacemsg><![CDATA["X" 撤回了一条消息]]></replacemsg>
  </revokemsg>
</sysmsg>
```

Do not assume the Python display text is already clean.

### 11.5 `packed_info_data`

Declared BLOB, internal extension. Python ignores it. WeFlow uses it only as a preferred source of the video file name (32 hex chars) before falling back to XML (`chatService.ts:6550`). Its full structure is unknown (gap).

### 11.6 Compression and `WCDB_CT_*`

Observed content states (`SKILL.md:50`, `local_dump.py:41-57`):

- `message_content` is a string: use as-is.
- `message_content` is a blob starting with `28 b5 2f fd` (zstd frame magic): zstd-decompress, UTF-8 decode.
- Otherwise try zlib.
- If all fail, hex-encode for diagnostics.

The type tag column `WCDB_CT_message_content` is selected by the dumper (`local_dump.py:147`) but not used to choose a decoding branch; decoding is magic-based. The module docstring records the observed tag states: 0=text, 4=blob zstd (`local_dump.py:10`).

WeFlow uses the opposite field priority: it tries `compress_content` first, then `message_content` (`chatService.ts:8499-8505`), detects zstd in either byte order, and falls back to UTF-8/latin1 rather than zlib (`chatService.ts:8562-8593`). A native reader should probably inspect both `WCDB_CT_*` tags, try the tagged field first, and keep both zstd and zlib fallbacks. The exact precedence is not settled by the sources (gap).

### 11.7 `status` and other state columns

`status` is selected and carried into output (`local_dump.py:146`), but no local source maps its values. `upload_status`, `download_status`, `server_seq`, `origin_source`, and `source` are never used by the Python reader. WeFlow does not define enums for them either. Do not invent semantics.

---

## 12. Contact and session semantics summary

- `username` is the stable key used everywhere: `contact.username`, `SessionTable.username`, `Name2Id.user_name`, `chat_room.username`, chatroom member ids via name2id. It is the wxid for individuals, `...@chatroom` for groups, `gh_...` for official accounts (`doc_validate.py:103-107`).
- Display name: `remark` first, then `nick_name`; `alias` is the WeChat id (`_docs\wechat-db-contact.md:256-259`). `SessionTable.last_sender_display_name` is only a cache of the last sender (`_docs\wechat-db-session.md:36`).
- Group membership: `chatroom_member(room_id, member_id)` with both sides being `name2id.rowid` (`_docs\wechat-db-contact.md:83-89`). Group metadata (owner, mute, member list) is partly in `chat_room.ext_buffer`; only the mute flag (protobuf field 12) is documented (`_docs\wechat-db-contact.md:266-269`). WeFlow delegates parsing to the native DLL (`groupAnalyticsService.ts:241-243`, `wcdbCore.ts:1084`).
- `is_in_chat_room`, `chat_room_notify`, and `flag` are bitfields whose exact bits are not documented; WeFlow just coalesces the flag column name (`wcdbCore.ts:781`).
- Session list ordering: `sort_timestamp` descending (`_docs\wechat-db-session.md:30`). Unread count is in `SessionTable.unread_count`; unread message ids are in `SessionUnreadListTable_1`.

---

## 13. v3 vs v4 and version differences

The entire working toolchain is v4-only:

- `keyutil.py:2` names it "WeChat 4.0 WCDB (SQLCipher4-compatible)".
- `dirmap.py:2` names the directory model "微信 4.0".
- The shard loop hardcodes `message_0.db`...`message_2.db` (`local_dump.py:139`); there is no `MSG0.db`, `MicroMsg.db`, or `WeChat Files` branch anywhere.
- The hardlink tables are the v4 names ending `_v4` (`doc_validate.py:206`).
- Interactions are read from `SnsMessage_tmp3`, which is the v4 location (`doc_validate.py:274-295`).

No v3 reader exists in the repo or the toolchain, and no v3 database is present on this machine. Its key form and schema are therefore not established by any local source; supporting v3 would require separate reverse engineering.

Version drift inside v4 that the sources already handle or anticipate:

| Difference | Handling |
|---|---|
| `media_*.db` under `message/` instead of `Media/` | documented deviation, `dirmap.py:11` |
| Table names with suffixes (`VoiceInfo%`, `Name2Id%`) | WeFlow introspects via `LIKE` (`chatService.ts:10517-10524`) |
| Column aliases (`local_id` vs `id`, `create_time` vs `createtime`, `local_type` vs `type`) | WeFlow checks `PRAGMA table_info` and picks (`chatService.ts:10436-10438`) |
| Server-id column candidates (`msg_svr_id`, `msgsvrid`, `svr_id`, `svrid`, `server_id`, `serverid`) | `chatService.ts:10551-10552` |
| Key hook RVA changes with WeChat build | anchor-string PE scan instead of a fixed RVA; RVA 0x5defb0 recorded for 4.1.12.55 (`SKILL.md:78`) |
| FTS tokenizer | `MMFtsTokenizer` requires WeChat's plugin; Python `sqlcipher3` cannot `MATCH` (`doc_validate.py:148-166`) |

A native reader should introspect `PRAGMA table_info` and required-column subsets rather than assert exact schemas. The validator itself asserts only that required columns are a subset (`doc_validate.py:197-198`).

---

## 14. Gaps: not established by any local source

These need empirical testing before a native reader can be considered complete.

1. **`chat_room.ext_buffer` / group member list encoding.** Only protobuf field 12 (mute) is documented (`_docs\wechat-db-contact.md:268-269`). Member list, owner, announcement, and join state are not decoded. WeFlow calls a native export (`wcdb_get_chat_room_ext_buffer`, `wcdbCore.ts:1084`) whose implementation is not in the repo.
2. **`packed_info_data` structure.** Only used to fish out a video file name (`chatService.ts:6550`); no schema or field map exists.
3. **`status`, `upload_status`, `download_status`, `server_seq`, `origin_source`, `source`, `contact.local_type`, `SessionTable.type` enums.** Selected or documented but never decoded.
4. **Compression precedence and tags.** Python prefers `message_content`; WeFlow prefers `compress_content`. `WCDB_CT_*` values beyond 0/4 are unverified. Whether zlib ever occurs in production, versus only in older data, is unverified.
5. **Shard count and discovery.** The Python reader hardcodes 0..2; WeFlow discovers paths. A machine with 4+ shards or a renamed shard would break the hardcoded path. Table discovery (`GLOB 'Msg_*'`) exists but is only used for listing, not for opening all shards.
6. **Key acquisition automation.** The local toolchain does not implement `Local State` -> DPAPI -> `safe:`/`v10` -> master. The `safe:` blob location was not found in the inspected config files. The application-lock (`lock:`) branch is untested here. The working key is hardcoded in `config.json`/`keyutil.py:22`.
7. **Native DLL contract.** `wcdb_open_account(const char* session_db_path, const char* hex_key, wcdb_handle* out_handle)` is only a signature comment (`wcdbCore.ts:953-955`); the DLL is absent from the repo. The Rust replacement is a stub that stores the key without opening anything (`lib.rs:59-94`), and its probe uses `cipher_compatibility=4` with a 32-byte raw key (`probe.rs:7-13`), which is not the WeChat open scheme.
8. **Unqueried databases.** `biz_message_0.db`, `bizchat.db`, `message_resource.db`, `favorite.db`, `general.db`, `solitaire.db`, and `chatbot_message.db` have no table/column inventory from local evidence. `message_fts.db` is only structurally inspected (`message_fts_v4_0` plus shadow tables); its `MMFtsTokenizer` is not usable outside WeChat (`doc_validate.py:143-167`).
9. **Media completeness.** `.dat` and video rows can be absent (`dat_not_local`, `video_not_local`). V1 decryption uses a fixed key and is rarely exercised; no current sample proves it. `wxgf` to HEVC conversion depends on ffmpeg and may fail for unusual streams.
10. **SQLCipher fine details.** The HMAC page reserve, `cipher_plaintext_header_size`, `cipher_use_hmac`, and full-database integrity were not independently measured; only the working pragma set, a 16-byte salt assumption, and a 100-page integrity probe are evidenced (`keyutil.py:35-37`, `keyutil.py:82-85`). A wrong `reserve_sqlite_header` or page-size assumption would show up as "file is not a database" or HMAC failures.
11. **Voice edge cases.** Self-sent voice with `svr_id=0` is handled heuristically by time window; two voices within the same 5-second window can still collide. The code's claimed-set reduces but does not eliminate ambiguity. Empty/silent voice rows need an energy check.
12. **Display-name and group-owner logic.** Priority `remark > nick_name` is documented but not implemented centrally; group owner resolution in WeFlow is heuristic (`groupAnalyticsService.ts:241-243`). `contact.flag` bit meanings are unknown.
13. **WeChat 3.x.** No support, no schema, no key path.
14. **Stub shard files.** The 4 KiB `db_storage/message_0.db` stubs can be mistaken for real shards by a naive path resolver. Always prefer `db_storage/message/`.
15. **Application-level ids.** `local_id` is not global; merging multiple sources requires the renumbering/`_dbLocalId` discipline used by `wechat_cli.py:220-231`, or an independent stable key such as `server_id`.
16. **VoIP events.** Only the type-50 CDATA duration is handled (`local_dump.py:127-131`); no `<voipmsg>`/VoIP XML schema is decoded by any local source.

---

## 15. Quick verification recipes

1. **Prove the key/open sequence**: copy one DB plus `-wal`/`-shm`, read the first 16 bytes, run the five pragmas in section 2.4, then `SELECT count(*) FROM sqlite_master;`. A wrong key fails at this last statement.
2. **Prove a message table**: compute `Msg_` + `md5(peer_wxid)`, check `sqlite_master`, then run the projection in 3.2. `isSend` must equal 1 exactly when `Name2Id[real_sender_id] == self_wxid`.
3. **Prove media resolution**: take an `<img md5>` from a message, run the image hardlink query, resolve both `dir1`/`dir2` through `dir2id`, and confirm the `.dat` exists. Then check the first 6 bytes to pick V1/V2/V3 and decrypt.
4. **Prove voice**: resolve `chat_name_id` from `Name2Id` in `media_*.db`, try `svr_id`, then the plus/minus 5-second window; decode `voice_data` with pysilk and compare duration to `<voicemsg voicelength>`.
5. **Prove SNS**: parse the newest `SnsTimeLine` row's XML, compute time from `tid`, and join `SnsMessage_tmp3` on `feed_id = tid`.
6. **Prove group membership**: join `chat_room` -> `chatroom_member` -> `name2id` as in 5.2; verify the returned wxids against a known group member list. If `ext_buffer` is needed beyond mute, that is still an open reverse-engineering task.
