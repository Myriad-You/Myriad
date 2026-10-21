import type { Song } from './musicPlayer'
import { emitAppEvent } from './appEvents'
import { getNeteaseAudioUrlImmediate, isNeteaseVipFromMeta } from './musicPlayer'
import { proxyImageUrlOr } from './proxyImageUrl'
import { showInfo } from './toastManager'

interface LibrarySongItem {
  id: string
  title: string
  cover: string | null
  metadata: any
}

/** 用站内播放器放一首网易云的歌；正在放的那首再点就打开播放器（暂停中则继续）。 */
export function playNeteaseLibraryItem(
  item: LibrarySongItem,
  copy: { unknownArtist: string; unknownAlbum: string; vipSongWarning: string },
): void {
  const songId = (
    item.metadata.id || item.id.replaceAll('netease_song_', '')
  ).toString()
  const musicState = (window as any).__musicPlayerState
  if (
    musicState?.currentSong?.id != null &&
    String(musicState.currentSong.id) === songId
  ) {
    emitAppEvent('open-control-panel')
    // 暂停中再点同一首应恢复，不要当成已在播放。
    if (!musicState.isPlaying) {
      emitAppEvent('toggle-play-pause')
    }
    return
  }

  const isVip = isNeteaseVipFromMeta(item.metadata)
  if (isVip) {
    showInfo(copy.vipSongWarning)
  }

  const name = item.metadata.name || item.title
  let artist = copy.unknownArtist
  const artists = item.metadata.ar || item.metadata.artists || []
  if (Array.isArray(artists) && artists.length > 0) {
    artist = artists.map((a: any) => a.name || a).join(', ')
  } else if (item.metadata.artist) {
    artist = item.metadata.artist
  }

  let album = copy.unknownAlbum
  let cover = item.cover || ''
  if (item.metadata.al) {
    album = item.metadata.al.name || album
    cover = item.metadata.al.picUrl || cover
  } else if (item.metadata.album) {
    album = item.metadata.album.name || item.metadata.album
    if (item.metadata.album.picUrl) {
      cover = item.metadata.album.picUrl
    }
  }

  const duration = item.metadata.dt
    ? Math.floor(item.metadata.dt / 1000)
    : item.metadata.duration
      ? item.metadata.duration
      : 0

  // 同步 URL 禁止 await geo，会把开播拖成数百 ms～数秒。
  const url = getNeteaseAudioUrlImmediate(songId)

  const song: Song = {
    id: songId.toString(),
    name,
    artist,
    album,
    // 裸 CDN 封面必须代理，否则取色 canvas CORS 失败。
    cover: proxyImageUrlOr(cover, cover || ''),
    url,
    duration,
    source: 'netease',
    isVip,
  }

  emitAppEvent('open-control-panel')
  emitAppEvent('play-song', { song })
  void import('./analyticsEvents').then(
    ({ trackProductEvent, AnalyticsEvents }) => {
      trackProductEvent(AnalyticsEvents.MUSIC_LIBRARY_PLAY, {
        target: songId,
        throttleMs: 3000,
      })
    },
  )
}
