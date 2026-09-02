import type { Song } from '../utils/musicPlayer'
import { pickMusicContextState } from '../utils/musicPlayerState'

const listeners = new Set<() => void>()
let current: Song | null = null
let bound = false

export function getCurrentSong(): Song | null {
  return current
}

export function setCurrentSongSnapshot(song: Song | null): void {
  if (sameTrack(current, song)) return
  current = song
  for (const listener of listeners) listener()
}

export function subscribeCurrentSong(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/**
 * Ingest a published `music-player-state-change` detail (or `__musicPlayerState`).
 * Partial events without `currentSong` leave the snapshot alone.
 */
export function applyPublishedMusicState(detail: Record<string, unknown>): void {
  const patch = pickMusicContextState(detail)
  if (!('currentSong' in patch)) return
  setCurrentSongSnapshot((patch.currentSong as Song | null) ?? null)
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

function onPublishedMusicState(event: Event): void {
  const detail = (event as CustomEvent<Record<string, unknown>>).detail
  if (detail) applyPublishedMusicState(detail)
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
