export const MEDIA_SCHEME = 'weflow-media'

const PASSTHROUGH = /^(data:|blob:|https?:|weflow-media:)/i

const buildMediaUrl = (filePath: string, version?: string): string => {
  const params = new URLSearchParams({ path: filePath })
  if (version) params.set('v', version)
  return `${MEDIA_SCHEME}://local/?${params.toString()}`
}

const parseFileUrl = (value: string): { filePath: string; version: string | null } | null => {
  try {
    const url = new URL(value)
    let filePath = decodeURIComponent(url.pathname)
    if (url.hostname) {
      filePath = `//${url.hostname}${filePath}`
    } else if (/^\/[a-zA-Z]:/.test(filePath)) {
      filePath = filePath.slice(1)
    }
    return { filePath, version: url.searchParams.get('v') }
  } catch {
    return null
  }
}

/**
 * 把本地路径 / file:// URL 统一换成 weflow-media://，其他（https/data/blob）原样返回。
 * 主进程协议只放行受控目录，渲染进程不再直接使用 file://。
 */
export const toMediaUrl = (source?: string | null, version?: string | number): string => {
  const raw = String(source ?? '').trim()
  if (!raw) return ''
  if (PASSTHROUGH.test(raw)) return raw

  const explicitVersion = version === undefined || version === null ? undefined : String(version)

  if (/^file:/i.test(raw)) {
    const parsed = parseFileUrl(raw)
    if (!parsed) return raw
    return buildMediaUrl(parsed.filePath, explicitVersion ?? parsed.version ?? undefined)
  }

  if (raw.startsWith('\\\\') || raw.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(raw)) {
    return buildMediaUrl(raw, explicitVersion)
  }

  return raw
}
