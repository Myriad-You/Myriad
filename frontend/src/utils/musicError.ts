import { isUselessErrorText } from './userFacingError'

export interface MusicErrorFlash {
  key: string
  detail: string
}

function usefulMusicDetail(message: string): string {
  const detail = message.replaceAll(/\s+/g, ' ').trim()
  if (!detail || isUselessErrorText(detail)) return ''
  if (/网易云API|QQ音乐|copyright|地理位置/.test(detail)) return ''
  if (/^\{[\s\S]*\}$/.test(detail)) return ''
  return detail.length > 160 ? `${detail.slice(0, 159)}…` : detail
}

export function classifyMusicLoadError(reason: unknown): MusicErrorFlash {
  const message = reason instanceof Error ? reason.message.trim() : ''
  if (/^RATE_LIMITED$|rate limited|访问频率过高/i.test(message)) {
    return { key: 'playlistRateLimited', detail: '' }
  }
  if (/^PLAYLIST_BLOCKED$|版权|地理位置|copyright/i.test(message)) {
    return { key: 'playlistBlocked', detail: '' }
  }
  if (/^PLAYLIST_EMPTY$|歌单为空|empty|no available songs/i.test(message)) {
    return { key: 'playlistEmpty', detail: '' }
  }
  if (/^FETCH_FAILED$|^INVALID_PLAYLIST$/i.test(message)) {
    return { key: 'loadPlaylistFailed', detail: '' }
  }
  return {
    key: 'loadPlaylistFailed',
    detail: usefulMusicDetail(message),
  }
}

export function formatMusicError(
  translated: string,
  detail: string | undefined,
): string {
  const extra = (detail || '').trim()
  if (!extra || extra === translated) return translated
  return `${translated} · ${extra}`
}
