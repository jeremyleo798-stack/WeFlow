# WeFlow HTTP API / Push 文档

WeFlow 提供本地 HTTP API（已支持GET 和 POST请求），便于外部脚本或工具读取聊天记录、会话、联系人、群成员和导出的媒体文件；也支持在检测到新消息后通过固定 SSE 地址主动推送消息事件。

## 启用方式

在应用设置页启用 `API 服务`。

- 默认监听地址：`127.0.0.1`
- 默认端口：`5031`
- 基础地址：`http://127.0.0.1:5031`
- 可选开启 `主动推送`，检测到新收到的消息后会通过 `GET /api/v1/push/messages` 推送给 SSE 订阅端

**状态记忆**：API 服务和主动推送的状态及端口会自动保存，重启 WeFlow 后会自动恢复运行。

## 鉴权规范

**鉴权规范 (Access Token)** 除健康检查接口外，所有 `/api/v1/*` 接口均受 Token 保护。支持三种传参方式（任选其一）：

1. **HTTP Header (推荐)**: `Authorization: Bearer <您的Token>`
2. **Query 参数**: `?access_token=<您的Token>`（SSE 长连接推荐此方式）
3. **JSON Body**: `{"access_token": "<您的Token>"}`（仅限 POST 请求）

## 接口列表

- `GET|POST /health`
- `GET|POST /api/v1/health`
- `GET|POST /api/v1/push/messages`
- `GET|POST /api/v1/messages`
- `GET|POST /api/v1/sessions`
- `GET /api/v1/sessions/:id/messages` (ChatLab Pull)
- `GET|POST /api/v1/contacts`
- `GET|POST /api/v1/group-members`
- `GET|POST /api/v1/media/*`

---

## 1. 健康检查

**请求**

```http
GET /health
```

或

```http
GET /api/v1/health
```

**响应**

```json
{
  "status": "ok"
}
```

---

## 2. 主动推送

通过 SSE 长连接接收新消息事件，端口与 HTTP API 共用。

**请求**

```http
GET /api/v1/push/messages
```

### 说明

- 需要先在设置页开启 `HTTP API 服务`
- 同时需要开启 `主动推送`
- 响应类型为 `text/event-stream`
- 事件名包含 `message.new` 和 `message.revoke`
- 建议接收端按 `event + rawid` 去重

### 事件字段

- `event`
- `sessionId`
- `rawid`
- `avatarUrl`
- `sourceName`
- `groupName`（仅群聊）
- `content`
- `timestamp`（消息时间，秒级 Unix 时间戳）

### 示例

```bash
curl -N "http://127.0.0.1:5031/api/v1/push/messages?access_token=YOUR_TOKEN
```

示例事件：

```text
event: message.new
data: {"event":"message.new","sessionId":"xxx@chatroom","sessionType":"group","rawid":"1234567890123456789","avatarUrl":"https://example.com/group.jpg","sourceName":"李四","groupName":"项目群","content":"[图片]","timestamp":1760000123}
```

撤回事件示例：

```text
event: message.revoke
data: {"event":"message.revoke","sessionId":"wxid_xxx","sessionType":"other","rawid":"1234567890123456789","avatarUrl":"https://example.com/avatar.jpg","sourceName":"张三","content":"对方撤回了一条消息（rawid：1234567890123456789） 内容为“你好”","timestamp":1760000180}
```

---

## 3. 获取消息

> 当使用 POST 时，请将参数放在 JSON Body 中（Content-Type: application/json）

读取指定会话的消息，支持原始 JSON 和 ChatLab 格式。

**请求**

```http
GET /api/v1/messages
```

### 参数

| 参数      | 类型   | 必填 | 说明                                                  |
| --------- | ------ | ---- | ----------------------------------------------------- |
| `talker`  | string | 是   | 会话 ID。私聊通常是对方 `wxid`，群聊是 `xxx@chatroom` |
| `limit`   | number | 否   | 返回条数，默认 `100`，范围 `1~10000`                  |
| `offset`  | number | 否   | 分页偏移，默认 `0`                                    |
| `start`   | string | 否   | 开始时间，支持 `YYYYMMDD` 或时间戳                    |
| `end`     | string | 否   | 结束时间，支持 `YYYYMMDD` 或时间戳                    |
| `keyword` | string | 否   | 基于消息显示文本过滤                                  |
| `chatlab` | string | 否   | `1/true` 时输出 ChatLab 格式                          |
| `format`  | string | 否   | `json` 或 `chatlab`                                   |
| `media`   | string | 否   | `1/true` 时导出媒体并返回媒体地址，兼容别名 `meiti`   |
| `image`   | string | 否   | 在 `media=1` 时控制图片导出，兼容别名 `tupian`        |
| `voice`   | string | 否   | 在 `media=1` 时控制语音导出，兼容别名 `vioce`         |
| `video`   | string | 否   | 在 `media=1` 时控制视频导出                           |
| `emoji`   | string | 否   | 在 `media=1` 时控制表情导出                           |

### 示例

```bash
curl "http://127.0.0.1:5031/api/v1/messages?talker=wxid_xxx&limit=20"
curl "http://127.0.0.1:5031/api/v1/messages?talker=xxx@chatroom&chatlab=1"
curl "http://127.0.0.1:5031/api/v1/messages?talker=wxid_xxx&start=20260101&end=20260131"
curl "http://127.0.0.1:5031/api/v1/messages?talker=xxx@chatroom&media=1&image=1&voice=0&video=0&emoji=0"
```

### JSON 响应字段

顶层字段：

- `success`
- `talker`
- `count`
- `hasMore`
- `media.enabled`
- `media.exportPath`
- `media.count`
- `messages`

单条消息字段：

- `localId`
- `serverId`
- `localType`
- `createTime`
- `isSend`
- `senderUsername`
- `content`
- `rawContent`
- `parsedContent`
- `replyToMessageId`（引用回复目标消息的 `serverId`，仅引用消息返回）
- `quote`（引用消息快照，包含被引用消息的 ID、发送者、内容和类型）
- `mediaType`
- `mediaFileName`
- `mediaUrl`
- `mediaLocalPath`

**示例响应**

```json
{
  "success": true,
  "talker": "xxx@chatroom",
  "count": 3,
  "hasMore": true,
  "media": {
    "enabled": true,
    "exportPath": "C:\\Users\\Alice\\Documents\\WeFlow\\api-media",
    "count": 1
  },
  "messages": [
    {
      "localId": 123,
      "serverId": "6116895530414915131",
      "localType": 1,
      "createTime": 1738713600,
      "isSend": 0,
      "senderUsername": "wxid_member",
      "content": "你好",
      "rawContent": "你好",
      "parsedContent": "你好"
    },
    {
      "localId": 125,
      "serverId": "6116895530414915133",
      "localType": 244813135921,
      "createTime": 1738713700,
      "isSend": 0,
      "senderUsername": "wxid_member",
      "content": "收到",
      "rawContent": "<msg>...</msg>",
      "parsedContent": "收到",
      "replyToMessageId": "6116895530414915131",
      "quote": {
        "platformMessageId": "6116895530414915131",
        "sender": "wxid_other",
        "accountName": "张三",
        "content": "你好",
        "type": 0
      }
    },
    {
      "localId": 124,
      "localType": 3,
      "createTime": 1738713660,
      "isSend": 0,
      "senderUsername": "wxid_member",
      "content": "[图片]",
      "mediaType": "image",
      "mediaFileName": "abc123.jpg",
      "mediaUrl": "http://127.0.0.1:5031/api/v1/media/xxx@chatroom/images/abc123.jpg",
      "mediaLocalPath": "C:\\Users\\Alice\\Documents\\WeFlow\\api-media\\xxx@chatroom\\images\\abc123.jpg"
    }
  ]
}
```

### ChatLab 响应

当 `chatlab=1` 或 `format=chatlab` 时，返回 ChatLab 结构：

- `chatlab.version`
- `chatlab.exportedAt`
- `chatlab.generator`
- `meta.name`
- `meta.platform`
- `meta.type`
- `meta.groupId`
- `meta.groupAvatar`
- `meta.ownerId`
- `members[].platformId`
- `members[].accountName`
- `members[].groupNickname`
- `members[].avatar`
- `messages[].sender`
- `messages[].accountName`
- `messages[].groupNickname`
- `messages[].timestamp`
- `messages[].type`
- `messages[].content`
- `messages[].platformMessageId`
- `messages[].replyToMessageId`
- `messages[].mediaPath`

群聊里 `groupNickname` 会优先来自群成员群昵称；若源数据缺失，则回退为空或展示名。

---

## 4. 获取会话列表

> 当使用 POST 时，请将参数放在 JSON Body 中（Content-Type: application/json）

**请求**

```http
GET /api/v1/sessions
```

### 参数

| 参数      | 类型   | 必填 | 说明                             |
| --------- | ------ | ---- | -------------------------------- |
| `keyword` | string | 否   | 匹配 `username` 或 `displayName` |
| `limit`   | number | 否   | 默认 `100`                       |

### 响应字段

- `success`
- `count`
- `sessions[].username`
- `sessions[].displayName`
- `sessions[].type`
- `sessions[].lastTimestamp`
- `sessions[].unreadCount`

**示例响应**

```json
{
  "success": true,
  "count": 1,
  "sessions": [
    {
      "username": "xxx@chatroom",
      "displayName": "项目群",
      "type": 2,
      "lastTimestamp": 1738713600,
      "unreadCount": 0
    }
  ]
}
```

---

## 4.1 获取会话列表（ChatLab 格式）

当 `format=chatlab` 时，返回 ChatLab Pull 协议兼容格式，可直接作为 ChatLab 远程数据源。

**请求**

```http
GET /api/v1/sessions?format=chatlab
```

### 参数

| 参数      | 类型   | 必填 | 说明                             |
| --------- | ------ | ---- | -------------------------------- |
| `format`  | string | 是   | 设为 `chatlab`                   |
| `keyword` | string | 否   | 匹配 `username` 或 `displayName` |
| `limit`   | number | 否   | 默认 `100`                       |

### 响应

```json
{
  "sessions": [
    {
      "id": "xxx@chatroom",
      "name": "项目群",
      "platform": "im",
      "type": "group",
      "messageCount": 58000,
      "lastMessageAt": 1738713600
    }
  ]
}
```

| 字段            | 说明                                |
| --------------- | ----------------------------------- |
| `id`            | 会话 ID（username）            |
| `name`          | 会话显示名称                        |
| `platform`      | 固定 `im`                       |
| `type`          | `group`（群聊）或 `private`（私聊） |
| `messageCount`  | 消息数量（估算值，可能不精确）      |
| `lastMessageAt` | 最后消息的秒级 Unix 时间戳          |

---

## 4.2 拉取会话消息（ChatLab Pull）

返回 ChatLab 标准格式的聊天数据，支持增量拉取和分页。

**请求**

```http
GET /api/v1/sessions/:id/messages
```

### 参数

| 参数     | 类型   | 必填 | 说明                                     |
| -------- | ------ | ---- | ---------------------------------------- |
| `:id`    | string | 是   | 会话 ID（Path 参数）                     |
| `since`  | number | 否   | 秒级 Unix 时间戳，仅返回此时间之后的消息 |
| `end`    | number | 否   | 秒级 Unix 时间戳，时间上界               |
| `limit`  | number | 否   | 单次返回上限，默认且最大 `5000`          |
| `offset` | number | 否   | 分页偏移，默认 `0`                       |

### 响应

返回 ChatLab 标准 JSON 格式，外加 `sync` 分页块：

```json
{
  "chatlab": {
    "version": "0.0.2",
    "exportedAt": 1738713600,
    "generator": "WeFlow"
  },
  "meta": {
    "name": "项目群",
    "platform": "im",
    "type": "group",
    "groupId": "xxx@chatroom",
    "ownerId": "wxid_xxx"
  },
  "members": [
    {
      "platformId": "wxid_a",
      "accountName": "张三",
      "groupNickname": "产品",
      "avatar": "https://example.com/avatar.jpg"
    }
  ],
  "messages": [
    {
      "sender": "wxid_a",
      "accountName": "张三",
      "timestamp": 1738713600,
      "type": 0,
      "content": "你好",
      "platformMessageId": "123456"
    }
  ],
  "sync": {
    "hasMore": true,
    "nextSince": 1738713600,
    "nextOffset": 5000,
    "watermark": 1738714000
  }
}
```

### sync 块

| 字段         | 说明                             |
| ------------ | -------------------------------- |
| `hasMore`    | 是否还有更多数据                 |
| `nextSince`  | 下次请求的 `since` 值            |
| `nextOffset` | 下次请求的 `offset` 值           |
| `watermark`  | 本次拉取的时间上界（秒级时间戳） |

**ChatLab 对接方式**：在 ChatLab 设置中添加远程数据源，`baseUrl` 填 `http://127.0.0.1:5031/api/v1`，Token 填 WeFlow 中配置的 API Token。

---

## 5. 获取联系人列表

> 当使用 POST 时，请将参数放在 JSON Body 中（Content-Type: application/json）

**请求**

```http
GET /api/v1/contacts
```

### 参数

| 参数      | 类型   | 必填 | 说明                                                 |
| --------- | ------ | ---- | ---------------------------------------------------- |
| `keyword` | string | 否   | 匹配 `username`、`nickname`、`remark`、`displayName` |
| `limit`   | number | 否   | 默认 `100`                                           |

### 响应字段

- `success`
- `count`
- `contacts[].username`
- `contacts[].displayName`
- `contacts[].remark`
- `contacts[].nickname`
- `contacts[].alias`
- `contacts[].avatarUrl`
- `contacts[].type`

**示例响应**

```json
{
  "success": true,
  "count": 1,
  "contacts": [
    {
      "username": "wxid_xxx",
      "displayName": "张三",
      "remark": "客户张三",
      "nickname": "张三",
      "alias": "zhangsan",
      "avatarUrl": "https://example.com/avatar.jpg",
      "type": "friend"
    }
  ]
}
```

---

## 6. 获取群成员列表

> 当使用 POST 时，请将参数放在 JSON Body 中（Content-Type: application/json）

返回群成员的 `accountId`、群昵称、备注、账号号等信息。

**请求**

```http
GET /api/v1/group-members
```

### 参数

| 参数                   | 类型   | 必填 | 说明                            |
| ---------------------- | ------ | ---- | ------------------------------- |
| `chatroomId`           | string | 是   | 群 ID，兼容使用 `talker` 传入   |
| `includeMessageCounts` | string | 否   | `1/true` 时附带成员发言数       |
| `withCounts`           | string | 否   | `includeMessageCounts` 的别名   |
| `forceRefresh`         | string | 否   | `1/true` 时跳过内存缓存强制刷新 |

### 响应字段

- `success`
- `chatroomId`
- `count`
- `fromCache`
- `updatedAt`
- `members[].wxid`
- `members[].displayName`
- `members[].nickname`
- `members[].remark`
- `members[].alias`
- `members[].groupNickname`
- `members[].avatarUrl`
- `members[].isOwner`
- `members[].isFriend`
- `members[].messageCount`

**示例请求**

```bash
curl "http://127.0.0.1:5031/api/v1/group-members?chatroomId=xxx@chatroom"
curl "http://127.0.0.1:5031/api/v1/group-members?chatroomId=xxx@chatroom&includeMessageCounts=1&forceRefresh=1"
```

**示例响应**

```json
{
  "success": true,
  "chatroomId": "xxx@chatroom",
  "count": 2,
  "fromCache": false,
  "updatedAt": 1760000000000,
  "members": [
    {
      "wxid": "wxid_member_a",
      "displayName": "客户A",
      "nickname": "阿甲",
      "remark": "客户A",
      "alias": "kehua",
      "groupNickname": "甲方",
      "avatarUrl": "https://example.com/a.jpg",
      "isOwner": true,
      "isFriend": true,
      "messageCount": 128
    },
    {
      "wxid": "wxid_member_b",
      "displayName": "李四",
      "nickname": "李四",
      "remark": "",
      "alias": "",
      "groupNickname": "",
      "avatarUrl": "",
      "isOwner": false,
      "isFriend": false,
      "messageCount": 0
    }
  ]
}
```

说明：

- `displayName` 是当前应用内的主展示名。
- `groupNickname` 是成员在该群里的群昵称。
- `remark` 是你对该联系人的备注。
- `alias` 是账号号。
- 当源数据里没有群昵称时，`groupNickname` 会为空。

---

## 7. 朋友圈接口

### 7.1 获取朋友圈时间线

```http
GET /api/v1/sns/timeline
```

参数：

| 参数        | 类型   | 必填 | 说明                                                         |
| ----------- | ------ | ---- | ------------------------------------------------------------ |
| `limit`     | number | 否   | 返回数量，默认 20，范围 `1~200`                              |
| `offset`    | number | 否   | 偏移量，默认 0                                               |
| `usernames` | string | 否   | 发布者过滤，逗号分隔，如 `wxid_a,wxid_b`                     |
| `keyword`   | string | 否   | 关键词过滤（正文）                                           |
| `start`     | string | 否   | 开始时间，支持 `YYYYMMDD` 或秒/毫秒时间戳                    |
| `end`       | string | 否   | 结束时间，支持 `YYYYMMDD` 或秒/毫秒时间戳                    |
| `media`     | number | 否   | 是否返回可直接访问的媒体地址，默认 `1`                       |
| `replace`   | number | 否   | `media=1` 时，是否用解析地址覆盖 `media.url/thumb`，默认 `1` |
| `inline`    | number | 否   | `media=1` 时，是否内联返回 `data:` URL，默认 `0`             |

示例：

```bash
curl "http://127.0.0.1:5031/api/v1/sns/timeline?limit=5"
curl "http://127.0.0.1:5031/api/v1/sns/timeline?usernames=wxid_a,wxid_b&keyword=旅行"
curl "http://127.0.0.1:5031/api/v1/sns/timeline?limit=3&media=1&replace=1"
curl "http://127.0.0.1:5031/api/v1/sns/timeline?limit=3&media=1&inline=1"
```

媒体字段说明（`media=1`）：

- `media[].url/thumb`：你应该优先直接使用的字段。
- `replace=1`（默认）时，`media[].url/thumb` 会直接被替换成可访问地址，等价于 `resolvedUrl/resolvedThumbUrl`。
- `replace=0` 时，`media[].url/thumb` 仍保留原始地址；这时再结合下面的 `raw/proxy/resolved` 字段自己决定用哪一个。
- `media[].rawUrl/rawThumb`：原始朋友圈地址
- `media[].proxyUrl/proxyThumbUrl`：可直接访问的代理地址
- `media[].resolvedUrl/resolvedThumbUrl`：最终可用地址（`inline=1` 时可能是 `data:` URL）
- `media[].token/key/encIdx`：源数据里的访问/解密参数。通常不需要你自己处理；如果你手动调用 `/api/v1/sns/media/proxy`，把当前条目的 `url` 和 `key` 原样传回即可。
- `media[].livePhoto`：实况图的视频部分。外层 `media[].url/thumb` 仍是封面图，`livePhoto` 内部会再提供一组自己的 `url/thumb/raw*/proxy*/resolved*` 字段。
- `media=0` 时，不会补充 `raw*/proxy*/resolved*`，接口只返回原始 `url/thumb` 以及源字段（如 `key/token/encIdx`）。

### 7.2 获取朋友圈发布者

```http
GET /api/v1/sns/usernames
```

### 7.3 获取朋友圈导出统计

```http
GET /api/v1/sns/export/stats
```

参数：

| 参数   | 类型   | 必填 | 说明                         |
| ------ | ------ | ---- | ---------------------------- |
| `fast` | number | 否   | `1` 使用快速统计（优先缓存） |

### 7.4 朋友圈媒体代理

```http
GET /api/v1/sns/media/proxy
```

参数：

| 参数  | 类型          | 必填 | 说明                     |
| ----- | ------------- | ---- | ------------------------ |
| `url` | string        | 是   | 媒体原始 URL             |
| `key` | string/number | 否   | 解密 key（部分资源需要） |

### 7.5 导出朋友圈

```http
POST /api/v1/sns/export
Content-Type: application/json
```

Body 示例：

```json
{
  "outputDir": "C:\\Users\\Alice\\Desktop\\sns-export",
  "format": "json",
  "usernames": "wxid_a,wxid_b",
  "keyword": "旅行",
  "exportMedia": true,
  "exportImages": true,
  "exportLivePhotos": true,
  "exportVideos": true,
  "start": "20250101",
  "end": "20251231"
}
```

`format` 支持：`json`、`html`、`arkmejson`（兼容写法：`arkme-json`）、`markdown`（兼容写法：`md`）。

### 7.6 朋友圈防删开关

```http
GET  /api/v1/sns/block-delete/status
POST /api/v1/sns/block-delete/install
POST /api/v1/sns/block-delete/uninstall
```

### 7.7 删除单条朋友圈

```http
DELETE /api/v1/sns/post/{postId}
```

---

## 8. 访问导出媒体

> 当使用 POST 时，请将参数放在 JSON Body 中（Content-Type: application/json）

通过消息接口启用 `media=1` 后，接口会先把图片、语音、视频、表情导出到本地缓存目录，再返回可访问的 HTTP 地址。

**请求**

```http
GET /api/v1/media/{relativePath}
```

### 示例

```bash
curl "http://127.0.0.1:5031/api/v1/media/xxx@chatroom/images/abc123.jpg"
curl "http://127.0.0.1:5031/api/v1/media/xxx@chatroom/voices/voice_100.wav"
curl "http://127.0.0.1:5031/api/v1/media/xxx@chatroom/videos/video_200.mp4"
curl "http://127.0.0.1:5031/api/v1/media/xxx@chatroom/emojis/emoji_300.gif"
```

### 支持的 Content-Type

| 扩展名           | Content-Type |
| ---------------- | ------------ |
| `.png`           | `image/png`  |
| `.jpg` / `.jpeg` | `image/jpeg` |
| `.gif`           | `image/gif`  |
| `.webp`          | `image/webp` |
| `.wav`           | `audio/wav`  |
| `.mp3`           | `audio/mpeg` |
| `.mp4`           | `video/mp4`  |

常见错误响应：

```json
{
  "error": "Media not found"
}
```

---

## 9. 使用示例

### PowerShell

```powershell
$headers = @{ "Authorization" = "Bearer YOUR_TOKEN" }
$body = @{ talker = "wxid_xxx"; limit = 10 } | ConvertTo-Json

Invoke-RestMethod -Uri "http://127.0.0.1:5031/api/v1/messages" -Method POST -Headers $headers -Body $body -ContentType "application/json"
```

### cURL

```bash
# GET 带 Token Header
curl -H "Authorization: Bearer YOUR_TOKEN" "http://127.0.0.1:5031/api/v1/messages?talker=wxid_xxx"

# POST 带 JSON Body
curl -X POST http://127.0.0.1:5031/api/v1/messages \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"talker": "xxx@chatroom", "chatlab": true}'
```

### Python

```python
import requests

BASE_URL = "http://127.0.0.1:5031"
headers = {"Authorization": "Bearer YOUR_TOKEN", "Content-Type": "application/json"}

# POST 方式获取消息
messages = requests.post(
    f"{BASE_URL}/api/v1/messages",
    json={"talker": "xxx@chatroom", "limit": 50},
    headers=headers
).json()

# GET 方式获取群成员
members = requests.get(
    f"{BASE_URL}/api/v1/group-members",
    params={"chatroomId": "xxx@chatroom", "includeMessageCounts": 1},
    headers=headers
).json()
```

### Node.js

```javascript
const axios = require('axios');

const BASE_URL = 'http://127.0.0.1:5031';
const headers = {
  'Authorization': 'Bearer YOUR_TOKEN',
  'Content-Type': 'application/json'
};

// 获取消息
async function getMessages() {
  const response = await axios.post(`${BASE_URL}/api/v1/messages`, {
    talker: 'xxx@chatroom',
    limit: 50,
    media: true,
    image: true
  }, { headers });
  return response.data;
}

// 获取会话列表
async function getSessions() {
  const response = await axios.get(`${BASE_URL}/api/v1/sessions`, {
    params: { limit: 100 },
    headers
  });
  return response.data;
}

// 获取群成员
async function getGroupMembers(chatroomId) {
  const response = await axios.get(`${BASE_URL}/api/v1/group-members`, {
    params: {
      chatroomId,
      includeMessageCounts: 1,
      forceRefresh: 0
    },
    headers
  });
  return response.data;
}
```

### SSE 消息推送订阅

```python
import requests
import json

BASE_URL = "http://127.0.0.1:5031"
TOKEN = "YOUR_TOKEN"

# 订阅消息推送（SSE）
def subscribe_message_push():
    url = f"{BASE_URL}/api/v1/push/messages?access_token={TOKEN}"
    
    with requests.get(url, stream=True) as response:
        response.raise_for_status()
        
        for line in response.iter_lines():
            if not line:
                continue
            
            line_str = line.decode('utf-8')
            
            # 解析 SSE 事件
            if line_str.startswith('event:'):
                event_type = line_str[6:].strip()
            elif line_str.startswith('data:'):
                data = json.loads(line_str[5:].strip())
                print(f"[{event_type}] {data}")
                
                # 处理新消息
                if event_type == 'message.new':
                    print(f"新消息: {data.get('content')}")
                elif event_type == 'message.revoke':
                    print(f"撤回消息: {data.get('rawid')}")
```

```javascript
// Node.js SSE 订阅
const EventSource = require('eventsource');

const BASE_URL = 'http://127.0.0.1:5031';
const TOKEN = 'YOUR_TOKEN';

const es = new EventSource(`${BASE_URL}/api/v1/push/messages?access_token=${TOKEN}`);

es.addEventListener('message.new', (event) => {
  const data = JSON.parse(event.data);
  console.log('新消息:', data.content);
});

es.addEventListener('message.revoke', (event) => {
  const data = JSON.parse(event.data);
  console.log('撤回消息:', data.rawid);
});

es.onerror = (error) => {
  console.error('SSE 连接错误:', error);
};
```

### 朋友圈接口示例

```python
import requests

BASE_URL = "http://127.0.0.1:5031"
headers = {"Authorization": "Bearer YOUR_TOKEN"}

# 获取朋友圈时间线
timeline = requests.get(
    f"{BASE_URL}/api/v1/sns/timeline",
    params={
        "limit": 20,
        "offset": 0,
        "media": 1,
        "replace": 1
    },
    headers=headers
).json()

# 导出朋友圈
export_result = requests.post(
    f"{BASE_URL}/api/v1/sns/export",
    json={
        "outputDir": "C:\\Users\\Alice\\Desktop\\sns-export",
        "format": "html",
        "exportMedia": True,
        "exportImages": True,
        "exportLivePhotos": True,
        "start": "20250101",
        "end": "20251231"
    },
    headers=headers
).json()

# 获取朋友圈发布者
usernames = requests.get(
    f"{BASE_URL}/api/v1/sns/usernames",
    headers=headers
).json()
```

### ChatLab Pull 协议对接

```python
import requests

BASE_URL = "http://127.0.0.1:5031"
headers = {"Authorization": "Bearer YOUR_TOKEN"}

# 获取会话列表（ChatLab 格式）
sessions = requests.get(
    f"{BASE_URL}/api/v1/sessions",
    params={"format": "chatlab"},
    headers=headers
).json()

# 增量拉取会话消息
def pull_messages(session_id, since=None, limit=5000):
    params = {"limit": limit}
    if since:
        params["since"] = since
    
    response = requests.get(
        f"{BASE_URL}/api/v1/sessions/{session_id}/messages",
        params=params,
        headers=headers
    ).json()
    
    messages = response.get("messages", [])
    sync = response.get("sync", {})
    
    # 处理消息
    for msg in messages:
        print(f"[{msg['accountName']}] {msg['content']}")
    
    # 如果有更多数据，继续拉取
    if sync.get("hasMore"):
        return pull_messages(
            session_id,
            since=sync.get("nextSince"),
            limit=limit
        )
    
    return messages

# 拉取某个会话的所有消息
all_messages = pull_messages("xxx@chatroom")
```

---

## 10. 错误处理

### 错误响应格式

所有错误响应都遵循统一的 JSON 格式：

```json
{
  "error": "错误描述信息"
}
```

### 常见错误码

| HTTP 状态码 | 说明 | 可能原因 |
|------------|------|---------|
| `400` | Bad Request | 参数缺失或格式错误 |
| `401` | Unauthorized | Token 无效或缺失 |
| `404` | Not Found | 资源不存在（如会话 ID 错误） |
| `405` | Method Not Allowed | 请求方法不支持（如应该用 GET 却用了 POST） |
| `500` | Internal Server Error | 服务器内部错误（如数据库未连接） |

### 错误处理示例

```python
import requests

def safe_api_call(url, params=None, json_data=None, method='GET'):
    headers = {"Authorization": "Bearer YOUR_TOKEN"}
    
    try:
        if method == 'GET':
            response = requests.get(url, params=params, headers=headers)
        else:
            response = requests.post(url, json=json_data, headers=headers)
        
        # 检查 HTTP 错误
        response.raise_for_status()
        
        # 解析 JSON
        data = response.json()
        
        # 检查业务错误
        if not data.get('success', True):
            print(f"业务错误: {data.get('error')}")
            return None
        
        return data
    
    except requests.exceptions.HTTPError as e:
        if e.response.status_code == 401:
            print("认证失败：请检查 Token")
        elif e.response.status_code == 404:
            print("资源不存在：请检查参数")
        else:
            print(f"HTTP 错误: {e}")
    
    except requests.exceptions.ConnectionError:
        print("连接失败：请确保 WeFlow 正在运行且 API 服务已启动")
    
    except Exception as e:
        print(f"未知错误: {e}")
    
    return None
```

---

## 11. 常见问题（FAQ）

### Q1: 为什么返回 401 Unauthorized？

**A:** 可能的原因：
1. Token 未配置：在 WeFlow 设置页 → API 服务 → 生成 Token
2. Token 传递方式错误：确保使用 `Authorization: Bearer <token>` 或 `?access_token=<token>`
3. Token 已过期或被重置：重新生成 Token

### Q2: 为什么消息接口返回空数组？

**A:** 可能的原因：
1. 数据库未连接：先在 WeFlow 中完成数据库连接
2. 会话 ID 错误：检查 `talker` 参数是否正确（私聊是 wxid，群聊是 xxx@chatroom）
3. 时间范围错误：检查 `start` 和 `end` 参数格式

### Q3: 媒体文件无法访问？

**A:** 必须先在消息接口中启用 `media=1`，系统会导出媒体文件到本地缓存目录，然后才能通过 `/api/v1/media/*` 访问。

### Q4: 如何获取实时消息推送？

**A:** 
1. 在 WeFlow 设置页启用"API 服务"和"主动推送"
2. 使用 SSE 客户端订阅 `GET /api/v1/push/messages?access_token=YOUR_TOKEN`
3. 监听 `message.new` 和 `message.revoke` 事件

### Q5: ChatLab 格式与标准格式有什么区别？

**A:** 
- **标准格式**：WeFlow 原生格式，字段名如 `localId`、`serverId`、`localType`
- **ChatLab 格式**：ChatLab 协议标准格式，字段名如 `platformMessageId`、`sender`、`timestamp`，可直接作为 ChatLab 远程数据源

使用 `chatlab=1` 或 `format=chatlab` 参数切换格式。

---

## 12. 注意事项

1. API 仅监听本机 `127.0.0.1`，不对外网开放。
2. 使用前需要先在 WeFlow 中完成数据库连接。
3. `start` 和 `end` 支持 `YYYYMMDD` 与时间戳；纯 `YYYYMMDD` 的 `end` 会扩展到当天 `23:59:59`。
4. 群成员的 `groupNickname` 依赖源数据；源数据缺失时不会自动补出。
5. 媒体访问链接只有在对应消息已经通过 `media=1` 导出后才可访问。
