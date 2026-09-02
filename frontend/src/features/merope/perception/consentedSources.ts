import { getCurrentSong } from '../../../contexts/currentSong'
import { perceptionRegistry } from './registry'
import { getForegroundSurface } from './surface'

export function replaceMusicTrackSource(input: {
  now: number
  pageConsent: boolean
  ttlMs: number
}): void {
  const song = getCurrentSong()
  if (input.pageConsent && song) {
    perceptionRegistry.replace({
      sourceId: 'music_track',
      kind: 'music',
      expiresAt: input.now + input.ttlMs,
      summary: `${song.name} — ${song.artist}`,
      safeFacts: { title: song.name, artist: song.artist, source: song.source },
      privacy: 'consented',
    })
  } else {
    perceptionRegistry.forget('music_track')
  }
}

export function replaceSurfaceSource(input: { now: number; ttlMs: number }): void {
  const surface = getForegroundSurface()
  perceptionRegistry.replace({
    sourceId: 'surface',
    kind: 'surface',
    expiresAt: input.now + input.ttlMs,
    summary:
      surface === 'control_panel'
        ? '正在看控制中心'
        : surface === 'notification'
          ? '正在看通知'
          : surface === 'user_modal'
            ? '正在看用户面板'
            : '没有打开浮层',
    safeFacts: { surface },
    privacy: 'local',
  })
}
