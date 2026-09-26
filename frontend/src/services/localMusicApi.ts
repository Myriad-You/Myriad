import { apiService } from './api'

export interface LocalTrack {
  id: number
  title: string
  artist: string
  album: string
  durationMs: number
  audioMediaId: number
  coverMediaId: number | null
  hasLyrics: boolean
  sortOrder: number
  enabled: boolean
  audioUrl: string
  coverUrl: string | null
  ext: string
  filename: string
  sizeBytes: number
}

export interface LocalPlaylist {
  id: number
  name: string
  sortOrder: number
  trackIds: number[]
}

export interface LocalTrackInput {
  title: string
  artist?: string
  album?: string
  durationMs?: number
  audioMediaId: number
  coverMediaId?: number | null
  lyrics?: string
  sortOrder?: number
  enabled?: boolean
}

export interface LocalPlaylistInput {
  name: string
  sortOrder?: number
  trackIds: number[]
}

/** Audio uploads can be large; keep a generous timeout like mediaApi. */
const LOCAL_MUSIC_UPLOAD_TIMEOUT_MS = 10 * 60_000

export async function listLocalTracks(): Promise<LocalTrack[]> {
  const body = await apiService.get<{ tracks: LocalTrack[] }>('/local-music')
  return body.tracks
}

export async function deleteLocalTrack(id: number): Promise<void> {
  await apiService.delete(`/local-music/${id}`)
}

export async function updateLocalTrack(
  id: number,
  input: LocalTrackInput,
): Promise<LocalTrack> {
  return apiService.patch(`/local-music/${id}`, input)
}

export async function uploadLocalTrack(form: FormData): Promise<LocalTrack> {
  return apiService.post<LocalTrack>('/local-music/upload', form, {
    timeout: LOCAL_MUSIC_UPLOAD_TIMEOUT_MS,
  })
}

/** Guest LRC endpoint; used by the admin editor to preview lyrics. */
export async function fetchLocalLyrics(id: number): Promise<string> {
  const body = await apiService.get<{ lrc?: string }>(
    `/proxy/music/local/lyrics/${id}`,
  )
  return body.lrc ?? ''
}

export async function listLocalPlaylists(): Promise<LocalPlaylist[]> {
  const body = await apiService.get<{ playlists: LocalPlaylist[] }>(
    '/local-music/playlists',
  )
  return body.playlists
}

export async function createLocalPlaylist(
  input: LocalPlaylistInput,
): Promise<LocalPlaylist> {
  return apiService.post('/local-music/playlists', input)
}

export async function updateLocalPlaylist(
  id: number,
  input: LocalPlaylistInput,
): Promise<LocalPlaylist> {
  return apiService.patch(`/local-music/playlists/${id}`, input)
}

export async function deleteLocalPlaylist(id: number): Promise<void> {
  await apiService.delete(`/local-music/playlists/${id}`)
}

export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor((ms || 0) / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m}:${String(s).padStart(2, '0')}`
}

export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '—'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
