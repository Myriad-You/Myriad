/**
 * 音乐播放器状态发布 / 合并的纯逻辑
 *
 * 宿主唯一写入 __musicPlayerState；Tapp 沙箱用 merge 规则消费事件，
 * 避免「部分字段事件」把进度/歌词编造成 0 或串曲。
 */

export interface MusicColorPalette {
  primary: string
  secondary: string
  accent: string
  light: string
  dark: string
}

/** 与宿主 Song 结构兼容（不强加 index signature，避免与接口冲突） */
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
  // 用宽松对象类型接纳宿主 Song，避免结构赋值摩擦
  song: MusicPlayerSongLike | null | Record<string, unknown>
  index: number
  colors: MusicColorPalette | null
  isPlaying: boolean
  isEnabled: boolean
  volume: number
  playMode: string
  playlist: ReadonlyArray<MusicPlayerSongLike | Record<string, unknown>>
  isTempPlay: boolean
  /** 切歌时 true：进度归零并清空歌词字段 */
  resetProgress: boolean
  /** 实时音频时钟；resetProgress 时忽略 */
  liveCurrentTime?: number
  liveDuration?: number
  /** 非切歌时保留的歌词（切歌时强制空） */
  lyrics?: unknown[]
  verbatimLyrics?: unknown[]
  hasVerbatimLyrics?: boolean
  verbatimLyricsSource?: string
  currentLyricIndex?: number
  /** 切歌世代，供 Tapp 丢弃过期事件 */
  generation?: number
  /** 音频缓冲/切歌加载中 */
  isLoading?: boolean
}

export type MusicPlayerSnapshot = Record<string, unknown>

export const DEFAULT_MUSIC_COLOR = '#ef4444'

/** 优先当前 palette，否则沿用上一首；都没有才 null（调用方再 fallback 默认色）。 */
export function resolveMusicPalette(
  preferred: MusicColorPalette | null | undefined,
  previous: MusicColorPalette | null | undefined,
): MusicColorPalette | null {
  return preferred ?? previous ?? null
}

/**
 * 从 audio 元素读取实时进度（切歌热路径外使用，避免 React state 滞后）。
 */
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

/**
 * 构建可写入 __musicPlayerState 并派发 music-player-state-change 的完整快照。
 * 颜色-only 更新时 resetProgress=false，保留 live 进度与歌词。
 */
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

  // colors 为 null 时不写死默认红（调用方应先 resolve 上一首色；首启无色才 fallback）
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

/**
 * Tapp 侧合并规则：detail 覆盖 global；切歌时禁止沿用旧曲歌词/进度。
 */
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
    if (!('lyrics' in detail)) {
      merged.lyrics = []
      merged.currentLyricIndex = -1
    }
    if (!('verbatimLyrics' in detail)) {
      merged.verbatimLyrics = []
      merged.hasVerbatimLyrics = false
      merged.verbatimLyricsSource = ''
    }
    if (!('currentTime' in detail)) {
      merged.currentTime = 0
    }
    if (!('audioDuration' in detail)) {
      merged.audioDuration = detailSong?.duration || 0
    }
  }

  return merged
}

/**
 * 从宿主快照构建 Tapp mediaStateChange payload（字段映射）。
 */
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
  // 有完整 palette 时以其 primary 为准；无色时仍给占位，Tapp 端会忽略 fallback 并保留 lastColors
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
    /** true 表示 primary 来自真实 palette，非仅占位默认色 */
    hasThemePalette: hasRealPalette,
    // 宿主播放错误码（如 playback_failed）；成功加载后为 null
    lastError:
      (detail.lastPlaybackError as string | null | undefined) ??
      (detail.lastError as string | null | undefined) ??
      null,
  }
}

/**
 * Context 订阅层只认这些字段，禁止把 currentTime:0 等事件碎片写回全局态。
 *
 * 收敛规则和字段名放在同一张表里：事件从宿主和沙箱两边来，值的类型不保证，
 * 写回前要统一。这张表以前在 MusicPlayerContext 里手抄了两份，加字段时漏改
 * 一处的表现是该字段静默不同步 —— 不报错、不崩，只是传不过去。
 */
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

/** 挑出 Context 拥有的字段并按上表收敛；未出现的键不会进结果。 */
export function pickMusicContextState(
  detail: Record<string, unknown>,
): Partial<Record<MusicContextOwnedKey, unknown>> {
  const out: Record<string, unknown> = {}
  for (const key of MUSIC_CONTEXT_OWNED_KEYS) {
    if (key in detail) out[key] = MUSIC_CONTEXT_COERCERS[key](detail[key])
  }
  return out
}
