import type { User } from '../contexts/AuthContext'
import type { MusicPlayerWindowState } from '../hooks/musicPlayer/globalState'
import type { Song } from './musicPlayer'

/**
 * Every window-level event the host dispatches, with its payload. Names stay
 * wire-compatible — TAPP sandboxes and existing listeners still see plain
 * CustomEvents — but producers are type-checked and the catalog lives in one
 * place. `void` means the event carries no detail.
 *
 * Agent panel events keep their own typed helpers in agentPanelEvents.ts.
 */
export interface AppEventMap {
  // Agent actions
  'arael-open-manage': { tab?: 'heartbeat' | 'skills' | 'memory' }
  'arael-open-session': { sessionId: string, runId?: string, taskId?: string }
  'arael-persona-updated': void

  // Session
  'auth-login-success': { user: User, isAdmin: boolean }
  'auth-state-changed': { isAuthenticated: boolean, isAdmin: boolean }
  'tapp-subject-ready': { isAuthenticated: boolean }

  // Site appearance (config saves and the wallpaper loader)
  'wallpaperChanged': { url: string, timestamp: number, fromCache: boolean }
  'wallpaperConfigChanged': void
  'footerConfigChanged': void
  'islandContentChanged': void

  // Shell chrome
  'config-loaded': void
  'open-control-panel': { tab?: 'notifications' } | undefined
  'control-panel-content-resize': void
  'gcp-animation-end': void
  'gcp-remeasure': { immediate?: boolean } | undefined
  'nav-expand-secondary': { path: string }
  'custom-platforms-update': { platforms: unknown[], persisted?: boolean }

  // Music player commands (handled by the player's host events)
  'play-song': { song: Song }
  'play-song-at-index': { index: number, song: Song }
  'jump-to-index': { index: number, song: Song }
  'toggle-play-pause': void
  'stop-temp-play': void
  'music-player-next': void
  'music-player-prev': void
  'music-player-seek': { position: number }
  'music-player-volume': { volume: number }
  'music-player-mute': { muted: boolean }
  'music-player-mode': { mode: string }
  'music-player-set-skip-vip': { value: boolean }
  'music-player-load-playlist': {
    playlistId: string
    source?: string
    autoPlay?: boolean
    timestamp?: number
  }
  /** Enrichment for the playing song; merged by id, never a replacement. */
  'music-player-patch-current-song': { song: Partial<Song> }
  'request-music-state-sync': void

  // Music player notifications
  'music-player-state-change': MusicPlayerWindowState
  'music-player-progress': {
    currentTime: number
    audioDuration: number
    songId: Song['id'] | null
  }
  'music-cover-loaded': { songId: Song['id'], cover: Song['cover'] }
}

export type AppEventName = keyof AppEventMap

/** Typed view of a received event, for listeners registered with addEventListener. */
export type AppEvent<K extends AppEventName> = CustomEvent<AppEventMap[K]>

type DetailArgs<K extends AppEventName> = [AppEventMap[K]] extends [void]
  ? []
  : undefined extends AppEventMap[K]
    ? [detail?: AppEventMap[K]]
    : [detail: AppEventMap[K]]

export function emitAppEvent<K extends AppEventName>(type: K, ...[detail]: DetailArgs<K>): void {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent(type, { detail }))
}
