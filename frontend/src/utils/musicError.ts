export interface MusicErrorFlash {
  key: string
  detail: string
}

function usefulMusicDetail(message: string): string {
  const detail = message.replace(/\s+/g, ' ').trim()
  if (!detail) return ''
  if (/^API Error:\s*\d+$/i.test(detail)) return ''
  if (/^\{[\s\S]*\}$/.test(detail)) return ''
  if (/failed to fetch|networkerror|load failed/i.test(detail)) return ''
  return detail.length > 160 ? `${detail.slice(0, 159)}…` : detail
}

export function classifyMusicLoadError(reason: unknown): MusicErrorFlash {
  const message = reason instanceof Error ? reason.message.trim() : ''
  if (/rate limited|访问频率过高/i.test(message)) {
    return { key: 'playlistRateLimited', detail: '' }
  }
  if (/版权|地理位置|copyright/i.test(message)) {
    return { key: 'playlistBlocked', detail: '' }
  }
  if (/歌单为空|empty|no available songs/i.test(message)) {
    return { key: 'playlistEmpty', detail: '' }
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
