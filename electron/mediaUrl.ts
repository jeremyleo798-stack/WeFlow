import { isAbsolute } from 'path'
import { fileURLToPath } from 'url'

export const MEDIA_SCHEME = 'weflow-media'

/**
 * 本地媒体统一走特权协议，渲染进程不再直接使用 file://。
 * 协议处理端只放行受控目录内的文件（见 electron/mediaProtocol.ts）。
 */
export const toMediaUrl = (filePath: string, version?: number | string): string => {
  const params = new URLSearchParams({ path: filePath })
  if (version !== undefined && version !== null && String(version) !== '') {
    params.set('v', String(version))
  }
  return `${MEDIA_SCHEME}://local/?${params.toString()}`
}

/** 把 file:// 或 weflow-media:// URL 还原成绝对路径；其他输入返回 null。 */
export const mediaUrlToPath = (value: string): string | null => {
  const raw = String(value || '').trim()
  if (!raw) return null

  if (/^file:/i.test(raw)) {
    try {
      const url = new URL(raw)
      url.search = ''
      url.hash = ''
      return fileURLToPath(url)
    } catch {
      return null
    }
  }

  if (!/^weflow-media:/i.test(raw)) return null
  try {
    const url = new URL(raw)
    const target = url.searchParams.get('path')
    if (!target || !isAbsolute(target)) return null
    return target
  } catch {
    return null
  }
}
