import { protocol } from 'electron'
import { createReadStream, realpathSync, statSync } from 'fs'
import { extname, isAbsolute, relative, resolve } from 'path'
import { Readable } from 'stream'
import { MEDIA_SCHEME } from './mediaUrl'

export type MediaRootsProvider = () => string[]

const MIME_BY_EXTENSION: Record<string, string> = {
  '.mp4': 'video/mp4',
  '.m4v': 'video/mp4',
  '.mov': 'video/quicktime',
  '.webm': 'video/webm',
  '.mkv': 'video/x-matroska',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.png': 'image/png',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.bmp': 'image/bmp',
  '.avif': 'image/avif',
  '.mp3': 'audio/mpeg',
  '.wav': 'audio/wav',
  '.ogg': 'audio/ogg',
  '.m4a': 'audio/mp4',
  '.aac': 'audio/aac',
  '.amr': 'audio/amr',
  '.silk': 'application/octet-stream',
  '.flac': 'audio/flac'
}

const COMMON_HEADERS = {
  'Accept-Ranges': 'bytes',
  'Access-Control-Allow-Origin': '*',
  'Cross-Origin-Resource-Policy': 'cross-origin',
  'Cache-Control': 'private, max-age=86400'
}

const respond = (status: number, message: string, extraHeaders?: Record<string, string>): Response =>
  new Response(message, {
    status,
    headers: {
      'Content-Type': 'text/plain; charset=utf-8',
      'Access-Control-Allow-Origin': '*',
      ...extraHeaders
    }
  })

const isInside = (root: string, candidate: string): boolean => {
  const rel = relative(root, candidate)
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel))
}

const realpathOrNull = (target: string): string | null => {
  try {
    // 必须用 native 版：非 native 的 realpathSync 不会展开 8.3 短名，TEMP 常以 ADMINI~1 形式传入
    return realpathSync.native(target)
  } catch {
    return null
  }
}

const resolveRange = (
  rangeHeader: string,
  size: number
): { start: number; end: number } | 'invalid' => {
  const match = /^bytes=(\d*)-(\d*)$/i.exec(rangeHeader.trim())
  if (!match) return 'invalid'
  const [, startRaw, endRaw] = match
  if (!startRaw && !endRaw) return 'invalid'

  if (!startRaw) {
    const suffixLength = Number(endRaw)
    if (!Number.isFinite(suffixLength) || suffixLength <= 0 || size === 0) return 'invalid'
    return { start: Math.max(0, size - suffixLength), end: size - 1 }
  }

  const start = Number(startRaw)
  const end = endRaw ? Math.min(Number(endRaw), size - 1) : size - 1
  if (!Number.isFinite(start) || !Number.isFinite(end) || start >= size || end < start) return 'invalid'
  return { start, end }
}

/** 必须在 app ready 之前调用。 */
export const registerMediaScheme = (): void => {
  protocol.registerSchemesAsPrivileged([
    {
      scheme: MEDIA_SCHEME,
      privileges: {
        standard: true,
        secure: true,
        supportFetchAPI: true,
        stream: true,
        corsEnabled: true
      }
    }
  ])
}

/** 必须在 app ready 之后调用；roots 每次请求实时计算，配置切换后立即生效。 */
export const registerMediaProtocol = (getRoots: MediaRootsProvider): void => {
  protocol.handle(MEDIA_SCHEME, async (request) => {
    if (request.method === 'OPTIONS') {
      return new Response(null, {
        status: 204,
        headers: {
          ...COMMON_HEADERS,
          'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
          'Access-Control-Allow-Headers': request.headers.get('access-control-request-headers') || 'Range'
        }
      })
    }
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return respond(405, 'Method Not Allowed', { Allow: 'GET, HEAD, OPTIONS' })
    }

    let target: string
    try {
      const url = new URL(request.url)
      const rawPath = url.searchParams.get('path')
      if (!rawPath || !isAbsolute(rawPath)) return respond(400, 'Bad Request')
      target = resolve(rawPath)
    } catch {
      return respond(400, 'Bad Request')
    }

    const realTarget = realpathOrNull(target)
    if (!realTarget) return respond(404, 'Not Found')

    const allowed = getRoots().some((root) => {
      if (!root) return false
      const realRoot = realpathOrNull(resolve(root))
      return realRoot ? isInside(realRoot, realTarget) : false
    })
    if (!allowed) return respond(403, 'Forbidden')

    let stat
    try {
      stat = statSync(realTarget)
    } catch {
      return respond(404, 'Not Found')
    }
    if (!stat.isFile()) return respond(404, 'Not Found')

    const contentType = MIME_BY_EXTENSION[extname(realTarget).toLowerCase()] || 'application/octet-stream'
    const headers: Record<string, string> = { ...COMMON_HEADERS, 'Content-Type': contentType }

    const rangeHeader = request.headers.get('range')
    if (rangeHeader) {
      const range = resolveRange(rangeHeader, stat.size)
      if (range === 'invalid') {
        return respond(416, 'Range Not Satisfiable', { 'Content-Range': `bytes */${stat.size}` })
      }
      headers['Content-Range'] = `bytes ${range.start}-${range.end}/${stat.size}`
      headers['Content-Length'] = String(range.end - range.start + 1)
      if (request.method === 'HEAD') return new Response(null, { status: 206, headers })
      const stream = createReadStream(realTarget, { start: range.start, end: range.end })
      return new Response(Readable.toWeb(stream) as unknown as ReadableStream, { status: 206, headers })
    }

    headers['Content-Length'] = String(stat.size)
    if (request.method === 'HEAD') return new Response(null, { status: 200, headers })
    const stream = createReadStream(realTarget)
    return new Response(Readable.toWeb(stream) as unknown as ReadableStream, { status: 200, headers })
  })
}
