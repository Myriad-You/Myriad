import { getNowPlaying } from '../../../contexts/currentSong'
import { perceptionRegistry } from './registry'
import { getForegroundSurface } from './surface'

export function replaceMusicTrackSource(input: {
  now: number
  pageConsent: boolean
  ttlMs: number
}): void {
  const { song, playing, lyric } = getNowPlaying()
  if (input.pageConsent && song) {
    const facts: Record<string, string | number | boolean> = {
      title: song.name,
      artist: song.artist,
      source: song.source,
      playing,
    }
    if (song.album?.trim()) facts.album = song.album.trim()
    if (lyric) facts.lyric = lyric
    const head = playing
      ? `${song.name} — ${song.artist}`
      : `Paused ${song.name} — ${song.artist}`
    perceptionRegistry.replace({
      sourceId: 'music_track',
      kind: 'music',
      expiresAt: input.now + input.ttlMs,
      summary: lyric ? `${head} · ${lyric}` : head,
      safeFacts: facts,
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
        ? 'Looking at the control center'
        : surface === 'notification'
          ? 'Looking at notifications'
          : surface === 'user_modal'
            ? 'Looking at the user panel'
            : 'No overlay open',
    safeFacts: { surface },
    privacy: 'local',
  })
}
