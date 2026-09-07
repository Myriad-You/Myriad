import type { Song } from '../utils/musicPlayer'
import { pickMusicContextState } from '../utils/musicPlayerState'

const listeners = new Set<() => void>()
let current: Song | null = null
let playing = false
let lyric = ''
let lastLyrics: unknown[] = []
let lastLyricIndex = -1
let bound = false

export function getCurrentSong(): Song | null {
  return current
}

export function getNowPlaying(): {
  song: Song | null
  playing: boolean
  lyric: string
} {
  return { song: current, playing, lyric }
}

export function setCurrentSongSnapshot(song: Song | null): void {
  if (song && sameTrack(current, song)) return
  current = song
  if (!song) {
    playing = false
    lyric = ''
    lastLyrics = []
    lastLyricIndex = -1
  }
  notify()
}

export function subscribeCurrentSong(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/**
 * Ingest a published `music-player-state-change` detail (or `__musicPlayerState`).
 * Partial events without `currentSong` leave the track alone, but still
 * update playing / current lyric.
 */
export function applyPublishedMusicState(
  detail: Record<string, unknown>,
): void {
  const patch = pickMusicContextState(detail)
  let changed = false

  if ('currentSong' in patch) {
    const song = (patch.currentSong as Song | null) ?? null
    if (!sameTrack(current, song)) {
      current = song
      if (!('lyrics' in patch) && !('currentLyricIndex' in patch)) {
        lastLyrics = []
        lastLyricIndex = -1
        lyric = ''
      }
      changed = true
    }
  }

  if ('isPlaying' in patch) {
    const next = Boolean(patch.isPlaying)
    if (playing !== next) {
      playing = next
      changed = true
    }
  }

  if ('lyrics' in patch) {
    lastLyrics = Array.isArray(patch.lyrics) ? patch.lyrics : []
  }
  if ('currentLyricIndex' in patch) {
    lastLyricIndex =
      typeof patch.currentLyricIndex === 'number' ? patch.currentLyricIndex : -1
  }
  if ('lyrics' in patch || 'currentLyricIndex' in patch) {
    const nextLyric = lyricLine(lastLyrics, lastLyricIndex)
    if (lyric !== nextLyric) {
      lyric = nextLyric
      changed = true
    }
  }

  if (changed) notify()
}

/** Bind once to the player publish event. No-op without `window`. */
export function bindPublishedMusicState(): void {
  if (bound || typeof window === 'undefined') return
  bound = true
  const initial = (
    window as unknown as { __musicPlayerState?: Record<string, unknown> }
  ).__musicPlayerState
  if (initial) applyPublishedMusicState(initial)
  window.addEventListener('music-player-state-change', onPublishedMusicState)
}

/** Strip play-url / cover before the object is sent to the Agent. */
export function agentMusicStatus(
  published: Record<string, unknown> | null | undefined,
): Record<string, unknown> | null {
  if (!published) return null
  const raw = published.currentSong
  const song =
    raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : null
  const currentSong = song
    ? {
        name: String(song.name ?? song.title ?? ''),
        artist: String(song.artist ?? ''),
        album: String(song.album ?? ''),
        source: String(song.source ?? ''),
        duration: Number(song.duration) || 0,
      }
    : null
  const currentLyric = lyricLine(published.lyrics, published.currentLyricIndex)
  return {
    isPlaying: !!published.isPlaying,
    isEnabled: !!published.isEnabled,
    currentSong,
    currentSongIndex: Number(published.currentSongIndex) || 0,
    playlistLength: Number(published.playlistLength) || 0,
    ...(currentLyric ? { currentLyric } : {}),
  }
}

/** Current player projection for Agent requests and live-presence renewals. */
export function currentAgentMusicStatus(): Record<string, unknown> | null {
  if (typeof window === 'undefined') return null
  return agentMusicStatus(
    (
      window as unknown as {
        __musicPlayerState?: Record<string, unknown>
      }
    ).__musicPlayerState,
  )
}

function onPublishedMusicState(event: Event): void {
  const detail = (event as CustomEvent<Record<string, unknown>>).detail
  if (detail) applyPublishedMusicState(detail)
}

function lyricLine(lyrics: unknown, index: unknown): string {
  if (!Array.isArray(lyrics) || typeof index !== 'number' || index < 0) {
    return ''
  }
  const row = lyrics[index]
  if (!row || typeof row !== 'object') return ''
  const text = (row as { text?: unknown }).text
  return typeof text === 'string' ? text.trim().slice(0, 120) : ''
}

function notify(): void {
  for (const listener of listeners) listener()
}

function sameTrack(left: Song | null, right: Song | null): boolean {
  if (left === right) return true
  if (!left || !right) return false
  return (
    left.id === right.id &&
    left.source === right.source &&
    left.name === right.name &&
    left.artist === right.artist
  )
}

if (typeof window !== 'undefined') {
  bindPublishedMusicState()
}
