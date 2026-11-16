import type { MediaAsset } from './mediaApi'
import { API_URL } from '../config'
import { apiService } from './api'
import { mediaAssetSrc } from './mediaApi'

/** 服务端要先去外站下载（最多 30 秒）再落库，比普通请求慢。 */
const REMOTE_MEDIA_TIMEOUT_MS = 90_000

/**
 * 把外站图片存进本站媒体库，返回本站地址。同样内容服务端只存一份。
 * 存下来是私有的，笔记发布时随引用公开，和上传的一样。
 */
export async function saveRemoteImage(url: string, signal?: AbortSignal): Promise<string> {
  const data = await apiService.post<{ item?: MediaAsset }>(
    '/media/remote',
    { url },
    { signal, timeout: REMOTE_MEDIA_TIMEOUT_MS },
  )
  const src = data?.item ? mediaAssetSrc(data.item) : ''
  if (typeof src !== 'string' || !src.trim()) throw new Error('Invalid media response')
  return src
}

/** 算「本站」的 origin：当前页面和 API 所在的。 */
export function siteImageOrigins(): string[] {
  if (typeof window === 'undefined') return []
  const origins = new Set([window.location.origin])
  try {
    origins.add(new URL(API_URL || '/', window.location.href).origin)
  } catch { /* API_URL 不是地址就只认当前页面。 */ }
  return [...origins]
}
