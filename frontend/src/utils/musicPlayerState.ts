export interface MusicColorPalette {
  primary: string
  secondary: string
  accent: string
  light: string
  dark: string
}

export interface MusicPlayerSongLike {
  id?: string | number
  name?: string
  title?: string
  artist?: string
  album?: string
  cover?: string
  duration?: number
  url?: string
  source?: string
  isVip?: boolean
  isTrial?: boolean
}

export interface MusicPlayerSnapshotInput {
  song: MusicPlayerSongLike | null | Record<string, unknown>
  index: number
  colors: MusicColorPalette | null
  isPlaying: boolean
  isEnabled: boolean
  volume: number
  playMode: string
  playlist: ReadonlyArray<MusicPlayerSongLike | Record<string, unknown>>
  isTempPlay: boolean
  resetProgress: boolean
  liveCurrentTime?: number
  liveDuration?: number
  lyrics?: unknown[]
  verbatimLyrics?: unknown[]
  hasVerbatimLyrics?: boolean
  verbatimLyricsSource?: string
  currentLyricIndex?: number
  generation?: number
  isLoading?: boolean
}

export type MusicPlayerSnapshot = Record<string, unknown>

export const DEFAULT_MUSIC_COLOR = '#ef4444'

export function resolveMusicPalette(
  preferred: MusicColorPalette | null | undefined,
  previous: MusicColorPalette | null | undefined,
): MusicColorPalette | null {
  return preferred ?? previous ?? null
}

export function readLiveAudioProgress(
  audio: {
    currentTime?: number
    duration?: number
  } | null,
): { currentTime: number; audioDuration: number } {
  if (!audio) return { currentTime: 0, audioDuration: 0 }
  const currentTime =
    typeof audio.currentTime === 'number' && Number.isFinite(audio.currentTime)
      ? audio.currentTime
      : 0
  const audioDuration =
    typeof audio.duration === 'number' &&
    Number.isFinite(audio.duration) &&
    audio.duration > 0
      ? audio.duration
      : 0
  return { currentTime, audioDuration }
}

export function buildMusicPlayerSnapshot(
  input: MusicPlayerSnapshotInput,
): MusicPlayerSnapshot {
  const {
    song,
    index,
    colors,
    isPlaying,
    isEnabled,
    volume,
    playMode,
    playlist,
    isTempPlay,
    resetProgress,
    generation,
    isLoading,
  } = input

  const currentTime = resetProgress ? 0 : (input.liveCurrentTime ?? 0)
  const songDuration =
    song && typeof (song as MusicPlayerSongLike).duration === 'number'
      ? (song as MusicPlayerSongLike).duration
      : 0
  const audioDuration = resetProgress
    ? 0
    : (input.liveDuration ?? songDuration ?? 0)

  const lyrics = resetProgress ? [] : (input.lyrics ?? [])
  const verbatimLyrics = resetProgress ? [] : (input.verbatimLyrics ?? [])
  const hasVerbatimLyrics = resetProgress
    ? false
    : (input.hasVerbatimLyrics ?? verbatimLyrics.length > 0)
  const verbatimLyricsSource = resetProgress
    ? ''
    : (input.verbatimLyricsSource ?? '')
  const currentLyricIndex = resetProgress ? -1 : (input.currentLyricIndex ?? -1)

  const musicColor = colors?.primary ?? null

  return {
    currentSong: song,
    isEnabled,
    isPlaying,
    musicColor: musicColor || DEFAULT_MUSIC_COLOR,
    musicColors: colors,
    isTempPlay,
    currentSongIndex: index,
    playlistLength: playlist.length,
    playlist,
    currentTime,
    audioDuration,
    volume,
    playMode,
    lyrics,
    verbatimLyrics,
    hasVerbatimLyrics,
    verbatimLyricsSource,
    currentLyricIndex,
    generation: generation ?? 0,
    isAudioLoading: Boolean(isLoading),
    ...(resetProgress ? { lastPlaybackError: null as string | null } : {}),
  }
}

/** On song change, do not keep old lyrics/progress. */
export function mergeMusicPlayerEventDetail(
  globalState: Record<string, unknown>,
  detail: Record<string, unknown>,
): Record<string, unknown> {
  const merged: Record<string, unknown> = { ...globalState, ...detail }

  const detailSong = detail.currentSong as
    | { id?: string | number; duration?: number }
    | null
    | undefined
  const globalSong = globalState.currentSong as
    | { id?: string | number }
    | null
    | undefined
  const detailId = detailSong?.id
  const globalId = globalSong?.id

  const songChanged =
    detailId != null &&
    globalId != null &&
    String(detailId) !== String(globalId)

  if (songChanged) {
    if (!Object.hasOwn(detail, 'lyrics')) {
      merged.lyrics = []
      merged.currentLyricIndex = -1
    }
    if (!Object.hasOwn(detail, 'verbatimLyrics')) {
      merged.verbatimLyrics = []
      merged.hasVerbatimLyrics = false
      merged.verbatimLyricsSource = ''
    }
    if (!Object.hasOwn(detail, 'currentTime')) {
      merged.currentTime = 0
    }
    if (!Object.hasOwn(detail, 'audioDuration')) {
      merged.audioDuration = detailSong?.duration || 0
    }
  }

  return merged
}

export function buildTappMediaState(detail: Record<string, unknown>) {
  const modeMap: Record<string, string> = {
    loop: 'loop',
    single: 'single',
    shuffle: 'shuffle',
  }
  const currentSong = detail.currentSong as Record<string, unknown> | null
  const currentTime = (detail.currentTime as number) || 0
  const audioDuration =
    (detail.audioDuration as number) ||
    (currentSong?.duration as number) ||
    0
  const volume = (detail.volume as number) ?? 0.7
  const playMode = (detail.playMode as string) || 'loop'
  const musicColors = detail.musicColors as MusicColorPalette | null
  const musicColor =
    musicColors?.primary ||
    (detail.musicColor as string) ||
    '#fc3c44'
  const hasRealPalette = Boolean(musicColors?.primary)

  return {
    isPlaying: detail.isPlaying || false,
    isPaused: !detail.isPlaying && currentSong !== null,
    isLoading: Boolean(
      detail.isAudioLoading ?? detail.isLoading ?? false,
    ),
    generation:
      typeof detail.generation === 'number' ? detail.generation : 0,
    currentTrack: currentSong
      ? {
          id: currentSong.id || '',
          title: currentSong.name || currentSong.title || '',
          name: currentSong.name || currentSong.title || '',
          artist: currentSong.artist || '',
          album: currentSong.album || '',
          cover: currentSong.cover || '',
          duration: currentSong.duration || 0,
          source: currentSong.source || '',
          isVip: Boolean(currentSong.isVip),
          isTrial: Boolean(currentSong.isTrial),
        }
      : null,
    progress: {
      current: currentTime,
      duration: audioDuration,
      percentage: audioDuration > 0 ? (currentTime / audioDuration) * 100 : 0,
    },
    position: currentTime,
    volume: Math.round(volume * 100),
    mode: modeMap[playMode] || 'sequence',
    muted: volume === 0,
    lyrics: detail.lyrics || [],
    currentLyricIndex: (detail.currentLyricIndex as number) ?? -1,
    primaryColor: musicColor,
    secondaryColor: hasRealPalette
      ? musicColors!.secondary || musicColor
      : musicColor,
    accentColor: hasRealPalette
      ? musicColors!.accent || musicColor
      : musicColor,
    lightColor: hasRealPalette
      ? musicColors!.light || '#ffffff'
      : '#ffffff',
    darkColor: hasRealPalette ? musicColors!.dark || '#000000' : '#000000',
    hasThemePalette: hasRealPalette,
    lastError:
      (detail.lastPlaybackError as string | null | undefined) ??
      (detail.lastError as string | null | undefined) ??
      null,
  }
}

/** Do not write event fragments (currentTime:0) into global state. */
const MUSIC_CONTEXT_COERCERS = {
  currentSong: (v: unknown) => v ?? null,
  isEnabled: (v: unknown) => Boolean(v),
  isPlaying: (v: unknown) => Boolean(v),
  musicColor: (v: unknown) => String(v || '#ef4444'),
  isTempPlay: (v: unknown) => Boolean(v),
  currentSongIndex: (v: unknown) => Number(v) || 0,
  playlistLength: (v: unknown) => Number(v) || 0,
  playlist: (v: unknown) => v || [],
  lyrics: (v: unknown) => v || [],
  verbatimLyrics: (v: unknown) => v || [],
  hasVerbatimLyrics: (v: unknown) => Boolean(v),
  verbatimLyricsSource: (v: unknown) => v || '',
  currentLyricIndex: (v: unknown) => (typeof v === 'number' ? v : -1),
} as const

export type MusicContextOwnedKey = keyof typeof MUSIC_CONTEXT_COERCERS

export const MUSIC_CONTEXT_OWNED_KEYS = Object.keys(
  MUSIC_CONTEXT_COERCERS,
) as MusicContextOwnedKey[]

/** Preserve snapshot identity when an event changes only host-owned fields. */
export function mergeMusicContextState<T extends object>(
  previous: T,
  patch: Partial<T>,
): T {
  for (const key of Object.keys(patch) as (keyof T)[]) {
    if (!Object.is(previous[key], patch[key])) {
      return { ...previous, ...patch }
    }
  }
  return previous
}

export function pickMusicContextState(
  detail: Record<string, unknown>,
): Partial<Record<MusicContextOwnedKey, unknown>> {
  const out: Record<string, unknown> = {}
  for (const key of MUSIC_CONTEXT_OWNED_KEYS) {
    if (Object.hasOwn(detail, key)) out[key] = MUSIC_CONTEXT_COERCERS[key](detail[key])
  }
  return out
}
