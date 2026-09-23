import type { ReactNode } from 'react'
import type {
  LyricLine,
  Song,
  VerbatimLyricsSource,
  WordLyricLine,
} from '../utils/musicPlayer'
import {
  useCallback,
  useEffect,
  useMemo,
  useSyncExternalStore,
} from 'react'
import { bindMusicMoodListening } from '../features/merope/musicMood'
import { emitAppEvent } from '../utils/appEvents'
import {
  mergeMusicContextState,
  pickMusicContextState,
} from '../utils/musicPlayerState'
import {
  bindPublishedMusicState,
} from './currentSong'

export {
  applyPublishedMusicState,
  getCurrentSong,
  subscribeCurrentSong,
} from './currentSong'

interface MusicPlayerState {
  currentSong: Song | null
  isEnabled: boolean
  isPlaying: boolean
  musicColor: string
  isTempPlay: boolean
  currentSongIndex: number
  playlistLength: number
  playlist: Song[]
  lyrics: LyricLine[]
  verbatimLyrics: WordLyricLine[]
  hasVerbatimLyrics: boolean
  verbatimLyricsSource: VerbatimLyricsSource
  currentLyricIndex: number
}

export function MusicPlayerProvider({ children }: { children: ReactNode }) {
  useEffect(() => bindMusicMoodListening(), [])
  return children
}

let globalMusicState: MusicPlayerState = {
  currentSong: null,
  isEnabled: false,
  isPlaying: false,
  musicColor: '#ef4444',
  isTempPlay: false,
  currentSongIndex: 0,
  playlistLength: 0,
  playlist: [],
  lyrics: [],
  verbatimLyrics: [],
  hasVerbatimLyrics: false,
  verbatimLyricsSource: '',
  currentLyricIndex: -1,
}

const musicStateListeners = new Set<() => void>()
let isMusicEventListenerAttached = false

function emitMusicStateChange() {
  musicStateListeners.forEach((listener) => listener())
}

function subscribeMusicState(listener: () => void) {
  musicStateListeners.add(listener)
  if (typeof window !== 'undefined') {
    const currentState = (window as any).__musicPlayerState
    if (currentState) {
      updateGlobalMusicState(pickMusicContextState(currentState) as Partial<MusicPlayerState>)
    }
  }
  attachMusicEventListener()
  return () => {
    musicStateListeners.delete(listener)
    if (musicStateListeners.size === 0) {
      detachMusicEventListener()
    }
  }
}

function getMusicStateSnapshot() {
  return globalMusicState
}

/** useMusicPlayer owns __musicPlayerState; do not write currentTime:0 / sparse patches back. */
function updateGlobalMusicState(newState: Partial<MusicPlayerState>) {
  const next = mergeMusicContextState(globalMusicState, newState)
  if (next === globalMusicState) return
  globalMusicState = next
  emitMusicStateChange()
}

function handleGlobalMusicStateChange(event: Event) {
  const detail = (event as CustomEvent).detail as
    Record<string, unknown> | undefined
  if (!detail) return
  // Ignore host-only fields such as currentTime / musicColors.
  const patch = pickMusicContextState(detail) as Partial<MusicPlayerState>
  updateGlobalMusicState(patch)
}

function attachMusicEventListener() {
  if (isMusicEventListenerAttached || typeof window === 'undefined') return
  window.addEventListener(
    'music-player-state-change',
    handleGlobalMusicStateChange,
  )
  isMusicEventListenerAttached = true
}

function detachMusicEventListener() {
  if (!isMusicEventListenerAttached || typeof window === 'undefined') return
  window.removeEventListener(
    'music-player-state-change',
    handleGlobalMusicStateChange,
  )
  isMusicEventListenerAttached = false
}

if (typeof window !== 'undefined') {
  // Tapp SDK reads spectrum data from window.audioManager.
  import('../utils/musicPlayer').then(({ audioManager }) => {
    ;(window as any).audioManager = audioManager
  })

  const initialState = (window as any).__musicPlayerState
  if (initialState) {
    updateGlobalMusicState(pickMusicContextState(initialState) as Partial<MusicPlayerState>)
  }
  bindPublishedMusicState()
  attachMusicEventListener()
}

export function useMusicPlayerControl() {
  const state = useSyncExternalStore(
    subscribeMusicState,
    getMusicStateSnapshot,
    getMusicStateSnapshot,
  )

  const playSong = useCallback((song: Song) => {
    emitAppEvent('play-song', { song })
  }, [])

  const togglePlayPause = useCallback(() => {
    emitAppEvent('toggle-play-pause')
  }, [])

  const stopTempPlay = useCallback(() => {
    emitAppEvent('stop-temp-play')
  }, [])

  return useMemo(
    () => ({
      ...state,
      playSong,
      togglePlayPause,
      stopTempPlay,
    }),
    [state, playSong, togglePlayPause, stopTempPlay],
  )
}

interface MusicLyricsSlice {
  lyrics: LyricLine[]
  currentLyricIndex: number
}

let lyricsSliceCache: MusicLyricsSlice = {
  lyrics: globalMusicState.lyrics,
  currentLyricIndex: globalMusicState.currentLyricIndex,
}

function getMusicLyricsSliceSnapshot(): MusicLyricsSlice {
  const s = globalMusicState
  if (
    lyricsSliceCache.lyrics === s.lyrics &&
    lyricsSliceCache.currentLyricIndex === s.currentLyricIndex
  ) {
    return lyricsSliceCache
  }
  lyricsSliceCache = {
    lyrics: s.lyrics,
    currentLyricIndex: s.currentLyricIndex,
  }
  return lyricsSliceCache
}

/** Lyrics + current index only; isPlaying / playlist must not retrigger. */
export function useMusicLyricsSlice(): MusicLyricsSlice {
  return useSyncExternalStore(
    subscribeMusicState,
    getMusicLyricsSliceSnapshot,
    getMusicLyricsSliceSnapshot,
  )
}
