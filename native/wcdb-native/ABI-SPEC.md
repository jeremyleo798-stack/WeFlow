# wcdb_api.dll ABI 实现参考（WeFlow 清洁室重写用）

本文档面向用 Rust 重新实现 `wcdb_api.dll`。所有结论来自对本仓库 TypeScript 侧的只读追踪：
`electron/services/wcdbCore.ts`（koffi 绑定 + 结果封装）→ `electron/wcdbWorker.ts` → `electron/services/wcdbService.ts`
（纯 Worker 代理、零字段改写）→ 各业务服务。字段名与路径保持英文，无法从代码确定的地方写 `未确定`。

> 约束：DLL 由 Node/Electron 通过 koffi 以 C ABI 加载。字符串出参一律由 DLL 分配 UTF-8 内存，
> 调用方 `koffi.decode(ptr,'char',-1)` 后调用 `wcdb_free_string(ptr)` 释放（`wcdbCore.ts:1619-1629`）。
> 消费者对 JSON 普遍做“多别名 + 类型强制”容错，但少数位置是硬门槛（见第 1.6、14 节）。
## 1. 通用 ABI 约定
### 1.1 必需导出与可选导出

- 只有 5 个导出是硬性必需：`wcdb_init`、`wcdb_shutdown`、`wcdb_open_account`、`wcdb_close_account`、
  `wcdb_free_string`（`docs/third-party-components.md:17-19`、`scripts/check-wcdb-optional-exports.cjs:26`）。
- 其余全部通过 `bindOptionalExport` 绑定，缺失时该能力返回“当前数据服务版本不支持/接口未就绪”，
  不影响其他能力（`wcdbCore.ts:908-1442`、`docs/third-party-components.md:122-126`）。
- 游标三件套必须同时存在，否则 `openMessageCursor` 直接失败（`scripts/check-wcdb-optional-exports.cjs:231-241`）。
- `wcdb_purge_memory`、`wcdb_set_my_account_id`（旧版 `wcdb_set_my_wxid`）、`wcdb_get_logs`、`InitProtection`、
  `VerifyUser` 均为可选；旧版账号设置接口在 `wcdb_set_my_account_id` 缺失时回退（`wcdbCore.ts:968-971,1983-1991`）。
### 1.2 返回值

- `int32` 返回 `0` = 成功，非 `0` = 失败；错误文本通常只由调用方拼接成 `"<动作>失败: <code>"`
  （例：`wcdbCore.ts:2092-2095`）。少数导出把详细错误写进出参字符串（见各函数 `- Return:`）。
- `wcdb_get_sessions` 失败码 `-2` 有特殊语义：DLL 进程级 native context 为空；消费者会重开一次
  （`chatService.ts:1233-1237`）。`-7` 表示消息表 schema 不匹配（`wcdbCore.ts:2264-2266`）。
- `-3` 在游标打开时表示无消息库，会被重试一次（`wcdbCore.ts:4102-4178`）。
- 触发器安装返回 `1` = 已安装，`0` = 本次安装成功，`<0` = 失败（`wcdbCore.ts:4982-4991`）；
  查询触发器用 `int32* outInstalled`，`1` 为已安装（`wcdbCore.ts:5028-5030`）。
### 1.3 字符串所有权

统一规则：`void** outJson/outError/outUrl/outCaption/outName/outHex` 由 DLL 分配；
调用方 decode 后立即 `wcdb_free_string`。同一指针只释放一次（`scripts/check-wcdb-optional-exports.cjs:41-45,265-266`）。
DLL 侧必须保证：成功/失败都可以返回 `NULL`；调用方在 `outPtr[0]` 为空时按失败处理。
### 1.4 非字符串出参清单（不要分配字符串）

| 导出 | 出参类型 | 调用方行为 |
|---|---|---|
| `wcdb_open_account` | `int64* handle` | 必须 `>0`，否则 code `-3003`（`wcdbCore.ts:1971-1975`） |
| `wcdb_get_message_count` / `wcdb_get_group_member_count` | `int32*` | 直接作为计数 |
| `wcdb_open_message_cursor` | `int64* cursor` | 后续 fetch/close 使用 |
| `wcdb_fetch_message_batch` / `wcdb_scan_media_stream` | `int32* hasMore` | `===1` 视为还有更多（`wcdbCore.ts:4195`） |
| `wcdb_check_*_trigger` | `int32* installed` | `===1` 视为已安装 |
| `VerifyUser` | `char* outResult`（调用方分配 1024 字节） | koffi decode 后 `JSON.parse`（`wcdbCore.ts:4803-4811`） |

`wcdb_set_message_cursor_projection`、`wcdb_stop_monitor_pipe`、`wcdb_cloud_stop`、`wcdb_free_string` 返回 `void`。
### 1.5 JSON 命名与容错

- 绝大多数导出用 `snake_case`；例外：SNS 年度统计用 camelCase（`annualReportService.ts:1205-1229`）、
  联系人状态用 `isFolded/isMuted`（`chatService.ts:1993-1997`）、聚合统计用 camelCase（`analyticsService.ts:349-423`）。
- 消息行字段容错极强：`getRowField` 大小写不敏感回退（`chatService.ts:3899-3914`）；
  `local_type` 只读该键（`chatService.ts:6432`）；`computed_is_send ?? is_send` 是发送方判定入口
  （`chatService.ts:6435`、`analyticsService.ts:254-274`）。
- 批次接口的入参几乎都是“JSON 字符串数组”，单条接口用裸字符串；不要把数组再包一层对象。
- 64 位 `server_id` 若以 JSON number 输出且超过 16 位，调用方会在 parse 前用正则把它变成字符串
  （`wcdbCore.ts:1631-1644`）；建议直接输出十进制字符串。
### 1.6 消费者硬门槛（必须精确匹配）

1. `wcdb_get_sessions` 顶层必须是数组；首行含 `_error`/`_info` 即整体判失败（`chatService.ts:1244-1252`）。
2. `wcdb_get_contact_status` 必须返回 `{username:{isFolded,isMuted}}` 的 map，camelCase（`chatService.ts:1993-1997`）。
3. 聚合/群统计必须有 `total`、`typeCounts`、`hourly`、`sessions`，且 `total>0` 才会被采用
   （`analyticsService.ts:468-539`、`groupAnalyticsService.ts:651-680,1392-1403`）。
4. `wcdb_reorder_sessions_by_time` 即使返回 0，也必须给 `{"success":true,...}`，否则判失败
   （`wcdbCore.ts:2184-2186`）。
5. SNS 时间线分页靠 `rows.length < pageSize` 停止，DLL 必须严格执行 `limit/offset`
   （`snsService.ts:640-699`）。
6. 消息行缺 `_db_path`/`table_name` 时，`messageKey` 会退化到时间戳组合，可能重复
   （`chatService.ts:3650-3699`）。
## 2. A. 生命周期 / 账号
### `InitProtection`
- C sig: `int32 InitProtection(const char* resourcePath)` — `wcdbCore.ts:908`
- Return: `0` 通过；非 0 中止初始化。严重度排序：`-2212..-2201` 最轻，`-102/-101/-1006` 次之，其余最重（`wcdbCore.ts:910-936`）。
- outJson: 无
- Consumer: `wcdbCore.ts:917-936`；测试 `scripts/check-wcdb-optional-exports.cjs:153-171`
- Required: 不存在该导出时跳过；存在时会对多个候选路径逐一调用（`wcdbCore.ts:815-841`）
- Notes: 失败时 `getLastInitError()` 必须包含错误码；`-103` 会阻止 `wcdb_init` 被调用（测试断言 153-160）。
### `wcdb_init` / `wcdb_shutdown`
- C sig: `int32 wcdb_init()` / `int32 wcdb_shutdown()` — `wcdbCore.ts:941,944`
- Return: `0` 成功；`-101` 等非 0 视为初始化失败（`scripts/check-wcdb-optional-exports.cjs:135-143`）
- outJson: 无
- Consumer: `wcdbCore.ts:1000-1100`、`scripts/check-wcdb-optional-exports.cjs:112-124`
- Required: 进程内可重复 init/shutdown；`close()` 只调 shutdown，不调 close_account（`wcdbCore.ts:2008-2060`）
- Notes: `wcdb_shutdown` 后再 query 属未定义；消费者重连时会重新 init。
### `wcdb_purge_memory`
- C sig: `int32 wcdb_purge_memory()` — `wcdbCore.ts:948`
- Return: 忽略返回值（周期任务）
- outJson: 无
- Consumer: `wcdbCore.ts:2016-2035`
- Required: 关闭空闲连接/页缓存，不得使已打开 handle 失效
- Notes: 可选；不存在时周期任务不启动。
### `wcdb_open_account`
- C sig: `int32 wcdb_open_account(const char* path, const char* key, _Out_ int64* handle)` — `wcdbCore.ts:955`
- Return: `0` 且 handle>0 成功；其他值映射到错误文案（`wcdbCore.ts:414-448,1966-1975`）
- outJson: 无（`int64` handle）
- Consumer: `wcdbCore.ts:1911-2006`
- Required: `path` 实际是 `<accountDir>/db_storage/session/session.db` 的完整路径，不是目录（`wcdbCore.ts:1954-1963`）
- Notes: `key` 为原样十六进制字符串；成功后消费者会用目录名清洗出的 accountId 调 `wcdb_set_my_account_id`（`wcdbCore.ts:1977-1991`）。
### `wcdb_close_account`
- C sig: `int32 wcdb_close_account(int64 handle)` — `wcdbCore.ts:959`
- Return: 忽略（消费者刻意不调用）
- outJson: 无
- Consumer: 无调用点（`wcdbCore.ts:2010` 注释“可能导致崩溃，使用 shutdown 代替”）
- Required: 仍应实现且可安全重复调用
- Notes: 若实现有资源需释放，建议在 `wcdb_shutdown` 内兜底。
### `wcdb_set_my_account_id` / `wcdb_set_my_wxid`
- C sig: `int32 wcdb_set_my_account_id(int64 handle, const char* accountId)` / `int32 wcdb_set_my_wxid(int64 handle, const char* wxid)` — `wcdbCore.ts:963,969`
- Return: 非 0 只写日志，不阻断打开（`wcdbCore.ts:1983-1991`）
- outJson: 无
- Consumer: `wcdbCore.ts:1983-1991`、`scripts/check-wcdb-optional-exports.cjs:173-180`
- Required: 收到的是清洗后的 accountId（去 `accountId_`/`_xxxx` 后缀），不是完整目录名
- Notes: 优先 `set_my_account_id`，缺失时回退 `set_my_wxid`。
### `wcdb_free_string`
- C sig: `void wcdb_free_string(void* ptr)` — `wcdbCore.ts:993`
- Return: 无
- outJson: 无
- Consumer: `wcdbCore.ts:1619-1629`
- Required: 必须能接收 `NULL`；同一指针只释放一次
- Notes: 所有字符串出参都走此函数，包括失败时的 outError。
## 3. B. 会话
### `wcdb_get_sessions`
- C sig: `int32 wcdb_get_sessions(int64 handle, _Out_ void** outJson)` — `wcdbCore.ts:996`
- Return: `0` 成功；`-2` 触发一次重开（`chatService.ts:1233-1237`）
- outJson: `[{"username":"wxid_a","type":1,"unread_count":2,"summary":"hi","sort_timestamp":1710000000,"last_timestamp":1710000000,"last_msg_type":1,"last_msg_sender":"wxid_b","last_sender_display_name":"B"}]`
- Consumer: `chatService.ts:1218-1401`
- Required: `username`（别名 user_name/userName/usrName/UsrName/talker/talker_id/talkerId，1254-1266）；`sort_timestamp`（sortTimestamp/sort_time/sortTime，1295-1302）
- Optional: `last_timestamp`（lastTimestamp/last_msg_time/lastMsgTime，1303-1310）；`summary|digest|last_msg|lastMsg`（1312）；`last_msg_type`（1313）；`message_count|messageCount|msg_count|msgCount|total_count|totalCount|n_msg|nMsg|message_num|messageNum`（1314-1324）；`type`、`unread_count|unreadCount|unreadcount`、`last_msg_sender`、`last_sender_display_name`（1346-1360）
- Notes: 顶层必须是数组；首行 `_error/_info` 会整体判失败（1244-1252）；`username` 为空/系统号的会话被过滤（1277-1286）。
### `wcdb_mark_all_sessions_read`
- C sig: `int32 wcdb_mark_all_sessions_read(int64 handle, _Out_ void** outError)` — `wcdbCore.ts:1000`
- Return: 非 0 时 out 字符串作为错误文案（`wcdbCore.ts:2125-2131`）
- outJson: 纯文本错误消息（不是 JSON）：`"update failed: ..."`
- Consumer: `wcdbCore.ts:2107-2138`
- Required: 成功时 out 可为空
- Notes: 成功后消费者清空媒体流缓存。
### `wcdb_reorder_sessions_by_time`
- C sig: `int32 wcdb_reorder_sessions_by_time(int64 handle, _Out_ void** outJson)` — `wcdbCore.ts:1007`
- Return: `0` 仍需 body `success:true`；失败时 body 可带 `code/stage/error`（`wcdbCore.ts:2172-2186`）
- outJson: `{"success":true,"table":"session","sort_column":"sort_timestamp","source_column":"last_timestamp","row_count":10,"updated_rows":3,"already_normalized":false}`
- Consumer: `wcdbCore.ts:2141-2204`
- Required: `success`（boolean）；`table`、`sort_column`、`source_column`、`row_count`、`updated_rows`、`already_normalized`
- Optional: 失败 body 的 `error`、`code`、`stage`
- Notes: 消费者把 snake_case 转成 camelCase 返回；`success` 缺失即判失败，即使返回码为 0。
## 4. C. 消息
### `wcdb_get_messages`
- C sig: `int32 wcdb_get_messages(int64 handle, const char* username, int32 limit, int32 offset, _Out_ void** outJson)` — `wcdbCore.ts:1013`
- Return: `0` 成功；非 0 → `"获取消息失败: <code>"`
- outJson: `[{"local_id":1,"server_id":"123","local_type":1,"create_time":1710000000,"sort_seq":1,"message_content":"hi","compress_content":"","status":2,"is_send":1,"sender_username":"wxid_a","_db_path":"C:/.../message_0.db","table_name":"Msg_..."}]`
- Consumer: `chatService.ts:2893-3116`、`3211-3259`（offset 回退）
- Required: `message_content`、`compress_content`、`local_type`、`create_time`（createTime/msg_time/msgTime/time）、`local_id`、`server_id`、`sort_seq`、`is_send`（或 `computed_is_send`）、`sender_username`（`chatService.ts:6424-6450`）
- Optional: `_db_path|db_path`、`db_name`、`table_name`（`chatService.ts:3650-3660`）；`status` 可为 null
- Notes: 参数名虽是 username，实际传 sessionId；排序方向未显式约定（未确定），消费者按 `create_time` 展示并二次排序；`compress_content` 优先于 `message_content`（`chatService.ts:8499-8506`）。
### `wcdb_get_messages_by_type`
- C sig: `int32 wcdb_get_messages_by_type(int64 handle, const char* sessionId, int64 localType, int32 ascending, int32 limit, int32 offset, _Out_ void** outJson)` — `wcdbCore.ts:1248`
- Return: `0` 成功；结果非数组时消费者按空数组处理（`wcdbCore.ts:2529-2533`）
- outJson: 同 `wcdb_get_messages` 的行数组
- Consumer: `chatService.ts:11310,11353,11476`（语音/图片/资源扫描）
- Required: 行字段同消息行；图片需 `image_md5|imageMd5`、`image_dat_name|imageDatName`（`chatService.ts:6424-6732`）
- Optional: `localType<=0` 时按 34/3 兜底（`ExportContext.ts:957-963`）
- Notes: `limit=0` 表示由 DLL 决定上限（消费者用它取全会话，`chatService.ts:11310`）。
### `wcdb_get_message_count`
- C sig: `int32 wcdb_get_message_count(int64 handle, const char* username, _Out_ int32* outCount)` — `wcdbCore.ts:1016`
- Return: `-7` = message schema mismatch；其他非 0 失败（`wcdbCore.ts:2264-2270`）
- outJson: 无（`int32`）
- Consumer: `wcdbCore.ts:2257-2275`
- Required: 计数为会话总消息数（不受时间过滤）
- Notes: 批量版本由 JS 循环调用，DLL 无批量接口。
### `wcdb_get_message_by_id`
- C sig: `int32 wcdb_get_message_by_id(int64 handle, const char* sessionId, int32 localId, _Out_ void** outJson)` — `wcdbCore.ts:1197`
- Return: `0` 且对象非空；空对象视为“未找到”（`wcdbCore.ts:4395-4397`）
- outJson: 单个消息对象（字段同上）
- Consumer: `wcdbCore.ts:4385-4401`
- Required: `local_type`、`create_time`、`message_content`、`compress_content`、`is_send`
- Optional: 同消息行
- Notes: 找不到时建议返回 `{}` 而不是错误码。
### `wcdb_get_message_by_svrid`
- C sig: `int32 wcdb_get_message_by_svrid(int64 handle, const char* sessionId, const char* svrid, _Out_ void** outJson)` — `wcdbCore.ts:1019`
- Return: `0`；空字符串/空对象都映射为 `row:null`（`wcdbCore.ts:2288-2299`）
- outJson: 单个消息对象或 `{}`
- Consumer: `wcdbCore.ts:2277-2299`
- Required: 同消息行
- Optional: `svrid` 以字符串传入，避免精度丢失
- Notes: 未命中不要返回非 0。
### `wcdb_get_session_message_counts`
- C sig: `int32 wcdb_get_session_message_counts(int64 handle, const char* sessionIdsJson, _Out_ void** outJson)` — `wcdbCore.ts:1223`
- Return: `0` 成功
- outJson: `{"wxid_a":123,"room@chatroom":456}`
- Consumer: `wcdbCore.ts:2340-2364`、`chatService.ts:2368-2390`
- Required: 顶层是“sessionId → number”的扁平 map
- Optional: 缺失的会话由调用方补 0
- Notes: 入参是 JSON 数组；不要返回数组。
### `wcdb_get_session_message_type_stats` / `_batch`
- C sig: `int32 wcdb_get_session_message_type_stats(int64, const char* sessionId, int32 begin, int32 end, _Out_ void** outJson)` / `int32 wcdb_get_session_message_type_stats_batch(int64, const char* sessionIdsJson, const char* optionsJson, _Out_ void** outJson)` — `wcdbCore.ts:1228,1233`
- Return: `0` 成功；失败时消费者回退游标扫描（`chatService.ts:5763-5773`）
- outJson: `{"total_messages":100,"voice_messages":1,"image_messages":2,"video_messages":0,"emoji_messages":3,"file_messages":0,"call_messages":0,"transfer_messages":0,"red_packet_messages":0,"first_timestamp":1710000000,"last_timestamp":1710003600,"group_my_messages":10,"group_sender_count":5,"date_counts":{"2024-05-01":12}}`
- Consumer: `chatService.ts:5745-5833`、`5853-5880`；batch 选项 `{begin,end,quick_mode,include_group_sender_count,include_date_counts}`（`wcdbCore.ts:2426-2433`）
- Required: `total_messages`、`voice_messages`、`image_messages`、`video_messages`、`emoji_messages`、`first_timestamp`、`last_timestamp`
- Optional: `file_messages`（缺失会触发游标兜底）、`call_messages`、`transfer_messages`、`red_packet_messages`、`group_my_messages`、`group_sender_count`、`date_counts`
- Notes: batch 返回 `{sessionId: row}`；单条返回 row 本身（`wcdbCore.ts:2393-2443`）。
### `wcdb_get_session_message_date_counts` / `_batch`
- C sig: `int32 wcdb_get_session_message_date_counts(int64, const char* sessionId, _Out_ void** outJson)` / `..._batch(int64, const char* sessionIdsJson, _Out_ void** outJson)` — `wcdbCore.ts:1238,1243`
- Return: `0` 成功
- outJson: `{"2024-05-01":12,"2024-05-02":3}`（batch：`{"wxid_a":{"2024-05-01":12}}`）
- Consumer: `wcdbCore.ts:2446-2504`、`chatService.ts:12116-12126`
- Required: 日期键为 `YYYY-MM-DD`（本地时区）；值为正整数
- Optional: 值 <=0 的条目会被调用方丢弃
- Notes: 日期键必须与 `JumpToDatePopover` 的 `YYYY-MM-DD` 对齐（`src/components/JumpToDatePopover.tsx:67-71`）。
### `wcdb_open_message_cursor`
- C sig: `int32 wcdb_open_message_cursor(int64 handle, const char* sessionId, int32 batchSize, int32 ascending, int32 beginTimestamp, int32 endTimestamp, _Out_ int64* outCursor)` — `wcdbCore.ts:1152`
- Return: `0`；`-3` 重开一次后重试；`-7` 提示 schema mismatch（`wcdbCore.ts:4102-4178`）
- outJson: 无（`int64` cursor）
- Consumer: `wcdbCore.ts:4102-4178`、`chatService.ts:2893-3116`
- Required: `ascending=1` 时按时间升序；`begin/end` 为秒，0 表示不限制
- Optional: `batchSize` 只是提示，`fetch` 的返回条数不得为 0 且 hasMore=1
- Notes: `projection=1` 为 journey 轻量投影，由 `wcdb_set_message_cursor_projection` 设置（`wcdbCore.ts:1157`）。
### `wcdb_fetch_message_batch`
- C sig: `int32 wcdb_fetch_message_batch(int64 handle, int64 cursor, _Out_ void** outJson, _Out_ int32* outHasMore)` — `wcdbCore.ts:1163`
- Return: `0` 成功；`hasMore` 仅 `===1` 为真（`wcdbCore.ts:4195`）
- outJson: 消息行数组（字段同 `wcdb_get_messages`）
- Consumer: `wcdbCore.ts:4180-4199`
- Required: 行数组；`hasMore` 与行数一致（最后一批必须 0）
- Optional: 投影模式可只返回 `local_id/server_id/create_time/local_type/sort_seq/sender_username/is_send/message_content/compress_content`
- Notes: 游标只能单线程顺序消费；close 前允许重复 fetch 到 hasMore=0。
### `wcdb_close_message_cursor`
- C sig: `int32 wcdb_close_message_cursor(int64 handle, int64 cursor)` — `wcdbCore.ts:1166`
- Return: `0` 成功
- outJson: 无
- Consumer: `wcdbCore.ts:4201-4215`
- Required: 幂等；无效 cursor 不应崩溃
- Notes: 必须在 finally 中调用，消费者依赖它释放资源。
### `wcdb_set_message_cursor_projection`
- C sig: `int32 wcdb_set_message_cursor_projection(int64 handle, int64 cursor, int32 projection)` — `wcdbCore.ts:1157`
- Return: `0` 成功；消费者忽略失败
- outJson: 无
- Consumer: `wcdbCore.ts:4102-4178`（`'journey'` → `1`）
- Required: `1` 启用轻量投影；`0` 恢复全字段
- Notes: 可选导出；缺失时游标仍应可用。
### `wcdb_search_messages`
- C sig: `int32 wcdb_search_messages(int64 handle, const char* sessionId, const char* keyword, int32 limit, int32 offset, int32 begin, int32 end, _Out_ void** outJson)` — `wcdbCore.ts:1265`
- Return: `0` 成功
- outJson: 消息行数组
- Consumer: `chatService.ts:12727-12740`、`agentModelLedService.ts:5151`、`httpService.ts:1078`
- Required: 行字段同消息行；`sessionId` 为空串表示全库搜索（`wcdbCore.ts:4834`）
- Optional: `keyword` 可能包含中文/特殊字符，按参数化或转义处理
- Notes: 结果需按 `create_time` 倒序（消费者直接透传后再映射）。
### `wcdb_ai_query_session_candidates`
- C sig: `int32 wcdb_ai_query_session_candidates(int64 handle, const char* optionsJson, _Out_ void** outJson)` — `wcdbCore.ts:1270`
- Return: `0` 成功
- outJson: 未确定（消费者只透传 `data`）
- Consumer: `wcdbCore.ts:4847-4877`（无其他调用点）
- Required: 入参 `{keyword,session_id,limit,offset,begin_timestamp,end_timestamp}`（`wcdbCore.ts:4868-4875`）
- Optional: 未确定
- Notes: 语义为“按发送者聚合搜索候选”，输出形状未被任何消费者校验。
## 5. D. 联系人
### `wcdb_get_contact`
- C sig: `int32 wcdb_get_contact(int64 handle, const char* username, _Out_ void** outJson)` — `wcdbCore.ts:1054`
- Return: `0` 成功；Windows 下失败会回退 SQL 查询（`wcdbCore.ts:3532-3578`）
- outJson: `{"username":"wxid_a","alias":"a","remark":"A","nick_name":"Nick","big_head_img_url":"https://..."}`
- Consumer: `chatService.ts:8695-8717`、`groupAnalyticsService.ts:228-235`
- Required: `username`（别名 user_name/userName）；`alias|Alias`；`remark|Remark`；`nickName|nick_name|nickname|NickName`（`chatService.ts:8702-8716`）
- Optional: `big_head_img_url|bigHeadImgUrl|bigHeadUrl|big_head_url|small_head_img_url|smallHeadImgUrl|smallHeadUrl|small_head_url|head_img_url|headImgUrl|avatar_url|avatarUrl`（`wcdbCore.ts:3300-3330`）
- Notes: 平台上 macOS 优先 SQL，因此该导出可缺失（`wcdbCore.ts:3545-3548`）。
### `wcdb_get_contact_status`
- C sig: `int32 wcdb_get_contact_status(int64 handle, const char* usernamesJson, _Out_ void** outJson)` — `wcdbCore.ts:1058`
- Return: `0` 成功
- outJson: `{"wxid_a":{"isFolded":true,"isMuted":false}}`
- Consumer: `chatService.ts:1982-2005`
- Required: 键必须是 username；值对象必须是 camelCase `isFolded`/`isMuted`（`chatService.ts:1993-1997`）
- Optional: 缺失用户由调用方补 `{false,false}`
- Notes: 这是少数不接受 snake_case 别名的接口。
### `wcdb_get_contact_type_counts`
- C sig: `int32 wcdb_get_contact_type_counts(int64 handle, _Out_ void** outJson)` — `wcdbCore.ts:1064`
- Return: `0` 成功；失败回退 `getContactsCompact` 计算（`wcdbCore.ts:3737-3770`）
- outJson: `{"private":10,"group":3,"official":1,"former_friend":2,"blocked":0}`
- Consumer: `chatService.ts:2223-2242`、`src/stores/contactTypeCountsStore.ts:126`
- Required: `private`、`group`、`official`、`former_friend`、`blocked`
- Optional: 无
- Notes: 值可为数字字符串，消费者 `Number()` 强转。
### `wcdb_get_contacts_compact`
- C sig: `int32 wcdb_get_contacts_compact(int64 handle, const char* usernamesJson, _Out_ void** outJson)` — `wcdbCore.ts:1069`
- Return: `0` 且数组；否则回退 `exec_query('contact',...)`（`wcdbCore.ts:3772-3797`）
- outJson: `[{"username":"wxid_a","remark":"A","nick_name":"Nick","alias":"a","encrypt_username":"v1_x","local_type":1}]`
- Consumer: `chatService.ts:1565-1578`、`groupAnalyticsService.ts:630-655`
- Required: `username`；`local_type|localType`
- Optional: `remark`、`nick_name|nickName`、`alias`、`user_name|userName`、`encrypt_username|encryptUsername`、`encrypt_user_name|encryptUserName`（`groupAnalyticsService.ts:600-611`）
- Notes: `usernamesJson` 为 `null` 时表示全量（`wcdbCore.ts:3789`）。
### `wcdb_get_contact_alias_map`
- C sig: `int32 wcdb_get_contact_alias_map(int64 handle, const char* usernamesJson, _Out_ void** outJson)` — `wcdbCore.ts:1074`
- Return: `0` 成功
- outJson: `{"wxid_a":"alias_a"}`
- Consumer: `analyticsService.ts:85-98`、`chatService.ts:8706-8712`
- Required: 扁平 map，值必须是非空字符串
- Optional: 无 alias 的用户可省略或给 `""`
- Notes: 消费者会跳过 falsy alias（`analyticsService.ts:94`）。
### `wcdb_get_contact_friend_flags`
- C sig: `int32 wcdb_get_contact_friend_flags(int64 handle, const char* usernamesJson, _Out_ void** outJson)` — `wcdbCore.ts:1079`
- Return: `0` 成功
- outJson: `{"wxid_a":true,"wxid_b":false}`
- Consumer: `export/core/ExportContext.ts:5198-5220`
- Required: 扁平 map，值可 boolean/0/1（消费者 `Boolean()`）
- Optional: 缺失用户默认 false
- Notes: 入参是 JSON 数组。
### `wcdb_get_chat_room_ext_buffer`
- C sig: `int32 wcdb_get_chat_room_ext_buffer(int64 handle, const char* chatroomId, _Out_ void** outJson)` — `wcdbCore.ts:1084`
- Return: `0` 成功
- outJson: `{"ext_buffer":"<binary or base64/text>"}`
- Consumer: `groupAnalyticsService.ts:241-247`
- Required: `ext_buffer`（字符串）；消费者再按 XML/JSON 解析找群主
- Optional: 无
- Notes: 值可为 base64 或原始文本，消费者 `String()` 后正则解析（`groupAnalyticsService.ts:245-247`）。
### `wcdb_get_display_names`
- C sig: `int32 wcdb_get_display_names(int64 handle, const char* usernamesJson, _Out_ void** outJson)` — `wcdbCore.ts:1022`
- Return: `0` 成功；缺失导出时自动 SQL 回退（`wcdbCore.ts:3142-3249`）
- outJson: `{"wxid_a":"Alice"}`
- Consumer: `wcdbCore.ts:3142-3249`、`groupAnalyticsService.ts:1403-1412`
- Required: 扁平 map；键必须与入参 username 完全一致
- Optional: 缺失项调用方保持 username 原值
- Notes: SQL 回退按 `remark|nick_name|nickName|nickname|NickName|alias|Alias` 取值（`wcdbCore.ts:3195-3230`）。
### `wcdb_get_avatar_urls`
- C sig: `int32 wcdb_get_avatar_urls(int64 handle, const char* usernamesJson, _Out_ void** outJson)` — `wcdbCore.ts:1025`
- Return: `0` 成功；缺失导出时 SQL 回退（`wcdbCore.ts:3251-3355`）
- outJson: `{"wxid_a":"https://..."}`
- Consumer: `wcdbCore.ts:3251-3355`、`groupAnalyticsService.ts:1403-1412`
- Required: 扁平 map，值为完整 URL
- Optional: 无头像用户可省略
- Notes: 回退顺序包含 `big_head_img_url`→`small_head_img_url`→`head_img_url`→`avatar_url`（大小写/驼峰变体，`wcdbCore.ts:3300-3330`）。
### `wcdb_get_head_image_buffers`
- C sig: `int32 wcdb_get_head_image_buffers(int64 handle, const char* usernamesJson, _Out_ void** outJson)` — `wcdbCore.ts:1258`
- Return: `0` 成功
- outJson: `{"wxid_a":"ffd8ffe0..."}`（头像图片字节的十六进制串）
- Consumer: `chatService.ts:2175-2192`
- Required: 扁平 map，值为 hex；消费者 `Buffer.from(hex,'hex')` 后拼 `data:image/jpeg;base64,`
- Optional: 失败/空值可省略
- Notes: 必须是纯 hex（`^[a-fA-F0-9]+$`），不能带 `0x` 或 base64。
## 6. E. 群
### `wcdb_get_group_member_count`
- C sig: `int32 wcdb_get_group_member_count(int64 handle, const char* chatroomId, _Out_ int32* outCount)` — `wcdbCore.ts:1028`
- Return: `0` 成功
- outJson: 无（`int32`）
- Consumer: `wcdbCore.ts:3357-3372`
- Required: 计数为群成员总数
- Optional: 无
- Notes: 群不存在时建议返回 0 + 成功，而不是错误。
### `wcdb_get_group_member_counts`
- C sig: `int32 wcdb_get_group_member_counts(int64 handle, const char* chatroomIdsJson, _Out_ void** outJson)` — `wcdbCore.ts:1032`
- Return: `0` 成功
- outJson: `{"room@chatroom":5}`
- Consumer: `wcdbCore.ts:3374-3405`
- Required: 扁平 map，值为 number
- Optional: 缺失群由调用方当 0
- Notes: 入参 JSON 数组。
### `wcdb_get_group_members`
- C sig: `int32 wcdb_get_group_members(int64 handle, const char* chatroomId, _Out_ void** outJson)` — `wcdbCore.ts:1038`
- Return: `0` 成功
- outJson: `[{"username":"wxid_a","originalName":"群昵称","avatarUrl":"https://..."}]`
- Consumer: `groupAnalyticsService.ts:1128-1144,1285-1305`
- Required: 数组；每项 `username`
- Optional: `originalName`（群昵称）、`avatarUrl`
- Notes: 数组为空时消费者返回空列表，不报错（`groupAnalyticsService.ts:1139`）。
### `wcdb_get_group_nicknames`
- C sig: `int32 wcdb_get_group_nicknames(int64 handle, const char* chatroomId, _Out_ void** outJson)` — `wcdbCore.ts:1042`
- Return: `0` 成功
- outJson: `{"wxid_a":"群昵称"}`
- Consumer: `chatService.ts:4838-4853`、`groupAnalyticsService.ts:274-282`
- Required: 扁平 map，键 username，值非空字符串
- Optional: 缺失成员可省略
- Notes: 消费者会做“唯一性净化”，同 key 多值时可能丢弃（`groupAnalyticsService.ts:282-310`）。
### `wcdb_get_group_stats`
- C sig: `int32 wcdb_get_group_stats(int64 handle, const char* chatroomId, int32 begin, int32 end, _Out_ void** outJson)` — `wcdbCore.ts:1132`
- Return: `0` 成功；缺失导出时回退 `get_aggregate_stats([chatroomId])`（`wcdbCore.ts:3991-4012`）
- outJson: `{"total":100,"sent":40,"received":60,"firstTime":1710000000,"lastTime":1710003600,"typeCounts":{"1":50,"3":10},"hourly":[24 ints],"weekday":[7 ints],"daily":{"2024-05-01":12},"sentDaily":{"2024-05-01":5},"monthly":{"5":12},"sessions":{"room@chatroom":{"total":100,"sent":40,"received":60,"lastTime":1710003600,"senders":{"123":20}}},"idMap":{"123":"wxid_a"}}`
- Consumer: `groupAnalyticsService.ts:651-680,1392-1403,1442-1452,1461-1470`
- Required: `sessions[chatroomId].senders`、`idMap`、`hourly[24]`、`typeCounts`
- Optional: 其余聚合字段
- Notes: `senders` 的键是数字 id，必须用 `idMap` 还原 username（`groupAnalyticsService.ts:660-665`）；`hourly` 缺失时当 0（1448-1450）。
## 7. F. 写操作
### `wcdb_update_message`
- C sig: `int32 wcdb_update_message(int64 handle, const char* sessionId, int64 localId, int32 createTime, const char* newContent, _Out_ void** outError)` — `wcdbCore.ts:973`
- Return: `0` 成功；非 0 时 out 字符串作为错误文案（`wcdbCore.ts:5209-5217`）
- outJson: 纯文本错误消息
- Consumer: `wcdbCore.ts:5199-5224`
- Required: `(sessionId, localId, createTime)` 三元组唯一定位消息
- Optional: out 可为空
- Notes: 只改文本内容，不新增记录。
### `wcdb_insert_text_message`
- C sig: `int32 wcdb_insert_text_message(int64 handle, const char* sessionId, const char* content, int64 status, int64 createTime, _Out_ void** outJson)` — `wcdbCore.ts:980`
- Return: `0` 且 body `success:true`；非 0 时 out 为错误文本（`wcdbCore.ts:5246-5252`）
- outJson: `{"success":true,"local_id":1,"create_time":1710000000,"sort_seq":1,"status":2,"sender_row_id":1,"session_updated":true,"db_path":"C:/.../message_0.db","table_name":"Msg_..."}`
- Consumer: `wcdbCore.ts:5227-5270`
- Required: `success`（boolean）、`local_id`、`create_time`、`status`
- Optional: `sort_seq`、`sender_row_id`、`session_updated`、`db_path`、`table_name`、`error`
- Notes: `createTime=0` 表示由 DLL 取当前时间；发送者必须是“当前账号”。
### `wcdb_delete_message`
- C sig: `int32 wcdb_delete_message(int64 handle, const char* sessionId, int64 localId, int32 createTime, const char* dbPathHint, _Out_ void** outError)` — `wcdbCore.ts:987`
- Return: `0` 成功；非 0 时 out 为错误文案（`wcdbCore.ts:5286-5294`）
- outJson: 纯文本错误消息
- Consumer: `wcdbCore.ts:5275-5300`
- Required: `dbPathHint` 可能为空串，表示自行定位分片
- Optional: 无
- Notes: 删除需同时维护会话摘要/计数（由 DLL 负责）。
### `wcdb_install_message_anti_revoke_trigger`
- C sig: `int32 wcdb_install_message_anti_revoke_trigger(int64 handle, const char* sessionId, _Out_ void** outError)` — `wcdbCore.ts:1356`
- Return: `1` 已安装；`0` 本次安装成功；`<0` 失败，out 为错误文本（`wcdbCore.ts:4982-4991`）
- outJson: 纯文本错误消息
- Consumer: `wcdbCore.ts:4971-4994`
- Required: 幂等；`sessionId` 为空时消费者已拦截
- Optional: 无
- Notes: 触发器 SQL 内容未确定；消费者只关心安装状态与错误文本（`chatService.ts:1583-1594`）。
### `wcdb_uninstall_message_anti_revoke_trigger`
- C sig: `int32 wcdb_uninstall_message_anti_revoke_trigger(int64 handle, const char* sessionId, _Out_ void** outError)` — `wcdbCore.ts:1363`
- Return: `0` 成功；非 0 失败（`wcdbCore.ts:5011-5014`）
- outJson: 纯文本错误消息
- Consumer: `wcdbCore.ts:4996-5016`
- Required: 未安装时也应返回 0（幂等）
- Optional: 无
- Notes: 只删除本实现创建的触发器。
### `wcdb_check_message_anti_revoke_trigger`
- C sig: `int32 wcdb_check_message_anti_revoke_trigger(int64 handle, const char* sessionId, _Out_ int32* outInstalled)` — `wcdbCore.ts:1370`
- Return: `0` 成功；`outInstalled` 仅 `===1` 为已安装（`wcdbCore.ts:5028-5030`）
- outJson: 无（`int32`）
- Consumer: `wcdbCore.ts:5018-5033`
- Required: 已安装写 1，未安装写 0
- Optional: 无
- Notes: 检查失败不要写 1。
### `wcdb_install_sns_block_delete_trigger` / `wcdb_uninstall_sns_block_delete_trigger` / `wcdb_check_sns_block_delete_trigger`
- C sig: `int32 wcdb_install_sns_block_delete_trigger(int64 handle, _Out_ void** outError)` / `int32 wcdb_uninstall_sns_block_delete_trigger(int64 handle, _Out_ void** outError)` / `int32 wcdb_check_sns_block_delete_trigger(int64 handle, _Out_ int32* outInstalled)` — `wcdbCore.ts:1377,1384,1391`
- Return: 安装 `1`=已安装、`0`=成功、`<0`=失败；检查 `1`=已安装（`wcdbCore.ts:5103-5149`）
- outJson: 纯文本错误消息（install/uninstall）
- Consumer: `wcdbCore.ts:5089-5152`
- Required: 作用于 sns.db，不依赖 sessionId
- Optional: 无
- Notes: 目的是阻止朋友圈“删除”操作；SQL 未确定。
### `wcdb_delete_sns_post`
- C sig: `int32 wcdb_delete_sns_post(int64 handle, const char* postId, _Out_ void** outError)` — `wcdbCore.ts:1398`
- Return: `0` 成功；非 0 失败，out 为错误文本（`wcdbCore.ts:5166-5170`）
- outJson: 纯文本错误消息
- Consumer: `wcdbCore.ts:5154-5172`
- Required: `postId` 为字符串（朋友圈 tid）
- Optional: 无
- Notes: 与 block-delete 触发器联动时需避免自锁。
## 8. G. 媒体
### `wcdb_scan_media_stream`
- C sig: `int32 wcdb_scan_media_stream(int64 handle, const char* sessionIdsJson, int32 mediaType, int32 begin, int32 end, int32 limit, int32 offset, _Out_ void** outJson, _Out_ int32* outHasMore)` — `wcdbCore.ts:1253`
- Return: `0` 成功；`hasMore` 非 0 即有更多（`wcdbCore.ts:3119-3136`）
- outJson: `[{"session_id":"wxid_a","local_id":1,"server_id":"123","create_time":1710000000,"local_type":3,"is_send":1,"sender_username":"wxid_b","image_md5":"...","image_dat_name":"...dat","video_md5":"...","message_content":"<msg>...","compress_content":"","packed_info_data":"..."}]`
- Consumer: `wcdbCore.ts:2536-3140`、`ResourcesPage.tsx:1359-1425`、`ExportContext.ts:922-1000`
- Required: 行数组；`create_time`、`local_type`；图片 `image_md5|imageMd5`；视频 `video_md5|videoMd5|raw_md5|rawMd5`
- Optional: 列别名极多：`message_content|messageContent|message_content_text|messageText|StrContent|str_content|msg_content|msgContent|strContent|content|rawContent|WCDB_CT_message_content`；`compress_content` 同理；`packed_info_data|packedInfoData|packed_info_blob|packedInfoBlob|packed_info|packedInfo|BytesExtra|bytes_extra|WCDB_CT_packed_info|reserved0|Reserved0|WCDB_CT_Reserved0`（`wcdbCore.ts:2800-2820`）
- Notes: `mediaType` 0=全部/1=图片/2=视频（`wcdbCore.ts:2962`）；`sessionIdsJson` 为空数组时消费者不调用；行需按时间倒序，offset 必须稳定。
### `wcdb_resolve_image_hardlink` / `_batch`
- C sig: `int32 wcdb_resolve_image_hardlink(int64, const char* md5, const char* accountDir, _Out_ void** outJson)` / `int32 wcdb_resolve_image_hardlink_batch(int64, const char* requestsJson, _Out_ void** outJson)` — `wcdbCore.ts:1334,1339`
- Return: `0` 成功；单条 out 为对象，batch 为数组（`wcdbCore.ts:4475-4523`）
- outJson: 单条 `{"file_name":"abc.dat","full_path":"C:/.../abc.dat"}`；batch 请求 `[{"md5":"...","account_dir":"C:/..."}]`，响应 `[{"index":0,"md5":"...","success":true,"data":{"file_name":"...","full_path":"..."},"error":""}]`
- Consumer: `imageDecryptService.ts:489-505`、`1594-1610`
- Required: `data.file_name`、`data.full_path`（绝对路径）；batch 的 `success` 可为 `true|1|"1"`（`wcdbCore.ts:4560-4600`）
- Optional: `index`、`md5`、`error`
- Notes: 消费者会检查路径存在并缓存；`full_path` 必须可直接 `existsSync`。
### `wcdb_resolve_video_hardlink_md5` / `_batch`
- C sig: `int32 wcdb_resolve_video_hardlink_md5(int64, const char* md5, const char* dbPath, _Out_ void** outJson)` / `..._batch(int64, const char* requestsJson, _Out_ void** outJson)` — `wcdbCore.ts:1344,1349`
- Return: `0` 成功
- outJson: 单条 `{"resolved_md5":"..."}`；batch 请求 `[{"md5":"...","db_path":"..."}]`，响应 `[{"index":0,"md5":"...","success":true,"data":{"resolved_md5":"..."}}]`
- Consumer: `videoService.ts:211-305`
- Required: `data.resolved_md5`（非空字符串）
- Optional: `index`、`md5`、`error`
- Notes: 未命中时返回空对象/空字符串，不要返回错误码（消费者会按未解析处理）。
### `wcdb_get_voice_data`
- C sig: `int32 wcdb_get_voice_data(int64 handle, const char* sessionId, int32 createTime, int32 localId, int64 svrId, const char* candidatesJson, _Out_ void** outHex)` — `wcdbCore.ts:1208`
- Return: `0` 且 out 非空；否则失败（`wcdbCore.ts:4411-4415`）
- outJson: 纯十六进制字符串（语音 blob，通常 SILK）：`"02232153494c4b..."`，不是 JSON
- Consumer: `chatService.ts:10016-10360`、`decodeVoiceBlob`（`8194-8214`）
- Required: out 为偶数长度 hex；空字符串视为未命中
- Optional: `candidatesJson` 是候选路径/键数组，可为 `[]`
- Notes: 消费者也接受 base64/数组/对象，但首选 hex；`svrId` 用 int64 直传以避免精度问题（`chatService.ts:10317-10322`）。
### `wcdb_get_voice_data_batch`
- C sig: `int32 wcdb_get_voice_data_batch(int64 handle, const char* requestsJson, _Out_ void** outJson)` — `wcdbCore.ts:1213`
- Return: `0` 成功
- outJson: `[{"index":0,"hex":"0223..."}]`
- Consumer: `wcdbCore.ts:4420-4441`、`chatService.ts:10334-10360`
- Required: 请求 `[{session_id,create_time,local_id,svr_id,candidates}]`；响应数组项 `index`、`hex`
- Optional: 未命中项可省略或 `hex:""`
- Notes: `index` 必须对应请求下标；消费者只取 `rows[0].hex`。
### `wcdb_list_message_dbs`
- C sig: `int32 wcdb_list_message_dbs(int64 handle, _Out_ void** outJson)` — `wcdbCore.ts:1191`
- Return: `0` 成功
- outJson: `["C:/.../message_0.db","C:/.../message_1.db"]`
- Consumer: `wcdbCore.ts:4355-4369`、`chatService.ts:2257-2275`、`backupService.ts:601-619`
- Required: 字符串数组，绝对路径
- Optional: 无
- Notes: 顺序建议按分片号升序；空数组合法。
### `wcdb_list_media_dbs`
- C sig: `int32 wcdb_list_media_dbs(int64 handle, _Out_ void** outJson)` — `wcdbCore.ts:1194`
- Return: `0` 成功
- outJson: `["C:/.../media_0.db","C:/.../favorite.db"]`
- Consumer: `wcdbCore.ts:4371-4385`、`chatService.ts:795-805,10384+`
- Required: 字符串数组，绝对路径
- Optional: 无
- Notes: 消费者直接当路径数组使用，不能包对象。
### `wcdb_get_media_schema_summary`
- C sig: `int32 wcdb_get_media_schema_summary(int64 handle, const char* dbPath, _Out_ void** outJson)` — `wcdbCore.ts:1218`
- Return: `0` 成功
- outJson: 未确定（消费者仅透传 `data`）
- Consumer: `wcdbCore.ts:4443-4457`（无业务调用点）
- Required: 建议返回 `{tables:[...],columns:{...}}`，但无消费者约束
- Optional: 未确定
- Notes: 纯诊断接口。
## 9. H. 表情
### `wcdb_get_emoticon_cdn_url`
- C sig: `int32 wcdb_get_emoticon_cdn_url(int64 handle, const char* dbPath, const char* md5, _Out_ void** outUrl)` — `wcdbCore.ts:1172`
- Return: `0` 且 out 非空（`wcdbCore.ts:4300-4310`）
- outJson: 纯 URL 字符串（不是 JSON）
- Consumer: `chatService.ts:3145-3155`、`dualReportService.ts:710-716`
- Required: 完整 `http(s)` URL
- Optional: 未命中返回空串 + 成功
- Notes: 消费者仅在 `success && url` 时采用。
### `wcdb_get_emoticon_caption`
- C sig: `int32 wcdb_get_emoticon_caption(int64 handle, const char* dbPath, const char* md5, _Out_ void** outCaption)` — `wcdbCore.ts:1176`
- Return: `0` 且 out 非空
- outJson: 纯文本说明/关键词
- Consumer: `wcdbCore.ts:4313-4332`（无业务调用点）
- Required: 字符串
- Optional: 未命中空串
- Notes: 与 strict 版本区别是带 dbPath。
### `wcdb_get_emoticon_caption_strict`
- C sig: `int32 wcdb_get_emoticon_caption_strict(int64 handle, const char* md5, _Out_ void** outCaption)` — `wcdbCore.ts:1184`
- Return: `0` 且 out 非空
- outJson: 纯文本
- Consumer: `export/core/ExportContext.ts:1275-1285`
- Required: 字符串；导出时作为表情描述
- Optional: 未命中空串
- Notes: 不带 dbPath，DLL 自行定位表情库。
## 10. I. SNS（朋友圈）
### `wcdb_get_sns_timeline`
- C sig: `int32 wcdb_get_sns_timeline(int64 handle, int32 limit, int32 offset, const char* username, const char* keyword, int32 startTime, int32 endTime, _Out_ void** outJson)` — `wcdbCore.ts:1277`
- Return: `0` 成功；out 必须是数组，否则消费者判失败（`snsService.ts:1307-1308`）
- outJson: `[{"username":"wxid_a","nickname":"A","type":1,"tid":"...","createTime":1710000000,"contentDesc":"...","rawXml":"<TimelineObject>...","location":"...","media":[{"url":"...","thumb":"...","md5":"...","token":"...","thumbToken":"...","key":"...","encIdx":1,"livePhoto":{...}}],"comments":[{"id":"1","nickname":"B","content":"...","refCommentId":"0","refNickname":"","emojis":[{"url":"...","md5":"...","width":1,"height":1,"encryptUrl":"...","aesKey":"..."}]}],"likes":[...]}]`
- Consumer: `snsService.ts:1306-1368`、`640-699`
- Required: `username`（或 user_name/userName，`snsService.ts:634-638`）；`type`；`rawXml`；`media`；`comments`；`nickname`；`location`
- Optional: `tid`、`createTime`、`contentDesc`、`likes`、`linkTitle`、`linkUrl`（`snsService.ts:45-61`）
- Notes: 分页必须严格：`rows.length < pageSize` 用于停止（`snsService.ts:648-660`）；`username` 为空串表示全部；`keyword` 为空串表示不筛选。
### `wcdb_get_sns_annual_stats`
- C sig: `int32 wcdb_get_sns_annual_stats(int64 handle, int32 begin, int32 end, _Out_ void** outJson)` — `wcdbCore.ts:1284`
- Return: `0` 成功
- outJson: `{"totalPosts":42,"typeCounts":{"1":30,"15":2},"topLikers":[{"username":"wxid_a","count":10}],"topLiked":[{"username":"wxid_b","count":8}]}`
- Consumer: `annualReportService.ts:1205-1229`
- Required: camelCase `totalPosts`、`topLikers[]`、`topLiked[]`（每项 `username`、`count`）
- Optional: `typeCounts`
- Notes: 全时间范围 `begin=0,end=now`；返回 0 时消费者会回退导出统计（1234-1243）。
### `wcdb_get_sns_usernames`
- C sig: `int32 wcdb_get_sns_usernames(int64 handle, _Out_ void** outJson)` — `wcdbCore.ts:1289`
- Return: `0` 成功
- outJson: `["wxid_a","wxid_b"]`
- Consumer: `snsService.ts:976-1000`、`httpService.ts:1379`
- Required: 字符串数组；空数组合法（消费者会回退时间线收集）
- Optional: 无
- Notes: 元素会被 `String().trim()` 过滤空值。
### `wcdb_get_sns_export_stats`
- C sig: `int32 wcdb_get_sns_export_stats(int64 handle, const char* myAccountId, _Out_ void** outJson)` — `wcdbCore.ts:1294`
- Return: `0` 成功
- outJson: `{"total_posts":42,"total_friends":10,"my_posts":5}`
- Consumer: `wcdbCore.ts:4948-4970`、`snsService.ts:1000-1011`、`annualReportService.ts:1234-1243`
- Required: `total_posts`、`total_friends`；`my_posts` 可为 `null`
- Optional: 无
- Notes: 消费者在 core 内转成 camelCase；`myAccountId` 可为空字符串/null。
## 11. J. 统计聚合
### `wcdb_get_aggregate_stats`
- C sig: `int32 wcdb_get_aggregate_stats(int64 handle, const char* sessionIdsJson, int32 begin, int32 end, _Out_ void** outJson)` — `wcdbCore.ts:1093`
- Return: `0` 成功；`total===0` 时消费者可能重算（`analyticsService.ts:468-539`）
- outJson: `{"total":100,"sent":40,"received":60,"firstTime":1710000000,"lastTime":1710003600,"typeCounts":{"1":50,"3":10},"hourly":[24],"weekday":[7],"daily":{"2024-05-01":12},"sentDaily":{"2024-05-01":5},"monthly":{"5":12},"sessions":{"wxid_a":{"total":100,"sent":40,"received":60,"lastTime":1710003600}},"idMap":{"123":"wxid_a"}}`
- Consumer: `analyticsService.ts:349-423,468-539,717-748`
- Required: camelCase `total`、`sent`、`received`、`firstTime`、`lastTime`、`typeCounts`、`hourly`、`sessions`
- Optional: `daily`、`sentDaily`、`monthly`、`weekday`、`idMap`
- Notes: `total>0` 才会被采用；`typeCounts` 至少要有 1/3/34/43/47（`analyticsService.ts:730-740`）；sessionIds 全是数字时 core 会尝试数字 id 直传并用 `idMap` 反查（`wcdbCore.ts:3848-3905`）。
### `wcdb_get_available_years`
- C sig: `int32 wcdb_get_available_years(int64 handle, const char* sessionIdsJson, _Out_ void** outJson)` — `wcdbCore.ts:1097`
- Return: `0` 成功；空数组会触发消费者表格扫描兜底（`annualReportService.ts:726-760`）
- outJson: `[2022,2023,2024]`
- Consumer: `annualReportService.ts:628-760`
- Required: 数字数组（升序）
- Optional: 元素可为数字字符串，`normalizeAvailableYears` 会强转
- Notes: 消费者给该调用设置了 5s 超时（`annualReportService.ts:719-733`），实现应尽快返回。
### `wcdb_get_annual_report_stats`
- C sig: `int32 wcdb_get_annual_report_stats(int64 handle, const char* sessionIdsJson, int32 begin, int32 end, _Out_ void** outJson)` — `wcdbCore.ts:1104`
- Return: `0` 成功；缺失导出时回退 `get_aggregate_stats`（`wcdbCore.ts:3930-3935`）
- outJson: `{"total":100,"sessions":{"wxid_a":{"sent":40,"received":60,"monthly":{"5":12}}},"daily":{"2024-05-01":12}}`
- Consumer: `annualReportService.ts:903-939`
- Required: `total`、`sessions`（每项 `sent`、`received`、`monthly`）、`daily`
- Optional: 无
- Notes: `monthly` 键是月份数字（1-12）；`daily` 键是 `YYYY-MM-DD`。
### `wcdb_get_annual_report_extras`
- C sig: `int32 wcdb_get_annual_report_extras(int64 handle, const char* sessionIdsJson, int32 begin, int32 end, int32 peakBegin, int32 peakEnd, _Out_ void** outJson)` — `wcdbCore.ts:1111`
- Return: `0` 成功；失败则消费者走完整逐条分析（`annualReportService.ts:957-1026`）
- outJson: `{"heatmap":[[24 ints] x7],"midnight":{"wxid_a":3},"conversation":{"wxid_a":{"initiated":2,"received":1}},"response":{"wxid_a":{"avg":12.5,"fastest":1,"count":9}},"peakDay":{"wxid_a":5},"topPhrases":[{"phrase":"哈哈","count":3}],"streak":{"sessionId":"wxid_a","days":5,"startDate":"2024-05-01","endDate":"2024-05-05"}}`
- Consumer: `annualReportService.ts:957-1026`
- Required: `heatmap` 为 7x24 数字矩阵；`midnight`、`conversation`、`response`、`peakDay`、`topPhrases`、`streak`
- Optional: `response` 的键名 `avg/fastest/count` 由消费者透传
- Notes: `peakBegin/peakEnd` 是峰值日的时间范围（秒），用于只统计当天。
### `wcdb_get_dual_report_stats`
- C sig: `int32 wcdb_get_dual_report_stats(int64 handle, const char* sessionId, int32 begin, int32 end, _Out_ void** outJson)` — `wcdbCore.ts:1118`
- Return: `0` 成功
- outJson: `{"counts":{"total":100,"words":800,"image":10,"voice":5,"emoji":3},"emojis":[{"md5":"...","url":"...","isMe":true,"count":4}],"phrases":[{"phrase":"...","count":3}],"myPhrases":[...],"friendPhrases":[...],"heatmap":[[24]x7],"initiative":{},"response":{},"monthly":{},"streak":{}}`
- Consumer: `dualReportService.ts:650-735,760-810`
- Required: `counts`（`total`、`words`、`image`、`voice`、`emoji`）；`emojis` 项 `md5`、`isMe`、`count`；`phrases`/`myPhrases`/`friendPhrases` 项 `phrase`、`count`
- Optional: `url`、`heatmap`、`initiative`、`response`、`monthly`、`streak`（形状未确定，消费者原样透传给渲染层）
- Notes: `emojis` 缺失时消费者回退扫描（`dualReportService.ts:700-715`）。
### `wcdb_get_my_footprint_stats`
- C sig: `int32 wcdb_get_my_footprint_stats(int64 handle, const char* optionsJson, _Out_ void** outJson)` — `wcdbCore.ts:1139`
- Return: `0` 成功；失败时消费者走游标兜底（`chatService.ts:13811+`）
- outJson: `{"summary":{"private_inbound_people":1,"private_replied_people":1,"private_outbound_people":1,"private_reply_rate":0.5,"mention_count":2,"mention_group_count":1},"private_sessions":[{"session_id":"wxid_a","incoming_count":3,"outgoing_count":2,"replied":true,"first_incoming_ts":1710000000,"first_reply_ts":1710000010,"latest_ts":1710000100,"anchor_local_id":1,"anchor_create_time":1710000000}],"private_segments":[{"session_id":"wxid_a","segment_index":0,"start_ts":1710000000,"end_ts":1710000100,"duration_sec":100,"incoming_count":3,"outgoing_count":2,"message_count":5,"replied":true,"first_incoming_ts":1710000000,"first_reply_ts":1710000010,"latest_ts":1710000100,"anchor_local_id":1,"anchor_create_time":1710000000}],"mentions":[{"session_id":"room@chatroom","local_id":1,"create_time":1710000000,"sender_username":"wxid_b","message_content":"@me ...","source":"text_at_me"}],"mention_groups":[{"session_id":"room@chatroom","count":2,"latest_ts":1710000000}],"diagnostics":{"truncated":false,"scanned_dbs":3,"elapsed_ms":12}}`
- Consumer: `chatService.ts:13333-13414`、`12130-12290`
- Required: 所有键 `snake_case`；`private_sessions`、`private_segments`、`mentions`、`mention_groups` 必须是数组
- Optional: `diagnostics.mention_truncated`、`private_truncated`；`displayName/avatarUrl` 由消费者补充
- Notes: 入参 `{begin,end,my_account_id,private_session_ids,group_session_ids,mention_limit,private_limit,mention_mode}`（`wcdbCore.ts:4053-4063`）；`mention_limit=0` 表示不截断候选。
### `wcdb_get_message_dates`
- C sig: `int32 wcdb_get_message_dates(int64 handle, const char* sessionId, _Out_ void** outJson)` — `wcdbCore.ts:1146`
- Return: 非 0 或空指针时返回 `{success:true,dates:[]}`（`wcdbCore.ts:3479-3489`）
- outJson: `["2024-05-01","2024-05-03"]`
- Consumer: `chatService.ts:11532-11550`、`JumpToDatePopover.tsx:67-71`
- Required: `YYYY-MM-DD` 字符串数组（本地日期）
- Optional: 顺序建议升序
- Notes: 日期格式必须与前端拼 `T00:00:00` 可解析。
## 12. K. 库内省 / 快照
### `wcdb_get_message_tables`
- C sig: `int32 wcdb_get_message_tables(int64 handle, const char* sessionId, _Out_ void** outJson)` — `wcdbCore.ts:1048`
- Return: `0` 成功
- outJson: `[{"table_name":"Msg_abc","db_path":"C:/.../message_0.db"}]`
- Consumer: `wcdbCore.ts:3449-3467`（业务侧已改用 `get_message_table_stats`）
- Required: 数组；`table_name`、`db_path`
- Optional: `name` 作为 `table_name` 别名（`chatService.ts:5420-5422`）
- Notes: 空数组合法。
### `wcdb_get_message_table_stats`
- C sig: `int32 wcdb_get_message_table_stats(int64 handle, const char* sessionId, _Out_ void** outJson)` — `wcdbCore.ts:1090`
- Return: `0` 成功
- outJson: `[{"table_name":"Msg_abc","db_path":"C:/.../message_0.db","count":100,"first_timestamp":1710000000,"last_timestamp":1710003600}]`
- Consumer: `chatService.ts:1750-1778,5418-5437,9570-9629`、`annualReportService.ts:366-380`
- Required: `table_name`（或 `name`）、`db_path`、`count`（别名 `message_count|messageCount`）
- Optional: `first_timestamp|firstTimestamp|first_time|firstTime|min_create_time|minCreateTime`；`last_timestamp|lastTimestamp|last_time|lastTime|max_create_time|maxCreateTime`
- Notes: 每个会话可有多张表/多个分片；`count` 必须为该表消息数（`chatService.ts:9581-9586`）。
### `wcdb_get_message_meta`
- C sig: `int32 wcdb_get_message_meta(int64 handle, const char* dbPath, const char* tableName, int32 limit, int32 offset, _Out_ void** outJson)` — `wcdbCore.ts:1051`
- Return: `0` 成功
- outJson: 行数组（未确定字段，原样返回）
- Consumer: `wcdbCore.ts:3512-3530`（无业务调用点）
- Required: 数组
- Optional: 未确定
- Notes: 纯调试接口。
### `wcdb_get_message_table_columns`
- C sig: `int32 wcdb_get_message_table_columns(int64 handle, const char* dbPath, const char* tableName, _Out_ void** outJson)` — `wcdbCore.ts:1299`
- Return: `0` 成功
- outJson: `["local_id","server_id","local_type","create_time","message_content"]`
- Consumer: `chatService.ts:5439-5467`、`annualReportService.ts:291-310`
- Required: 字符串数组（列名，不含类型）
- Optional: 无
- Notes: 消费者统一转小写做存在性判断。
### `wcdb_get_message_table_time_range`
- C sig: `int32 wcdb_get_message_table_time_range(int64 handle, const char* dbPath, const char* tableName, _Out_ void** outJson)` — `wcdbCore.ts:1329`
- Return: `0` 成功
- outJson: `{"column":"create_time","first_ts":1710000000,"last_ts":1710003600}`
- Consumer: `annualReportService.ts:313-343`
- Required: `column`；`first_ts`（或 firstTs/min_ts/minTs）；`last_ts`（lastTs/max_ts/maxTs）
- Optional: 无
- Notes: `column` 与调用方期望列不一致时结果会被丢弃（`annualReportService.ts:330-332`）。
### `wcdb_list_tables`
- C sig: `int32 wcdb_list_tables(int64 handle, const char* kind, const char* dbPath, _Out_ void** outJson)` — `wcdbCore.ts:1304`
- Return: `0` 成功
- outJson: `["contact","biz_info","stranger"]`
- Consumer: `backupService.ts:775-790`
- Required: 字符串数组
- Optional: 无
- Notes: `kind` 由调用方传入（`contact/message/media` 等），`dbPath` 为空时表示默认库。
### `wcdb_get_table_schema`
- C sig: `int32 wcdb_get_table_schema(int64 handle, const char* kind, const char* dbPath, const char* tableName, _Out_ void** outJson)` — `wcdbCore.ts:1309`
- Return: `0` 成功（`wcdbCore.ts:3647-3660`）
- outJson: `{"schema":"CREATE TABLE ..."}`
- Consumer: `backupService.ts:782-790`、`1113-1119`
- Required: `schema` 为完整 `CREATE TABLE` SQL
- Optional: `success:false` + `error` 表示失败
- Notes: 该 SQL 会在恢复时原样传给 `import_table_snapshot_with_schema`。
### `wcdb_export_table_snapshot`
- C sig: `int32 wcdb_export_table_snapshot(int64 handle, const char* kind, const char* dbPath, const char* tableName, const char* outputPath, _Out_ void** outJson)` — `wcdbCore.ts:1314`
- Return: `0` 且 `data.success !== false`（`wcdbCore.ts:3662-3675`）
- outJson: `{"success":true,"rows":100,"columns":8}`
- Consumer: `backupService.ts:805-820`
- Required: `rows`、`columns`；文件写到 `outputPath`
- Optional: `error`
- Notes: 文件格式（`.wfsnap`）对 JS 完全不透明——消费者从不读取内容（`backupService.ts:798-810`）；导出/导入必须自洽，具体格式未确定。
### `wcdb_import_table_snapshot` / `_with_schema`
- C sig: `int32 wcdb_import_table_snapshot(int64, const char* kind, const char* dbPath, const char* tableName, const char* inputPath, _Out_ void** outJson)` / `int32 wcdb_import_table_snapshot_with_schema(int64, ..., const char* createTableSql, _Out_ void** outJson)` — `wcdbCore.ts:1319,1324`
- Return: `0` 且 `data.success !== false`（`wcdbCore.ts:3677-3719`）
- outJson: `{"success":true,"rows":100,"inserted":98,"ignored":1,"malformed":1,"columns":8}`
- Consumer: `backupService.ts:1113-1130`
- Required: `inserted`、`ignored`（`malformed`/`columns`/`rows` 用于统计）
- Optional: `error`
- Notes: `_with_schema` 必须先按 `createTableSql` 建表（目标表不存在时）；导入应做冲突忽略而不是覆盖。
### `wcdb_exec_query`
- C sig: `int32 wcdb_exec_query(int64 handle, const char* kind, const char* path, const char* sql, _Out_ void** outJson)` — `wcdbCore.ts:1169`
- Return: `0` 成功；行结果必须是数组（`wcdbCore.ts:4282-4290`）
- outJson: `[{"username":"wxid_a","type":1}]`（对象数组，一行一个对象）
- Consumer: `chatService.ts:4047,4081,8228,10448`、`bizService.ts:203`、`backupService.ts` 等
- Required: `kind` 小写化后可能是 `contact|message|media|sns`；`path` 可为空串；`sql` 为单条 SELECT/PRAGMA
- Optional: 写语句不在消费者使用范围内
- Notes: `kind==='contact'` 且 `path` 为空时，core 会替换为 `<db_storage>/Contact/contact.db`（`wcdbCore.ts:4255-4265`）；参数化 `params` 未实现（`wcdbCore.ts:4247-4253`），SQL 已由调用方转义。
### `wcdb_get_db_status`
- C sig: `int32 wcdb_get_db_status(int64 handle, _Out_ void** outJson)` — `wcdbCore.ts:1201`
- Return: `0` 成功；失败只写日志（`wcdbCore.ts:522-533`）
- outJson: 未确定（建议 `{"handles":n,"dbs":[{"path":"...","open":true}]}`）
- Consumer: `wcdbCore.ts:512-535`（仅诊断日志）
- Required: 任意合法 JSON 对象
- Optional: 未确定
- Notes: 无业务字段约束，可自由扩展。
## 13. L. 杂项
### `wcdb_get_logs`
- C sig: `int32 wcdb_get_logs(_Out_ void** outJson)` — `wcdbCore.ts:1125`（无 handle）
- Return: `0` 成功
- outJson: `["[12:00:00] init ok","..."]`
- Consumer: `wcdbCore.ts:4217-4232`、`printLogs`
- Required: 字符串数组
- Optional: 可为空数组
- Notes: 可选导出；不存在时不报错，只是没有日志（`scripts/check-wcdb-optional-exports.cjs:224-229`）。
### `wcdb_start_monitor_pipe` / `wcdb_stop_monitor_pipe` / `wcdb_get_monitor_pipe_name`
- C sig: `int32 wcdb_start_monitor_pipe()` / `void wcdb_stop_monitor_pipe()` / `int32 wcdb_get_monitor_pipe_name(_Out_ void** outName)` — `wcdbCore.ts:1405,1406,1407`
- Return: start `0` 成功；pipe name `0` 且 out 为字符串（`wcdbCore.ts:246-263`）
- outJson: 管道名纯字符串，例如 `\\.\pipe\weflow_monitor_1234`
- Consumer: `wcdbCore.ts:226-376`、`chatService.ts:768-789`
- Required: Windows 命名管道；消息为换行或 NUL 分隔的 JSON，每行含 `action` 字段
- Optional: 不提供 pipe name 时默认为 `\\.\pipe\weflow_monitor`（`wcdbCore.ts:253`）
- Notes: 消费者把每行 `action || 'update'` 作为 type（`wcdbCore.ts:319-322`），JSON 内的表名/会话 id 用于失效缓存（`chatService.ts:5303-5350`）。
### `wcdb_cloud_init` / `wcdb_cloud_report` / `wcdb_cloud_stop`
- C sig: `int32 wcdb_cloud_init(int32 intervalSeconds)` / `int32 wcdb_cloud_report(const char* statsJson)` / `void wcdb_cloud_stop()` — `wcdbCore.ts:1425,1432,1439`
- Return: 非 0 失败（`wcdbCore.ts:4738-4785`）
- outJson: 无
- Consumer: `cloudControlService.ts:38-40,104-116`
- Required: `cloud_report` 接收 UTF-8 JSON 数组（每项 `{appVersion,platform,deviceId,timestamp,online,pages}`）
- Optional: 无
- Notes: 上报失败会触发指数退避与熔断（`cloudControlService.ts:100-125`）；`intervalSeconds` 消费者传 300。
### `VerifyUser`
- C sig: `void VerifyUser(int64 hwnd, const char* message, _Out_ char* outResult, int maxLen)` — `wcdbCore.ts:1418`
- Return: 无返回值
- outJson: 固定缓冲区内的 JSON：`{"success":true}`
- Consumer: `wcdbCore.ts:4787-4812`
- Required: 写入 NUL 结尾字符串且不超过 `maxLen=1024`
- Optional: 失败时 `{"success":false,"error":"..."}`
- Notes: 缓冲区由调用方 `Buffer.alloc`，DLL 只写不分配；解析失败会返回 `{success:false,error}`。
## 14. 附录：实现优先级与硬约束清单
### 14.1 最小可用路径

1. `wcdb_init` → `wcdb_open_account` → `wcdb_set_my_account_id` → `wcdb_get_sessions`
   → `wcdb_get_messages` / 游标三件套 → `wcdb_get_message_count` → `wcdb_exec_query`。
2. 统计/报告：`get_aggregate_stats`（camelCase）、`get_available_years`、`get_annual_report_*`。
3. 资源页：`scan_media_stream` + `list_media_dbs` + 两个 hardlink 解析。
4. 导出/备份：`get_message_table_stats`、`get_message_table_columns`、`list_tables`、
   `get_table_schema`、`export/import_table_snapshot`。
5. 朋友圈：`get_sns_timeline`（严格分页）、`get_sns_export_stats`、`get_sns_annual_stats`。
### 14.2 必须精确匹配的 JSON 形状

- 会话：数组 + `username` + `sort_timestamp`（`chatService.ts:1244-1302`）。
- 消息：`local_type`、`computed_is_send/is_send`、`create_time`、`local_id`、`server_id`、
  `sort_seq`、`sender_username`（`chatService.ts:6424-6450`）。
- 聚合：camelCase `total/typeCounts/hourly/sessions`，且 `total>0`（`analyticsService.ts:468-539`）。
- 联系人状态：`{username:{isFolded,isMuted}}`（`chatService.ts:1993-1997`）。
- 日消息：`{YYYY-MM-DD:count}`（`wcdbCore.ts:2446-2466`）。
- SNS 时间线：数组且 `limit/offset` 精确（`snsService.ts:640-699`）。
### 14.3 错误码约定（来自 `wcdbCore.ts:414-448`）

| 码 | 含义 |
|---|---|
| `-1` | 未配置/找不到实现 |
| `-3` | 找不到 session.db（游标调用中表示无消息库） |
| `-4` | session.db 无法打开 |
| `-5` | schema 不可读 |
| `-6` | 无兼容会话表 |
| `-7` | 无排序时间字段；消息接口中表示 message schema mismatch |
| `-8` | 无最后消息时间字段 |
| `-9` | 排序/最后消息字段冲突 |
| `-10` | 读取会话数量失败 |
| `-11` | 回写排序时间失败并回滚 |
| `-101` / `-1000` / `-1005..-1010` | 环境异常 |
| `-102` | 系统时间回拨 |
| `-103` | 完整性校验失败 |
| `-104` | 调试/分析环境 |
| `-105` | 防回拨状态损坏 |
| `-2301` / `-2302` / `-2303` | 调用异常 / 初始化异常 / 初始化失败 |
| `-3001` / `-3002` / `-3003` / `-3004` | 无 db_storage / 无 session.db / handle 无效 / 恢复失败 |
| `-2212..-2201` | 视为最轻的 InitProtection 失败 |
### 14.4 未确定项汇总

- 触发器内部 SQL、快照文件二进制/文本格式、`get_db_status`/`get_media_schema_summary`/
  `get_message_meta`/`ai_query_session_candidates` 的完整字段：仓库内无消费者读取，`未确定`。
- `wcdb_get_messages` 的默认排序方向与 offset 语义：仓库只传参不校验，`未确定`；
  建议 `create_time DESC, sort_seq DESC, local_id DESC`。
- `get_dual_report_stats` 的 `initiative/response/monthly/streak` 子结构：消费者原样透传，`未确定`。
- `VerifyUser` 的 `message` 语义与 `hwnd` 用途：仅透传，`未确定`。
