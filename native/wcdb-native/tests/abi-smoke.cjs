// Read-only ABI smoke test against a live WeChat 4.x account.
// Usage:
//   node native/wcdb-native/tests/abi-smoke.cjs <session.db> <keyHex> <accountId> [dll]
// Never performs write operations.

const path = require('path')
const koffi = require('koffi')

const sessionDb = process.argv[2]
const keyHex = process.argv[3]
const accountId = process.argv[4]
if (!sessionDb || !keyHex || !accountId) {
  console.error('Usage: node native/wcdb-native/tests/abi-smoke.cjs <session.db> <keyHex> <accountId> [dll]')
  process.exit(2)
}
const dllPath = process.argv[5] || path.join(__dirname, '..', 'dist', 'wcdb_api.dll')
const accountDir = path.resolve(path.dirname(sessionDb), '..', '..')

const lib = koffi.load(dllPath)

const F = {
  init: lib.func('int32 wcdb_init()'),
  shutdown: lib.func('int32 wcdb_shutdown()'),
  open: lib.func('int32 wcdb_open_account(const char* path, const char* key, _Out_ int64* handle)'),
  setAccount: lib.func('int32 wcdb_set_my_account_id(int64 handle, const char* accountId)'),
  free: lib.func('void wcdb_free_string(void* ptr)'),
  sessions: lib.func('int32 wcdb_get_sessions(int64 handle, _Out_ void** outJson)'),
  messages: lib.func('int32 wcdb_get_messages(int64 handle, const char* username, int32 limit, int32 offset, _Out_ void** outJson)'),
  messagesByType: lib.func('int32 wcdb_get_messages_by_type(int64 handle, const char* sessionId, int64 localType, int32 ascending, int32 limit, int32 offset, _Out_ void** outJson)'),
  messageCount: lib.func('int32 wcdb_get_message_count(int64 handle, const char* username, _Out_ int32* outCount)'),
  messageById: lib.func('int32 wcdb_get_message_by_id(int64 handle, const char* sessionId, int32 localId, _Out_ void** outJson)'),
  messageBySvrid: lib.func('int32 wcdb_get_message_by_svrid(int64 handle, const char* sessionId, const char* svrid, _Out_ void** outJson)'),
  sessionCounts: lib.func('int32 wcdb_get_session_message_counts(int64 handle, const char* sessionIdsJson, _Out_ void** outJson)'),
  typeStats: lib.func('int32 wcdb_get_session_message_type_stats(int64 handle, const char* sessionId, int32 beginTimestamp, int32 endTimestamp, _Out_ void** outJson)'),
  typeStatsBatch: lib.func('int32 wcdb_get_session_message_type_stats_batch(int64 handle, const char* sessionIdsJson, const char* optionsJson, _Out_ void** outJson)'),
  dateCounts: lib.func('int32 wcdb_get_session_message_date_counts(int64 handle, const char* sessionId, _Out_ void** outJson)'),
  dateCountsBatch: lib.func('int32 wcdb_get_session_message_date_counts_batch(int64 handle, const char* sessionIdsJson, _Out_ void** outJson)'),
  search: lib.func('int32 wcdb_search_messages(int64 handle, const char* sessionId, const char* keyword, int32 limit, int32 offset, int32 beginTimestamp, int32 endTimestamp, _Out_ void** outJson)'),
  openCursor: lib.func('int32 wcdb_open_message_cursor(int64 handle, const char* sessionId, int32 batchSize, int32 ascending, int32 beginTimestamp, int32 endTimestamp, _Out_ int64* outCursor)'),
  fetchBatch: lib.func('int32 wcdb_fetch_message_batch(int64 handle, int64 cursor, _Out_ void** outJson, _Out_ int32* outHasMore)'),
  closeCursor: lib.func('int32 wcdb_close_message_cursor(int64 handle, int64 cursor)'),
  setProjection: lib.func('int32 wcdb_set_message_cursor_projection(int64 handle, int64 cursor, int32 projection)'),
  contact: lib.func('int32 wcdb_get_contact(int64 handle, const char* username, _Out_ void** outJson)'),
  contactsCompact: lib.func('int32 wcdb_get_contacts_compact(int64 handle, const char* usernamesJson, _Out_ void** outJson)'),
  contactStatus: lib.func('int32 wcdb_get_contact_status(int64 handle, const char* usernamesJson, _Out_ void** outJson)'),
  typeCounts: lib.func('int32 wcdb_get_contact_type_counts(int64 handle, _Out_ void** outJson)'),
  aliasMap: lib.func('int32 wcdb_get_contact_alias_map(int64 handle, const char* usernamesJson, _Out_ void** outJson)'),
  friendFlags: lib.func('int32 wcdb_get_contact_friend_flags(int64 handle, const char* usernamesJson, _Out_ void** outJson)'),
  displayNames: lib.func('int32 wcdb_get_display_names(int64 handle, const char* usernamesJson, _Out_ void** outJson)'),
  avatarUrls: lib.func('int32 wcdb_get_avatar_urls(int64 handle, const char* usernamesJson, _Out_ void** outJson)'),
  extBuffer: lib.func('int32 wcdb_get_chat_room_ext_buffer(int64 handle, const char* chatroomId, _Out_ void** outJson)'),
  groupMembers: lib.func('int32 wcdb_get_group_members(int64 handle, const char* chatroomId, _Out_ void** outJson)'),
  groupNicknames: lib.func('int32 wcdb_get_group_nicknames(int64 handle, const char* chatroomId, _Out_ void** outJson)'),
  groupMemberCount: lib.func('int32 wcdb_get_group_member_count(int64 handle, const char* chatroomId, _Out_ int32* outCount)'),
  groupMemberCounts: lib.func('int32 wcdb_get_group_member_counts(int64 handle, const char* chatroomIdsJson, _Out_ void** outJson)'),
  groupStats: lib.func('int32 wcdb_get_group_stats(int64 handle, const char* chatroomId, int32 begin, int32 end, _Out_ void** outJson)'),
  snsTimeline: lib.func('int32 wcdb_get_sns_timeline(int64 handle, int32 limit, int32 offset, const char* username, const char* keyword, int32 startTime, int32 endTime, _Out_ void** outJson)'),
  snsUsernames: lib.func('int32 wcdb_get_sns_usernames(int64 handle, _Out_ void** outJson)'),
  snsExport: lib.func('int32 wcdb_get_sns_export_stats(int64 handle, const char* myAccountId, _Out_ void** outJson)'),
  snsAnnual: lib.func('int32 wcdb_get_sns_annual_stats(int64 handle, int32 begin, int32 end, _Out_ void** outJson)'),
  aggregate: lib.func('int32 wcdb_get_aggregate_stats(int64 handle, const char* sessionIdsJson, int32 begin, int32 end, _Out_ void** outJson)'),
  years: lib.func('int32 wcdb_get_available_years(int64 handle, const char* sessionIdsJson, _Out_ void** outJson)'),
  annual: lib.func('int32 wcdb_get_annual_report_stats(int64 handle, const char* sessionIdsJson, int32 begin, int32 end, _Out_ void** outJson)'),
  annualExtras: lib.func('int32 wcdb_get_annual_report_extras(int64 handle, const char* sessionIdsJson, int32 begin, int32 end, int32 peakBegin, int32 peakEnd, _Out_ void** outJson)'),
  dual: lib.func('int32 wcdb_get_dual_report_stats(int64 handle, const char* sessionId, int32 begin, int32 end, _Out_ void** outJson)'),
  footprint: lib.func('int32 wcdb_get_my_footprint_stats(int64 handle, const char* optionsJson, _Out_ void** outJson)'),
  dates: lib.func('int32 wcdb_get_message_dates(int64 handle, const char* sessionId, _Out_ void** outJson)'),
  messageTables: lib.func('int32 wcdb_get_message_tables(int64 handle, const char* sessionId, _Out_ void** outJson)'),
  tableStats: lib.func('int32 wcdb_get_message_table_stats(int64 handle, const char* sessionId, _Out_ void** outJson)'),
  tableColumns: lib.func('int32 wcdb_get_message_table_columns(int64 handle, const char* dbPath, const char* tableName, _Out_ void** outJson)'),
  tableRange: lib.func('int32 wcdb_get_message_table_time_range(int64 handle, const char* dbPath, const char* tableName, _Out_ void** outJson)'),
  listTables: lib.func('int32 wcdb_list_tables(int64 handle, const char* kind, const char* dbPath, _Out_ void** outJson)'),
  tableSchema: lib.func('int32 wcdb_get_table_schema(int64 handle, const char* kind, const char* dbPath, const char* tableName, _Out_ void** outJson)'),
  execQuery: lib.func('int32 wcdb_exec_query(int64 handle, const char* kind, const char* path, const char* sql, _Out_ void** outJson)'),
  listMessageDbs: lib.func('int32 wcdb_list_message_dbs(int64 handle, _Out_ void** outJson)'),
  listMediaDbs: lib.func('int32 wcdb_list_media_dbs(int64 handle, _Out_ void** outJson)'),
  mediaSchema: lib.func('int32 wcdb_get_media_schema_summary(int64 handle, const char* dbPath, _Out_ void** outJson)'),
  voice: lib.func('int32 wcdb_get_voice_data(int64 handle, const char* sessionId, int32 createTime, int32 localId, int64 svrId, const char* candidatesJson, _Out_ void** outHex)'),
  voiceBatch: lib.func('int32 wcdb_get_voice_data_batch(int64 handle, const char* requestsJson, _Out_ void** outJson)'),
  image: lib.func('int32 wcdb_resolve_image_hardlink(int64 handle, const char* md5, const char* accountDir, _Out_ void** outJson)'),
  imageBatch: lib.func('int32 wcdb_resolve_image_hardlink_batch(int64 handle, const char* requestsJson, _Out_ void** outJson)'),
  video: lib.func('int32 wcdb_resolve_video_hardlink_md5(int64 handle, const char* md5, const char* dbPath, _Out_ void** outJson)'),
  videoBatch: lib.func('int32 wcdb_resolve_video_hardlink_md5_batch(int64 handle, const char* requestsJson, _Out_ void** outJson)'),
  emoticonUrl: lib.func('int32 wcdb_get_emoticon_cdn_url(int64 handle, const char* dbPath, const char* md5, _Out_ void** outUrl)'),
  emoticonCaption: lib.func('int32 wcdb_get_emoticon_caption(int64 handle, const char* dbPath, const char* md5, _Out_ void** outCaption)'),
  emoticonCaptionStrict: lib.func('int32 wcdb_get_emoticon_caption_strict(int64 handle, const char* md5, _Out_ void** outCaption)'),
  headImages: lib.func('int32 wcdb_get_head_image_buffers(int64 handle, const char* usernamesJson, _Out_ void** outJson)'),
  mediaStream: lib.func('int32 wcdb_scan_media_stream(int64 handle, const char* sessionIdsJson, int32 mediaType, int32 beginTimestamp, int32 endTimestamp, int32 limit, int32 offset, _Out_ void** outJson, _Out_ int32* outHasMore)'),
  dbStatus: lib.func('int32 wcdb_get_db_status(int64 handle, _Out_ void** outJson)'),
  logs: lib.func('int32 wcdb_get_logs(_Out_ void** outJson)'),
  cloudInit: lib.func('int32 wcdb_cloud_init(int32 intervalSeconds)'),
  cloudReport: lib.func('int32 wcdb_cloud_report(const char* statsJson)'),
  cloudStop: lib.func('void wcdb_cloud_stop()'),
  startMonitor: lib.func('int32 wcdb_start_monitor_pipe()'),
  stopMonitor: lib.func('void wcdb_stop_monitor_pipe()'),
  monitorName: lib.func('int32 wcdb_get_monitor_pipe_name(_Out_ void** outName)'),
  verifyUser: lib.func('void VerifyUser(int64 hwnd, const char* message, _Out_ char* outResult, int maxLen)'),
  purge: lib.func('int32 wcdb_purge_memory()'),
  closeAccount: lib.func('int32 wcdb_close_account(int64 handle)'),
}

function takeString(ptr) {
  if (!ptr) return null
  try {
    return koffi.decode(ptr, 'char', -1)
  } finally {
    try {
      F.free(ptr)
    } catch {}
  }
}

function takeJson(ptr) {
  const text = takeString(ptr)
  if (text === null || text === '') return null
  return JSON.parse(text)
}

let failures = 0
let checks = 0
function check(name, condition, detail) {
  checks += 1
  if (condition) {
    console.log(`PASS ${name}${detail ? ` :: ${detail}` : ''}`)
  } else {
    failures += 1
    console.log(`FAIL ${name}${detail ? ` :: ${detail}` : ''}`)
  }
}

function jsonCall(name, fn, ...args) {
  const out = [null]
  const rc = fn(...args, out)
  if (rc !== 0) {
    check(name, false, `rc=${rc}`)
    return null
  }
  try {
    const value = takeJson(out[0])
    check(name, value !== null, Array.isArray(value) ? `rows=${value.length}` : 'object')
    return value
  } catch (e) {
    check(name, false, `parse: ${e.message}`)
    return null
  }
}

function intCall(name, fn, ...args) {
  const out = [0]
  const rc = fn(...args, out)
  check(name, rc === 0, `rc=${rc} value=${out[0]}`)
  return out[0]
}

function main() {
  check('init', F.init() === 0)
  const handleOut = [0]
  const openRc = F.open(sessionDb, keyHex, handleOut)
  check('open_account', openRc === 0 && handleOut[0] > 0, `rc=${openRc} handle=${handleOut[0]}`)
  if (openRc !== 0 || !handleOut[0]) return summarize()
  const h = handleOut[0]
  check('set_my_account_id', F.setAccount(h, accountId) === 0)

  const sessions = jsonCall('get_sessions', F.sessions, h)
  if (!Array.isArray(sessions) || sessions.length === 0) return summarize()
  const first = sessions[0]
  console.log('sample session:', JSON.stringify(first).slice(0, 300))
  const sessionId = first.username || first.user_name
  check('session username', typeof sessionId === 'string' && sessionId.length > 0, sessionId)

  let targetSession = sessionId
  let bestCount = -1
  for (const row of sessions.slice(0, 80)) {
    const id = row.username || row.user_name
    if (!id) continue
    const out = [0]
    if (F.messageCount(h, id, out) === 0 && out[0] > bestCount) {
      bestCount = out[0]
      targetSession = id
    }
  }
  check('target session with messages', bestCount > 0, `${targetSession} count=${bestCount}`)

  intCall('get_message_count', F.messageCount, h, targetSession)
  const msgs = jsonCall('get_messages', F.messages, h, targetSession, 5, 0)
  if (Array.isArray(msgs) && msgs.length) {
    console.log('sample message:', JSON.stringify(msgs[0]).slice(0, 400))
    const m = msgs[0]
    check('message fields', m.local_type !== undefined && m.create_time !== undefined && m.is_send !== undefined)
    if (m.local_id !== undefined) {
      const byId = jsonCall('get_message_by_id', F.messageById, h, targetSession, m.local_id)
      check('message_by_id matches', !!byId && Number(byId.local_id) === Number(m.local_id))
    }
    if (m.server_id) {
      jsonCall('get_message_by_svrid', F.messageBySvrid, h, targetSession, String(m.server_id))
    }
  }

  jsonCall('session_message_counts', F.sessionCounts, h, JSON.stringify([targetSession]))
  jsonCall('message_type_stats', F.typeStats, h, targetSession, 0, 0)
  jsonCall('message_type_stats_batch', F.typeStatsBatch, h, JSON.stringify([targetSession]), JSON.stringify({ begin: 0, end: 0, include_date_counts: true }))
  jsonCall('message_date_counts', F.dateCounts, h, targetSession)
  jsonCall('message_date_counts_batch', F.dateCountsBatch, h, JSON.stringify([targetSession]))
  jsonCall('message_dates', F.dates, h, targetSession)
  jsonCall('search_messages', F.search, h, targetSession, '的', 5, 0, 0, 0)

  // Cursor: full pass over the first 3 batches.
  const cursorOut = [0]
  const cursorRc = F.openCursor(h, targetSession, 50, 1, 0, 0, cursorOut)
  check('open_message_cursor', cursorRc === 0 && cursorOut[0] > 0, `rc=${cursorRc} cursor=${cursorOut[0]}`)
  if (cursorRc === 0 && cursorOut[0] > 0) {
    check('set_projection', F.setProjection(h, cursorOut[0], 1) === 0)
    let fetched = 0
    for (let i = 0; i < 3; i += 1) {
      const outPtr = [null]
      const hasMore = [0]
      const rc = F.fetchBatch(h, cursorOut[0], outPtr, hasMore)
      if (rc !== 0) {
        check(`fetch_batch_${i}`, false, `rc=${rc}`)
        break
      }
      const rows = takeJson(outPtr[0])
      fetched += Array.isArray(rows) ? rows.length : 0
      console.log(`fetch_batch_${i}: rows=${Array.isArray(rows) ? rows.length : 'n/a'} hasMore=${hasMore[0]}`)
      if (!hasMore[0]) break
    }
    check('cursor rows', fetched > 0, `total=${fetched}`)
    check('close_cursor', F.closeCursor(h, cursorOut[0]) === 0)
    check('close_cursor idempotent', F.closeCursor(h, cursorOut[0]) === 0)
  }

  jsonCall('contacts_compact', F.contactsCompact, h, JSON.stringify([sessionId]))
  jsonCall('contact', F.contact, h, sessionId)
  jsonCall('contact_status', F.contactStatus, h, JSON.stringify([sessionId]))
  jsonCall('contact_type_counts', F.typeCounts, h)
  jsonCall('contact_alias_map', F.aliasMap, h, JSON.stringify([sessionId]))
  jsonCall('contact_friend_flags', F.friendFlags, h, JSON.stringify([sessionId]))
  jsonCall('display_names', F.displayNames, h, JSON.stringify([sessionId]))
  jsonCall('avatar_urls', F.avatarUrls, h, JSON.stringify([sessionId]))

  const chatroom = sessions.find((row) => String(row.username || '').endsWith('@chatroom'))
  if (chatroom) {
    const room = chatroom.username
    intCall('group_member_count', F.groupMemberCount, h, room)
    jsonCall('group_member_counts', F.groupMemberCounts, h, JSON.stringify([room]))
    const members = jsonCall('group_members', F.groupMembers, h, room)
    if (Array.isArray(members) && members.length) {
      console.log('sample member:', JSON.stringify(members[0]))
      check('group member username', typeof members[0].username === 'string' && members[0].username.length > 0)
    }
    const nicknames = jsonCall('group_nicknames', F.groupNicknames, h, room)
    check('group nicknames shape', !!nicknames && !Array.isArray(nicknames), `keys=${nicknames ? Object.keys(nicknames).length : 0}`)
    jsonCall('chat_room_ext_buffer', F.extBuffer, h, room)
    jsonCall('group_stats', F.groupStats, h, room, 0, 0)
  } else {
    console.log('SKIP group tests (no chatroom session)')
  }

  jsonCall('sns_timeline', F.snsTimeline, h, 5, 0, '', '', 0, 0)
  jsonCall('sns_usernames', F.snsUsernames, h)
  jsonCall('sns_export_stats', F.snsExport, h, accountId)
  jsonCall('sns_annual_stats', F.snsAnnual, h, 0, Math.floor(Date.now() / 1000))

  const ids = [targetSession, ...sessions.slice(0, 3).map((row) => row.username).filter(Boolean)]
    .filter((id, index, list) => id && list.indexOf(id) === index)
  jsonCall('aggregate_stats', F.aggregate, h, JSON.stringify(ids), 0, 0)
  jsonCall('available_years', F.years, h, JSON.stringify(ids))
  jsonCall('annual_report_stats', F.annual, h, JSON.stringify(ids), 0, 0)
  jsonCall('annual_report_extras', F.annualExtras, h, JSON.stringify(ids), 0, 0, 0, 0)
  jsonCall('dual_report_stats', F.dual, h, targetSession, 0, 0)
  jsonCall('my_footprint_stats', F.footprint, h, JSON.stringify({ begin: 0, end: 0, my_account_id: accountId, private_session_ids: ids, group_session_ids: [], mention_limit: 50, private_limit: 50, mention_mode: 'both' }))

  jsonCall('message_tables', F.messageTables, h, targetSession)
  const tableStats = jsonCall('table_stats', F.tableStats, h, targetSession)
  jsonCall('list_message_dbs', F.listMessageDbs, h)
  jsonCall('list_media_dbs', F.listMediaDbs, h)
  jsonCall('list_tables contact', F.listTables, h, 'contact', '')
  if (Array.isArray(tableStats) && tableStats.length) {
    const t = tableStats[0]
    console.log('sample table stat:', JSON.stringify(t))
    jsonCall('message_table_columns', F.tableColumns, h, t.db_path, t.table_name)
    jsonCall('message_table_time_range', F.tableRange, h, t.db_path, t.table_name)
    jsonCall('table_schema', F.tableSchema, h, 'message', t.db_path, t.table_name)
  }
  jsonCall('exec_query contact', F.execQuery, h, 'contact', '', 'SELECT * FROM contact LIMIT 2')
  jsonCall('exec_query session', F.execQuery, h, 'session', '', 'SELECT * FROM SessionTable LIMIT 2')
  jsonCall('media_schema_summary', F.mediaSchema, h, '')

  // Media probes: pick real rows.
  const images = jsonCall('messages_by_type image', F.messagesByType, h, sessionId, 3, 0, 3, 0)
  if (Array.isArray(images) && images.length) {
    const md5 = images[0].image_md5 || extractMd5(images[0])
    if (md5) {
      jsonCall('resolve_image_hardlink', F.image, h, md5, accountDir)
      jsonCall('resolve_image_batch', F.imageBatch, h, JSON.stringify([{ md5, account_dir: accountDir }]))
    }
  }
  const voices = jsonCall('messages_by_type voice', F.messagesByType, h, sessionId, 34, 0, 3, 0)
  if (Array.isArray(voices) && voices.length) {
    const v = voices[0]
    const out = [null]
    const rc = F.voice(h, sessionId, Number(v.create_time) || 0, Number(v.local_id) || 0, Number(v.server_id) || 0, '[]', out)
    const hex = takeString(out[0])
    check('get_voice_data', rc === 0 && typeof hex === 'string', `rc=${rc} hexLen=${(hex || '').length}`)
    jsonCall('get_voice_data_batch', F.voiceBatch, h, JSON.stringify([{ session_id: sessionId, create_time: Number(v.create_time) || 0, local_id: Number(v.local_id) || 0, svr_id: Number(v.server_id) || 0, candidates: [] }]))
  }
  const emojis = jsonCall('messages_by_type emoji', F.messagesByType, h, sessionId, 47, 0, 3, 0)
  if (Array.isArray(emojis) && emojis.length) {
    const md5 = emojis[0].emoji_md5 || extractMd5(emojis[0])
    if (md5) {
      const out = [null]
      const rc = F.emoticonUrl(h, '', md5, out)
      const url = takeString(out[0])
      check('emoticon_cdn_url', rc === 0, `rc=${rc} url=${(url || '').slice(0, 40)}`)
      const out2 = [null]
      const rc2 = F.emoticonCaptionStrict(h, md5, out2)
      const caption = takeString(out2[0])
      check('emoticon_caption_strict', rc2 === 0, `rc=${rc2} caption=${(caption || '').slice(0, 30)}`)
    }
  }
  const videos = jsonCall('messages_by_type video', F.messagesByType, h, sessionId, 43, 0, 2, 0)
  if (Array.isArray(videos) && videos.length) {
    const md5 = videos[0].video_md5 || extractMd5(videos[0])
    if (md5) jsonCall('resolve_video_hardlink', F.video, h, md5, path.resolve(path.dirname(sessionDb), '..', 'message', 'media_0.db'))
  }

  const streamOut = [null]
  const streamHasMore = [0]
  const streamRc = F.mediaStream(h, JSON.stringify(ids.slice(0, 1)), 0, 0, 0, 10, 0, streamOut, streamHasMore)
  if (streamRc === 0) {
    const rows = takeJson(streamOut[0])
    check('scan_media_stream', true, `rows=${Array.isArray(rows) ? rows.length : 'n/a'} hasMore=${streamHasMore[0]}`)
  } else {
    check('scan_media_stream', false, `rc=${streamRc}`)
  }

  jsonCall('head_image_buffers', F.headImages, h, JSON.stringify([sessionId]))
  jsonCall('get_db_status', F.dbStatus, h)
  jsonCall('get_logs', F.logs)

  check('cloud_init', F.cloudInit(300) === 0)
  check('cloud_report', F.cloudReport('[]') === 0)
  F.cloudStop()
  check('start_monitor_pipe returns failure (unsupported)', F.startMonitor() !== 0)
  F.stopMonitor()
  const nameOut = [null]
  const nameRc = F.monitorName(nameOut)
  const pipeName = takeString(nameOut[0])
  check('monitor_pipe_name', nameRc === 0 && typeof pipeName === 'string' && pipeName.length > 0, pipeName)

  const verifyBuf = Buffer.alloc(1024)
  F.verifyUser(0, 'probe', verifyBuf, 1024)
  const verifyText = verifyBuf.toString('utf8').replace(/\0+$/, '')
  let verifyJson = null
  try {
    verifyJson = JSON.parse(verifyText)
  } catch {}
  check('VerifyUser buffer', !!verifyJson && verifyJson.success === false, verifyText)

  check('purge_memory', F.purge() === 0)
  check('shutdown', F.shutdown() === 0)
  summarize()
}

function extractMd5(row) {
  const content = String(row.compress_content || row.message_content || '')
  const match = content.match(/md5="([0-9a-fA-F]{32})"/) || content.match(/md5=\\"([0-9a-fA-F]{32})/)
  return match ? match[1] : ''
}

function summarize() {
  console.log(`\nSUMMARY checks=${checks} failures=${failures}`)
  process.exitCode = failures === 0 ? 0 : 1
}

try {
  main()
} catch (e) {
  console.error('FATAL', e)
  process.exitCode = 2
}
