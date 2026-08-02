/**
 * 音乐播放器状态管理 Hook
 * 从 GlobalControlPanel 分离出来的音乐播放器核心逻辑
 */

import type { ColorPalette } from '../utils/colorExtractor'
import type {
  LyricLine,
  MusicSource,
  Song,
  VerbatimLyricsSource,
  WordLyricLine,
} from '../utils/musicPlayer'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  extractColorsFromImage,
  extractColorsFromLoadedImage,
  getCachedPalette,
  isDefaultPalette,
  setCachedPalette,
} from '../utils/colorExtractor'
import { notifyHttpRateLimit } from '../utils/httpRateLimitToast'
import {
  audioManager,
  createPlaybackAudioElement,
  destroyPlaybackAudioElement,
  filterPlaylist,
  getCurrentLyricIndex,
  getLyricsWithVerbatim,
  getNeteasePlaylist,
  getNeteaseProxyFallbackUrl,
  getQQPlaylist,
  getQQProxyFallbackUrl,
  shouldPreserveNativeAudioOutput,
  throttle,
} from '../utils/musicPlayer'
import {
  buildMusicPlayerSnapshot,
  readLiveAudioProgress,
} from '../utils/musicPlayerState'
import { proxyImageUrlOr } from '../utils/proxyImageUrl'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { loadResource } from '../utils/resourceLoader'

// 播放模式类型
export type PlayMode = 'loop' | 'single' | 'shuffle'

// 音乐播放器视图类型
export type MusicPlayerView = 'info' | 'lyrics' | 'playlist'

// 临时播放模式状态
interface TempPlayMode {
  enabled: boolean
  originalPlaylist: Song[]
  originalIndex: number
  originalSource: MusicSource
  originalPlaylistId: string
}

// 音乐颜色类型
export interface MusicColors {
  primary: string
  secondary: string
  accent: string
  light: string
  dark: string
}

// Hook 返回的状态和方法
export interface UseMusicPlayerReturn {
  // 基本状态
  playlist: Song[]
  currentSongIndex: number
  currentSong: Song | null
  isPlaying: boolean
  isAudioLoading: boolean
  currentTime: number
  audioDuration: number
  volume: number
  lyrics: LyricLine[]
  verbatimLyrics: WordLyricLine[]
  hasVerbatimLyrics: boolean
  verbatimLyricsSource: VerbatimLyricsSource
  currentLyricIndex: number
  musicEnabled: boolean
  musicSource: MusicSource
  playlistId: string
  musicErrorKey: string // 翻译键名，由组件端使用 t.music[key] 翻译
  musicPlayerView: MusicPlayerView
  playMode: PlayMode
  musicColors: MusicColors | null

  // 搜索和过滤
  playlistSearchQuery: string
  excludeVipSongs: boolean
  filteredPlaylist: Song[]

  // 临时播放模式
  isTempPlayMode: boolean

  // 控制方法
  togglePlay: () => Promise<void>
  playPrevious: () => void
  playNext: () => void
  handleSeek: (time: number) => void
  handleSeekStart: () => void
  handleSeekEnd: () => void
  handleVolumeChange: (volume: number) => void
  togglePlayMode: () => void
  selectSong: (song: Song, index: number, autoPlay?: boolean) => Promise<void>
  playSong: (song: Song) => void
  stopTempPlay: () => Promise<void>
  setMusicPlayerView: (view: MusicPlayerView) => void
  setPlaylistSearchQuery: (query: string) => void
  setExcludeVipSongs: (exclude: boolean) => void
  loadMusicConfig: () => Promise<void>

  // Refs (供外部使用)
  audioRef: React.RefObject<HTMLAudioElement | null>
  playlistScrollRef: React.RefObject<HTMLDivElement | null>
  progressBarRef: React.RefObject<HTMLInputElement | null>
  musicContainerRef: React.RefObject<HTMLDivElement | null>
  volumeControlRef: React.RefObject<HTMLDivElement | null>

  // 音量弹出控制
  showVolumePopup: boolean
  setShowVolumePopup: (show: boolean) => void

  // 播放模式相关
  getPlayModeInfo: () => {
    icon: React.ReactNode
    textKey: 'singleRepeat' | 'shuffle' | 'listRepeat'
  }

  /**
   * 进度条 UI 可见性开关：不可见时 timeupdate 跳过 setCurrentTime，
   * 避免宿主组件（GlobalControlPanel）在面板收起时仍以 5次/秒 重渲染。
   * 对外的进度同步（Tapp 广播 / Media Session / 全局状态）不受影响。
   */
  setProgressUiVisible: (visible: boolean) => void
}

// 全局状态恢复（跨页面切换）- SSR 安全
const isBrowser = typeof window !== 'undefined'

function getGlobalState() {
  if (!isBrowser) return null
  return (window as any).__musicPlayerState
}

function setGlobalState(state: any) {
  if (!isBrowser) {
    return
  }
  ;(window as any).__musicPlayerState = {
    ...((window as any).__musicPlayerState || {}),
    ...state,
  }
}

/**
 * 立即补丁加载/错误标志并通知 Tapp（不依赖 React 下一帧）。
 * 用于 canplay / error 热路径，让缓冲条与错误提示及时出现。
 */
function patchPlaybackFlags(flags: {
  isAudioLoading?: boolean
  lastPlaybackError?: string | null
  generation?: number
}) {
  if (!isBrowser) return
  const prev = (window as any).__musicPlayerState || {}
  const next = { ...prev, ...flags }
  ;(window as any).__musicPlayerState = next
  window.dispatchEvent(
    new CustomEvent('music-player-state-change', {
      detail: {
        ...next,
        // 保证 Tapp merge 能读到标志位
        isAudioLoading: next.isAudioLoading,
        lastPlaybackError: next.lastPlaybackError ?? null,
        generation: next.generation ?? 0,
      },
    }),
  )
}

export function useMusicPlayer(): UseMusicPlayerReturn {
  // 基本状态 - 使用默认值初始化，避免 SSR 问题
  const [playlist, setPlaylist] = useState<Song[]>([])
  /** 始终最新的歌单（临时播放会在 setState 前先写 ref，避免闭包读到旧列表） */
  const playlistRef = useRef<Song[]>([])
  playlistRef.current = playlist
  const [currentSongIndex, setCurrentSongIndex] = useState(0)
  const [currentSong, setCurrentSong] = useState<Song | null>(null)
  const [isPlaying, setIsPlaying] = useState(false)
  const [isAudioLoading, setIsAudioLoading] = useState(false)
  const [currentTime, setCurrentTime] = useState(0)
  const [audioDuration, setAudioDuration] = useState(0)
  const [volume, setVolume] = useState(0.7)
  const [lyrics, setLyrics] = useState<LyricLine[]>([])
  const [verbatimLyrics, setVerbatimLyrics] = useState<WordLyricLine[]>([])
  const [verbatimLyricsSource, setVerbatimLyricsSource] =
    useState<VerbatimLyricsSource>('')
  const [currentLyricIndex, setCurrentLyricIndex] = useState(-1)
  const [musicEnabled, setMusicEnabled] = useState(false)
  const [musicSource, setMusicSource] = useState<MusicSource>('netease')
  const [playlistId, setPlaylistId] = useState('')
  const [musicErrorKey, setMusicErrorKey] = useState<string>('')
  const [musicPlayerView, setMusicPlayerView] =
    useState<MusicPlayerView>('info')
  const [playMode, setPlayMode] = useState<PlayMode>('loop')
  const [musicColors, setMusicColors] = useState<MusicColors | null>(null)
  /** 与 musicColors 同步，供 pushSongTheme 读最新色而不塞进 useCallback 依赖 */
  const musicColorsRef = useRef<MusicColors | null>(null)
  musicColorsRef.current = musicColors

  // 在客户端从全局状态恢复
  const initializedRef = useRef(false)
  useEffect(() => {
    if (initializedRef.current) return
    initializedRef.current = true

    const globalState = getGlobalState()
    if (globalState) {
      if (globalState.playlist) setPlaylist(globalState.playlist)
      if (typeof globalState.currentSongIndex === 'number')
        setCurrentSongIndex(globalState.currentSongIndex)
      if (globalState.currentSong) setCurrentSong(globalState.currentSong)
      if (typeof globalState.isEnabled === 'boolean')
        setMusicEnabled(globalState.isEnabled)
      if (Array.isArray(globalState.lyrics)) setLyrics(globalState.lyrics)
      if (Array.isArray(globalState.verbatimLyrics))
        setVerbatimLyrics(globalState.verbatimLyrics)
      if (typeof globalState.verbatimLyricsSource === 'string')
        setVerbatimLyricsSource(globalState.verbatimLyricsSource)
      if (typeof globalState.currentLyricIndex === 'number')
        setCurrentLyricIndex(globalState.currentLyricIndex)
    }
  }, [])

  // 搜索和过滤状态
  const [playlistSearchQuery, setPlaylistSearchQuery] = useState('')
  const [excludeVipSongs, setExcludeVipSongs] = useState(true)

  // 音量弹出控制
  const [showVolumePopup, setShowVolumePopup] = useState(false)

  // Refs
  const audioRef = useRef<HTMLAudioElement | null>(null)
  const preloadAudioRef = useRef<HTMLAudioElement | null>(null)
  const playlistScrollRef = useRef<HTMLDivElement>(null)
  const progressBarRef = useRef<HTMLInputElement>(null)
  const musicContainerRef = useRef<HTMLDivElement>(null)
  const volumeControlRef = useRef<HTMLDivElement>(null)
  const seekingRef = useRef<boolean>(false)

  // 进度条 UI 是否可见（面板收起时为 false，timeupdate 跳过 setCurrentTime）
  const progressUiVisibleRef = useRef<boolean>(false)

  // 歌词相关 Refs（避免频繁触发 effect）
  const lyricsRef = useRef<LyricLine[]>([])
  const verbatimLyricsRef = useRef<WordLyricLine[]>([])
  const currentLyricIndexRef = useRef<number>(-1)

  // 封面颜色缓存
  const colorCacheRef = useRef<Map<string, MusicColors>>(new Map())

  // Timeout 追踪
  const timeoutIdsRef = useRef<number[]>([])
  const lyricRequestKeyRef = useRef('')

  // 预加载系统
  const [preloadedSongIndex, setPreloadedSongIndex] = useState<number>(-1)
  const preloadCacheRef = useRef<Map<number, boolean>>(new Map())
  const preloadErrorCountRef = useRef<number>(0)
  const preloadDisabledUntilRef = useRef<number>(0)

  // 预加载触发控制
  const currentSongLoadedRef = useRef<boolean>(false)
  const currentSongStartTimeRef = useRef<number>(0)
  const preloadTriggeredRef = useRef<boolean>(false)

  // 随机播放模式的下一首索引
  const nextShuffleIndexRef = useRef<number>(-1)

  // 临时播放模式：ref 存现场，state 驱动 UI（关闭钮 / 列表位切换）
  const tempPlayModeRef = useRef<TempPlayMode>({
    enabled: false,
    originalPlaylist: [],
    originalIndex: 0,
    originalSource: 'netease',
    originalPlaylistId: '',
  })
  const [isTempPlayMode, setIsTempPlayMode] = useState(false)

  /**
   * 用户播放意图（与 audio.paused 解耦）。
   * 系统因切后台 pause 时不应清掉意图；用户点暂停 / 锁屏媒体键暂停时清掉。
   * 用于页面回到前台时决定是否自动 resume。
   */
  const userWantsPlayingRef = useRef(false)
  const isPlayingRef = useRef(false)
  const currentSongRef = useRef<Song | null>(null)
  /** 同步索引：连点 next/prev 时 React state 可能尚未提交，必须读 ref */
  const currentSongIndexRef = useRef(0)
  /** 每次 selectSong 递增；异步取色 / delayed play 用它丢弃过期结果 */
  const selectGenerationRef = useRef(0)
  const pendingPlayTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  /**
   * 方案 C：本会话内已对某网易曲尝试过「直连 → 全量代理」降级的 songId。
   * 防止代理也失败时在 error 里死循环。
   */
  const neteaseProxyFallbackTriedRef = useRef<Set<string>>(new Set())
  /**
   * 当前 audio 元素正在加载的曲目（与 selectGeneration 绑定）。
   * 用于丢弃切歌后旧 load 触发的 stale error，避免误降级/误跳下一首。
   */
  const audioLoadSongIdRef = useRef<string | null>(null)
  const audioLoadGenerationRef = useRef(0)
  isPlayingRef.current = isPlaying
  currentSongRef.current = currentSong
  currentSongIndexRef.current = currentSongIndex

  // 过滤后的播放列表
  // 临时播放（资料库/嵌入）须保留当前曲，即使它是 VIP 且开启了「隐藏 VIP」
  const filteredPlaylist = useMemo(() => {
    let filtered = filterPlaylist(playlist, playlistSearchQuery)
    if (excludeVipSongs && !isTempPlayMode) {
      filtered = filtered.filter((song) => !song.isVip)
    }
    return filtered
  }, [playlist, playlistSearchQuery, excludeVipSongs, isTempPlayMode])

  // 同步 lyrics 和 currentLyricIndex 到 ref + globalState
  // 直接更新 globalState，确保进度 tick 读到最新数据（不依赖 broadcastStateChange 触发）
  useEffect(() => {
    lyricsRef.current = lyrics
    const g = (window as any).__musicPlayerState
    if (g) g.lyrics = lyrics
  }, [lyrics])

  useEffect(() => {
    verbatimLyricsRef.current = verbatimLyrics
    const g = (window as any).__musicPlayerState
    if (g) {
      g.verbatimLyrics = verbatimLyrics
      g.hasVerbatimLyrics = verbatimLyrics.length > 0
      g.verbatimLyricsSource = verbatimLyricsSource
    }
  }, [verbatimLyrics, verbatimLyricsSource])

  useEffect(() => {
    currentLyricIndexRef.current = currentLyricIndex
    const g = (window as any).__musicPlayerState
    if (g) g.currentLyricIndex = currentLyricIndex
  }, [currentLyricIndex])

  // 验证并规范化颜色值
  const normalizeColor = useCallback((color: string): string => {
    const cleaned = color.trim().replace(/\s+/g, '')
    if (/^#([0-9A-F]{3}){1,2}$/i.test(cleaned)) {
      return cleaned.toLowerCase()
    }
    console.warn(`Invalid color format: "${color}", using fallback`)
    return '#999999'
  }, [])

  /**
   * 同步写入 --music-*（取色完成当帧生效，不经 React commit 多等一帧）。
   * useEffect 仍保留作 React 态回放/严格模式双写兜底。
   */
  const applyMusicCssVars = useCallback(
    (colors: MusicColors) => {
      const root = document.documentElement
      root.style.setProperty(
        '--music-primary',
        normalizeColor(colors.primary),
      )
      root.style.setProperty(
        '--music-secondary',
        normalizeColor(colors.secondary),
      )
      root.style.setProperty('--music-accent', normalizeColor(colors.accent))
      root.style.setProperty('--music-light', normalizeColor(colors.light))
      root.style.setProperty('--music-dark', normalizeColor(colors.dark))
    },
    [normalizeColor],
  )

  // 应用音乐颜色到全局作用域（null 时保留上一帧 CSS，避免切歌闪默认色）
  useEffect(() => {
    if (!musicColors) return
    applyMusicCssVars(musicColors)
  }, [musicColors, applyMusicCssVars])

  // 进度条 UI 可见性开关；恢复可见时立即同步一次进度，避免展示过期值
  const setProgressUiVisible = useCallback((visible: boolean) => {
    progressUiVisibleRef.current = visible
    if (visible && audioRef.current) {
      setCurrentTime(audioRef.current.currentTime)
    }
  }, [])

  // 为随机模式生成下一首歌曲索引
  const generateNextShuffleIndex = useCallback(
    (currentIndex: number) => {
      if (playlist.length <= 1) return -1

      const availableSongs = excludeVipSongs
        ? playlist
            .map((song, idx) => ({ song, idx }))
            .filter((item) => !item.song.isVip)
        : playlist.map((song, idx) => ({ song, idx }))

      if (availableSongs.length === 0) return -1

      const availableOptions = availableSongs.filter(
        (item) => item.idx !== currentIndex,
      )
      if (availableOptions.length === 0) return availableSongs[0].idx

      const randomItem =
        availableOptions[Math.floor(Math.random() * availableOptions.length)]
      return randomItem.idx
    },
    [playlist, excludeVipSongs],
  )

  // 预加载下一首歌曲
  const preloadNextSong = useCallback(
    (nextIndex: number, force: boolean = false) => {
      if (preloadDisabledUntilRef.current > Date.now()) {
        return
      }

      if (
        !preloadAudioRef.current ||
        nextIndex < 0 ||
        nextIndex >= playlist.length
      ) {
        return
      }

      if (preloadCacheRef.current.has(nextIndex)) {
        return
      }

      if (!force) {
        if (!currentSongLoadedRef.current) {
          return
        }

        const currentPlayTime = Date.now() - currentSongStartTimeRef.current
        if (currentPlayTime < 30000) {
          return
        }

        if (preloadTriggeredRef.current) {
          return
        }
      }

      const nextSong = playlist[nextIndex]
      if (!nextSong) return

      if (excludeVipSongs && nextSong.isVip) {
        return
      }

      preloadTriggeredRef.current = true

      loadResource.low(`music-preload-${nextIndex}`, async () => {
        const preloadAudio = preloadAudioRef.current
        if (!preloadAudio) return

        return new Promise<void>((resolve, reject) => {
          let usedFallback = false

          const failPreload = () => {
            preloadErrorCountRef.current += 1

            if (preloadErrorCountRef.current >= 3) {
              preloadDisabledUntilRef.current = Date.now() + 5 * 60 * 1000
              console.warn('音乐预加载已临时禁用5分钟')
            }

            cleanup()
            reject(new Error('Preload failed'))
          }

          const handleError = () => {
            // 方案 C：预加载直连失败 → 再试一次全量代理（网易 / QQ）
            if (!usedFallback) {
              const fallback =
                nextSong.source === 'netease'
                  ? getNeteaseProxyFallbackUrl(nextSong)
                  : nextSong.source === 'qq'
                    ? getQQProxyFallbackUrl(nextSong)
                    : null
              if (fallback) {
                usedFallback = true
                neteaseProxyFallbackTriedRef.current.add(nextSong.id)
                console.warn(
                  `[MusicPlayer] 预加载直连失败，降级代理: ${nextSong.name}`,
                )
                setPlaylist((prev) =>
                  prev.map((s) =>
                    s.id === nextSong.id &&
                    (s.source === 'netease' || s.source === 'qq')
                      ? { ...s, url: fallback }
                      : s,
                  ),
                )
                preloadAudio.src = fallback
                preloadAudio.load()
                return
              }
            }
            failPreload()
          }

          const handleCanPlay = () => {
            preloadErrorCountRef.current = 0
            setPreloadedSongIndex(nextIndex)
            preloadCacheRef.current.set(nextIndex, true)

            if (preloadCacheRef.current.size > 1) {
              const oldestKey = Array.from(preloadCacheRef.current.keys())[0]
              preloadCacheRef.current.delete(oldestKey)
            }

            cleanup()
            resolve()
          }

          const cleanup = () => {
            preloadAudio.removeEventListener('error', handleError)
            preloadAudio.removeEventListener('canplay', handleCanPlay)
          }

          preloadAudio.addEventListener('error', handleError)
          preloadAudio.addEventListener('canplay', handleCanPlay)

          preloadAudio.src = nextSong.url
          preloadAudio.load()
        })
      })
    },
    [playlist, excludeVipSongs],
  )

  // 广播状态变化事件 - 使用 ref 避免重复广播
  const lastBroadcastRef = useRef<string>('')
  const broadcastStateChange = useCallback(() => {
    // 进度必须读 audio 实时时钟，不能用 React 的 currentTime：
    // 宿主进度条不可见时 timeupdate 会跳过 setCurrentTime（避免面板收起仍 5 次/秒重渲染），
    // 但歌词换句仍会走这里广播关键状态。若写进过期 currentTime，Tapp 进度条会先回退、
    // 再被下一次 music-player-progress 拉回（「换句时进度条闪退」）。
    const audio = audioRef.current
    const liveCurrentTime =
      audio && Number.isFinite(audio.currentTime)
        ? audio.currentTime
        : currentTime
    const liveDuration =
      audio && Number.isFinite(audio.duration) && audio.duration > 0
        ? audio.duration
        : audioDuration

    // 创建状态快照用于比较（currentTime 按秒取整，避免过于频繁的更新）
    const stateSnapshot = JSON.stringify({
      songId: currentSong?.id,
      isEnabled: musicEnabled,
      isPlaying,
      color: musicColors?.primary,
      isTempPlay: tempPlayModeRef.current.enabled,
      index: currentSongIndex,
      length: playlist.length,
      time: Math.floor(liveCurrentTime), // 按秒取整
      volume: Math.round(volume * 100),
      mode: playMode,
      lyrics: lyrics.length,
      lyricIndex: currentLyricIndex,
      verbatim: verbatimLyrics.length,
      verbatimSource: verbatimLyricsSource,
    })

    // 如果状态没有变化，跳过广播
    if (lastBroadcastRef.current === stateSnapshot) {
      return
    }
    lastBroadcastRef.current = stateSnapshot

    // 主题色：优先 React 态，否则沿用全局已有色，避免广播时塞默认红
    const prevG = (window as any).__musicPlayerState as
      | Record<string, unknown>
      | undefined
    const resolvedPalette =
      musicColors ||
      (prevG?.musicColors as MusicColors | null | undefined) ||
      null
    const resolvedMusicColor =
      resolvedPalette?.primary ||
      (typeof prevG?.musicColor === 'string' ? prevG.musicColor : null) ||
      null

    // 🎯 同步更新全局状态（供 Tapp API 读取）— 写入完整状态，不依赖 Context 转手
    ;(window as any).__musicPlayerState = {
      ...((window as any).__musicPlayerState || {}),
      currentSong,
      isEnabled: musicEnabled,
      isPlaying,
      musicColor: resolvedMusicColor || '#ef4444',
      musicColors: resolvedPalette,
      isTempPlay: tempPlayModeRef.current.enabled,
      currentSongIndex,
      playlistLength: playlist.length,
      playlist,
      currentTime: liveCurrentTime,
      audioDuration: liveDuration,
      volume,
      playMode,
      lyrics,
      verbatimLyrics,
      hasVerbatimLyrics: verbatimLyrics.length > 0,
      verbatimLyricsSource,
      currentLyricIndex,
      generation: selectGenerationRef.current,
      isAudioLoading,
    }

    window.dispatchEvent(
      new CustomEvent('music-player-state-change', {
        detail: {
          currentSong,
          isEnabled: musicEnabled,
          isPlaying,
          musicColor: resolvedMusicColor || '#ef4444',
          musicColors: resolvedPalette,
          isTempPlay: tempPlayModeRef.current.enabled,
          currentSongIndex,
          playlistLength: playlist.length,
          playlist,
          // 🎯 添加实时播放信息（供 Tapp 使用）
          currentTime: liveCurrentTime,
          audioDuration: liveDuration,
          generation: selectGenerationRef.current,
          isAudioLoading,
          volume,
          playMode,
          // 🎯 添加歌词信息
          lyrics,
          verbatimLyrics,
          hasVerbatimLyrics: verbatimLyrics.length > 0,
          verbatimLyricsSource,
          currentLyricIndex,
        },
      }),
    )
  }, [
    currentSong,
    musicEnabled,
    isPlaying,
    musicColors,
    currentSongIndex,
    playlist,
    currentTime,
    audioDuration,
    volume,
    playMode,
    lyrics,
    verbatimLyrics,
    verbatimLyricsSource,
    currentLyricIndex,
    isAudioLoading,
  ])

  const resetLyrics = useCallback(() => {
    setLyrics([])
    setVerbatimLyrics([])
    setVerbatimLyricsSource('')
    setCurrentLyricIndex(-1)
    const g = (window as any).__musicPlayerState
    if (g) {
      g.lyrics = []
      g.verbatimLyrics = []
      g.hasVerbatimLyrics = false
      g.verbatimLyricsSource = ''
      g.currentLyricIndex = -1
    }
  }, [])

  const loadLyricsForSong = useCallback(
    (song: Song) => {
      const requestKey = `${song.source}-${song.id}`
      lyricRequestKeyRef.current = requestKey
      resetLyrics()

      // 不走 loadResource.completed 缓存：固定 id 首次完成后，二次点同一曲会
      // resetLyrics 后任务被 addTask 直接跳过，资料库/面板歌词永久空白。
      // HTTP 层仍可由 getLyricsWithVerbatim / 浏览器缓存复用。
      void (async () => {
        try {
          const result = await getLyricsWithVerbatim(song)
          if (lyricRequestKeyRef.current !== requestKey) return

          setLyrics(result.lines)
          setVerbatimLyrics(result.verbatim)
          setVerbatimLyricsSource(result.verbatimSource)
          setCurrentLyricIndex(-1)
        } catch (_error) {
          if (lyricRequestKeyRef.current !== requestKey) return
          resetLyrics()
        }
      })()
    },
    [resetLyrics],
  )

  /**
   * 立即写入全局态并推送给 Tapp（不依赖 React 下一帧）。
   * - resetProgress=true：切歌，进度/歌词归零
   * - resetProgress=false：颜色-only 等补丁，保留 audio 实时进度与现有歌词
   * - colors=null：保留上一首主题色，绝不刷默认红/灰（等新曲取色完成再换）
   */
  const pushSongTheme = useCallback(
    (
      song: Song,
      index: number,
      colors: MusicColors | null,
      playing: boolean,
      options?: { resetProgress?: boolean },
    ) => {
      const resetProgress = options?.resetProgress ?? false

      const g = (window as any).__musicPlayerState as
        | Record<string, unknown>
        | undefined
      const prevColors =
        (g?.musicColors as MusicColors | null | undefined) ||
        musicColorsRef.current ||
        null
      // 未命中缓存时沿用上一首 palette，避免切歌瞬间默认色闪烁
      const resolvedColors = colors ?? prevColors

      if (colors) {
        // 同步写 CSS，避免等 useEffect 再晚一帧（取色「不及时」的主因之一）
        applyMusicCssVars(colors)
        setMusicColors(colors)
        musicColorsRef.current = colors
      }
      // colors === null：不 setMusicColors(null)，CSS 变量与 React 态保持上一首

      const live = resetProgress
        ? { currentTime: 0, audioDuration: 0 }
        : readLiveAudioProgress(audioRef.current)

      const detail = buildMusicPlayerSnapshot({
        song,
        index,
        colors: resolvedColors,
        isPlaying: playing,
        isEnabled: musicEnabled,
        volume,
        playMode,
        // 读 ref：临时播放 setPlaylist 后若立刻 selectSong，闭包里的 playlist 仍是旧列表
        playlist: playlistRef.current,
        isTempPlay: tempPlayModeRef.current.enabled,
        resetProgress,
        liveCurrentTime: live.currentTime,
        liveDuration: live.audioDuration,
        lyrics: resetProgress ? [] : ((g?.lyrics as LyricLine[]) ?? []),
        verbatimLyrics: resetProgress
          ? []
          : ((g?.verbatimLyrics as WordLyricLine[]) ?? []),
        hasVerbatimLyrics: resetProgress
          ? false
          : Boolean(g?.hasVerbatimLyrics),
        verbatimLyricsSource: resetProgress
          ? ''
          : String(g?.verbatimLyricsSource || ''),
        currentLyricIndex: resetProgress
          ? -1
          : typeof g?.currentLyricIndex === 'number'
            ? g.currentLyricIndex
            : -1,
        generation: selectGenerationRef.current,
        isLoading: resetProgress
          ? true
          : Boolean(
              (g as { isAudioLoading?: boolean } | undefined)?.isAudioLoading,
            ),
      })

      setGlobalState(detail)
      window.dispatchEvent(
        new CustomEvent('music-player-state-change', { detail }),
      )
    },
    [musicEnabled, volume, playMode, applyMusicCssVars],
  )

  /** 写入双缓存，保持 hook colorCache 与 extractor 内存缓存一致 */
  const rememberCoverColors = useCallback(
    (cover: string, colors: MusicColors) => {
      if (isDefaultPalette(colors)) return
      if (colorCacheRef.current.size >= 50) {
        const firstKey = colorCacheRef.current.keys().next().value
        if (firstKey !== undefined) colorCacheRef.current.delete(firstKey)
      }
      colorCacheRef.current.set(cover, colors)
      setCachedPalette(cover, colors)
    },
    [],
  )

  /**
   * 从已渲染的封面 <img> 同步取色（零网络）。
   * 显示与取色共用同一张代理图时最稳；小尺寸 URL / 二次请求失败时的主兜底。
   */
  const tryExtractFromDomCover = useCallback(
    (cover: string): MusicColors | null => {
      const root = musicContainerRef.current
      if (!root || !cover) return null
      const img = root.querySelector(
        '.music-album-cover-large img',
      ) as HTMLImageElement | null
      if (!img?.complete) return null
      if ((img.naturalWidth || 0) <= 2 || (img.naturalHeight || 0) <= 2) {
        return null
      }
      const src = img.currentSrc || img.src || ''
      if (!src) return null
      try {
        const resolvedCover = new URL(cover, window.location.href).href
        const resolvedSrc = new URL(src, window.location.href).href
        if (resolvedCover !== resolvedSrc) return null
      } catch {
        if (src !== cover && !src.includes(cover) && !cover.includes(src)) {
          return null
        }
      }
      const palette = extractColorsFromLoadedImage(img)
      if (isDefaultPalette(palette)) return null
      return palette as MusicColors
    },
    [],
  )

  /**
   * 当前曲封面取色 + 失败后延迟再试（连点 abort / 瞬时网络失败后仍能补色）。
   * generation 过期或曲目已变则放弃。
   */
  const extractCoverColorsForSong = useCallback(
    (
      song: Song,
      index: number,
      generation: number,
      settleAttempt: number = 0,
    ) => {
      if (!song.cover) return
      const cover = song.cover
      const isCurrent = () =>
        selectGenerationRef.current === generation &&
        currentSongRef.current?.id === song.id

      const applyIfCurrent = (colors: MusicColors) => {
        if (!isCurrent() || isDefaultPalette(colors)) return false
        rememberCoverColors(cover, colors)
        pushSongTheme(
          song,
          index,
          colors,
          !!(audioRef.current && !audioRef.current.paused),
          { resetProgress: false },
        )
        return true
      }

      // 已有真实色则跳过
      const cached =
        colorCacheRef.current.get(cover) ||
        (getCachedPalette(cover) as MusicColors | null)
      if (cached && !isDefaultPalette(cached)) {
        applyIfCurrent(cached)
        return
      }

      // DOM 封面已解码：同步取色，避免再打一枪代理
      const fromDom = tryExtractFromDomCover(cover)
      if (fromDom) {
        applyIfCurrent(fromDom)
        return
      }

      const musicContainer = musicContainerRef.current
      if (settleAttempt === 0 && musicContainer) {
        musicContainer.classList.add('color-transitioning')
      }

      void extractColorsFromImage(cover, {
        context: 'music',
        priority: 'high',
        // settle 重试时强制绕过可能卡住的 in-flight
        forceRefresh: settleAttempt > 0,
      })
        .then((palette: ColorPalette) => {
          if (!isCurrent()) return
          let colors = palette as MusicColors
          // 网络路径仍失败：再试一次 DOM（封面可能刚好 onload）
          if (isDefaultPalette(colors)) {
            const domRetry = tryExtractFromDomCover(cover)
            if (domRetry) colors = domRetry
          }
          if (isDefaultPalette(colors)) {
            // extractor 已内部重试仍失败 → 宿主侧再排一次 settle
            if (settleAttempt < 3) {
              const delay = 200 * 2 ** settleAttempt
              window.setTimeout(() => {
                if (!isCurrent()) return
                extractCoverColorsForSong(
                  song,
                  index,
                  generation,
                  settleAttempt + 1,
                )
              }, delay)
            }
            if (musicContainer) {
              musicContainer.classList.remove('color-transitioning')
            }
            return
          }
          applyIfCurrent(colors)
          if (musicContainer) {
            musicContainer.classList.remove('color-transitioning')
          }
        })
        .catch((error) => {
          if (!isCurrent()) return
          // abort 不算失败；切走后 generation 会变
          const msg = error instanceof Error ? error.message : String(error)
          const aborted =
            msg.includes('cancel') ||
            msg.includes('Abort') ||
            msg.includes('aborted')
          if (aborted) {
            if (musicContainer) {
              musicContainer.classList.remove('color-transitioning')
            }
            return
          }
          // 网络失败：优先 DOM，再 settle
          const domRetry = tryExtractFromDomCover(cover)
          if (domRetry && applyIfCurrent(domRetry)) {
            if (musicContainer) {
              musicContainer.classList.remove('color-transitioning')
            }
            return
          }
          if (settleAttempt < 3) {
            const delay = 200 * 2 ** settleAttempt
            window.setTimeout(() => {
              if (!isCurrent()) return
              extractCoverColorsForSong(
                song,
                index,
                generation,
                settleAttempt + 1,
              )
            }, delay)
          } else {
            console.warn('Failed to extract colors from cover:', error)
          }
          if (musicContainer) {
            musicContainer.classList.remove('color-transitioning')
          }
        })
    },
    [pushSongTheme, rememberCoverColors, tryExtractFromDomCover],
  )

  /** 预取邻曲封面 + 取色入缓存，连点切歌时热命中（low 优先级，不打断当前曲） */
  const prefetchAroundIndex = useCallback(
    (center: number) => {
      const list = playlistRef.current
      if (!list.length) return
      const targets = [center - 1, center + 1, center + 2]
      for (const raw of targets) {
        if (raw < 0 || raw >= list.length || raw === center) continue
        const s = list[raw]
        if (!s?.cover) continue
        if (colorCacheRef.current.has(s.cover) || getCachedPalette(s.cover)) {
          continue
        }
        // 浏览器预解码封面（显示用全尺寸）
        try {
          const img = new Image()
          img.decoding = 'async'
          img.referrerPolicy = 'no-referrer'
          img.src = s.cover
        } catch {
          /* ignore */
        }
        void extractColorsFromImage(s.cover, {
          context: 'music',
          priority: 'low',
        })
          .then((palette) => {
            rememberCoverColors(s.cover, palette as MusicColors)
          })
          .catch(() => {})
      }
    },
    [rememberCoverColors],
  )

  // 选择歌曲
  const selectSong = useCallback(
    async (songIn: Song, index: number, autoPlay: boolean = false) => {
      // 歌单内跳过 VIP；临时播放（资料库/嵌入/Tapp 单曲）必须放行，否则 VIP 点了无声
      if (
        excludeVipSongs &&
        songIn.isVip &&
        !tempPlayModeRef.current.enabled
      ) {
        return
      }

      // 临时播放 / Tapp / 资料库入口常带裸 CDN 封面；统一代理后再取色（canvas CORS）
      const proxiedCover = proxyImageUrlOr(songIn.cover, songIn.cover || '')
      const song: Song =
        proxiedCover && proxiedCover !== songIn.cover
          ? { ...songIn, cover: proxiedCover }
          : songIn.cover
            ? songIn
            : { ...songIn, cover: proxiedCover }

      // 新一代切歌令牌：丢弃更早一次 select 的取色 / delayed play
      const generation = ++selectGenerationRef.current
      const isCurrentSelect = () => selectGenerationRef.current === generation

      if (pendingPlayTimeoutRef.current !== null) {
        clearTimeout(pendingPlayTimeoutRef.current)
        pendingPlayTimeoutRef.current = null
      }

      // 重置预加载状态
      currentSongLoadedRef.current = false
      currentSongStartTimeRef.current = 0
      preloadTriggeredRef.current = false

      // 同步更新 ref，保证连点 next/prev 读到最新索引
      currentSongIndexRef.current = index
      currentSongRef.current = song

      setCurrentSong(song)
      setCurrentSongIndex(index)
      setAudioDuration(0)
      setCurrentTime(0)

      // 切歌瞬间清空歌词/进度，避免 Tapp 合并全局态时「新歌 + 旧歌词」
      resetLyrics()

      // 同步解析主题色（双缓存），立刻推给 Tapp，避免带着上一首颜色
      let immediateColors: MusicColors | null = null
      if (song.cover) {
        immediateColors =
          colorCacheRef.current.get(song.cover) ||
          (getCachedPalette(song.cover) as MusicColors | null)
        if (immediateColors) {
          rememberCoverColors(song.cover, immediateColors)
        }
      }
      // 立刻推主题：缓存命中则新色；未命中则 pushSongTheme 内保留上一首色。
      // ⚠️ playing 必须传 false：autoPlay 也要等 audio 真正 play 事件再亮「播放中」，
      // 否则会出现频谱/按钮已在播、实际无声的「虚假播放」。
      pushSongTheme(song, index, immediateColors, false, {
        resetProgress: true,
      })
      setIsPlaying(false)
      // 清除上一首错误；标记缓冲（与 isAudioLoading 对齐）
      patchPlaybackFlags({
        isAudioLoading: true,
        lastPlaybackError: null,
        generation,
      })

      // 封面预解码（显示与取色并行，不阻塞音频）
      if (song.cover) {
        try {
          const warm = new Image()
          warm.decoding = 'async'
          warm.fetchPriority = 'high'
          warm.referrerPolicy = 'no-referrer'
          warm.src = song.cover
        } catch {
          /* ignore */
        }
      }

      // 加载歌词（低优先级）：逐字优先，逐行兜底
      loadLyricsForSong(song)

      // 音频立刻加载——不再等取色 await
      if (audioRef.current) {
        setIsAudioLoading(true)

        audioRef.current.pause()
        audioRef.current.currentTime = 0
        audioLoadSongIdRef.current = song.id
        audioLoadGenerationRef.current = generation
        audioRef.current.src = song.url
        audioRef.current.load()

        audioManager.setCurrentAudio(audioRef.current, song)

        if (autoPlay) {
          // 只记意图；isPlaying 仅由 audio 'play' 事件 / 确认 !paused 后置 true
          userWantsPlayingRef.current = true
          if (pendingPlayTimeoutRef.current !== null) {
            clearTimeout(pendingPlayTimeoutRef.current)
            pendingPlayTimeoutRef.current = null
          }
          void (async () => {
            if (!isCurrentSelect()) return
            if (currentSongRef.current?.id !== song.id) return
            const el = audioRef.current
            if (!el) return
            try {
              await el.play()
              if (!isCurrentSelect()) return
              // play() resolve 仍可能尚未真正出声（空 src / 立刻 pause）；以元素态为准
              if (el.paused) {
                setIsPlaying(false)
                audioManager.setPlaybackState('paused')
                return
              }
              setIsPlaying(true)
              audioManager.setPlaybackState('playing')
            } catch (_error) {
              if (!isCurrentSelect()) return
              // 策略拦截 / 尚未 canplay：保留 userWantsPlaying，等 canplay 再试
              // 不要清意图，否则临时播放点了永远不跟播
              setIsPlaying(false)
              audioManager.setPlaybackState('paused')
            }
          })()
        } else {
          userWantsPlayingRef.current = false
          setIsPlaying(false)
          audioManager.setPlaybackState('paused')
        }
      }

      // 随机模式需要提前确定下一首
      if (playMode === 'shuffle' && playlist.length > 1) {
        const nextIndex = generateNextShuffleIndex(index)
        if (nextIndex !== -1 && nextIndex !== index) {
          nextShuffleIndexRef.current = nextIndex
        }
      }

      // 异步取色（high + 失败 settle 重试）；须在邻曲 low 预取之前启动
      if (song.cover && !immediateColors) {
        extractCoverColorsForSong(song, index, generation, 0)
      }
      // 无封面：不 setMusicColors(null)，避免切到无封面曲时闪默认色

      // 邻曲封面/颜色预热（low，不打断当前曲 high 取色）
      prefetchAroundIndex(index)
    },
    [
      musicEnabled,
      playlist,
      playMode,
      volume,
      excludeVipSongs,
      generateNextShuffleIndex,
      loadLyricsForSong,
      resetLyrics,
      pushSongTheme,
      prefetchAroundIndex,
      rememberCoverColors,
      extractCoverColorsForSong,
    ],
  )

  // 播放单首歌曲（临时播放模式）
  const playSong = useCallback(
    (songIn: Song) => {
      // 资料库/嵌入/Tapp 常传裸 126.net 封面 → 必须代理，否则取色 canvas 被 CORS 污染
      const cover = proxyImageUrlOr(songIn.cover, songIn.cover || '')
      const song: Song = { ...songIn, cover }

      // 确保音频元素已初始化
      if (!audioRef.current) {
        audioRef.current = createPlaybackAudioElement(volume)
        audioManager.setCurrentAudio(audioRef.current, song)
      }

      if (!tempPlayModeRef.current.enabled) {
        // 用 ref 快照：避免 React 批更新期间读到中间态
        tempPlayModeRef.current = {
          enabled: true,
          originalPlaylist: [...playlistRef.current],
          originalIndex: currentSongIndexRef.current,
          originalSource: musicSource,
          originalPlaylistId: playlistId,
        }
        setIsTempPlayMode(true)
      }

      if (!musicEnabled) {
        setMusicEnabled(true)
        setMusicSource(song.source || 'netease')
      }

      // 先同步写 ref，再 setState：selectSong → pushSongTheme 立刻读到临时单曲列表
      const tempList = [song]
      playlistRef.current = tempList
      setPlaylist(tempList)

      // 同步切歌（不再 setTimeout）：旧写法会闭包住旧 selectSong/playlist，取色完成后
      // pushSongTheme 仍推旧歌单，主题/Tapp 态抖动，看起来像「取色不稳」
      void selectSong(song, 0, true)
    },
    [musicEnabled, volume, musicSource, playlistId, selectSong],
  )

  // 停止临时播放
  const stopTempPlay = useCallback(async () => {
    if (!tempPlayModeRef.current.enabled) return

    const {
      originalPlaylist,
      originalIndex,
      originalSource,
      originalPlaylistId,
    } = tempPlayModeRef.current

    tempPlayModeRef.current.enabled = false
    setIsTempPlayMode(false)

    if (audioRef.current) {
      audioRef.current.pause()
      audioRef.current.currentTime = 0
    }
    setIsPlaying(false)

    // 先写 ref 再 setState，保证紧接着的 selectSong 读到恢复后的歌单
    playlistRef.current = originalPlaylist
    setPlaylist(originalPlaylist)
    setMusicSource(originalSource)
    setPlaylistId(originalPlaylistId)

    if (originalPlaylist.length > 0 && originalPlaylist[originalIndex]) {
      await selectSong(originalPlaylist[originalIndex], originalIndex, false)
    } else {
      setCurrentSong(null)
    }
  }, [selectSong])

  // 加载歌单
  const loadPlaylist = useCallback(
    async (source: MusicSource, plistId: string, autoPlay: boolean = false) => {
      loadResource.medium(`music-playlist-${plistId}`, async () => {
        try {
          setMusicErrorKey('')
          // 换歌单后允许重新尝试直连（方案 C 降级标记重置）
          neteaseProxyFallbackTriedRef.current.clear()
          const songs =
            source === 'netease'
              ? await getNeteasePlaylist(plistId)
              : await getQQPlaylist(plistId)

          void import('../utils/analyticsEvents').then(
            ({ trackProductEvent, AnalyticsEvents }) => {
              trackProductEvent(AnalyticsEvents.MUSIC_SOURCE_SWITCH, {
                target: source,
                throttleMs: 10_000,
              })
            },
          )

          // 与临时播放相同：先写 ref 再 selectSong，否则 pushSongTheme 仍推旧歌单
          playlistRef.current = songs
          setPlaylist(songs)

          if (songs.length > 0) {
            let firstSongIndex = 0
            if (excludeVipSongs) {
              const nonVipIndex = songs.findIndex((song) => !song.isVip)
              if (nonVipIndex !== -1) {
                firstSongIndex = nonVipIndex
              }
            }
            selectSong(songs[firstSongIndex], firstSongIndex, autoPlay)
          } else {
            // 空结果与异常同样视为加载失败：无效歌单 ID（如 Agent 传参错误）
            // 通常返回空列表而非抛错，静默会让"叫了没反应"无从排查
            setMusicErrorKey('loadPlaylistFailed')
            setTimeout(() => {
              setMusicErrorKey('')
            }, 3000)
          }
        } catch (error) {
          console.error('Failed to load music playlist:', error)
          setMusicErrorKey('loadPlaylistFailed')
          playlistRef.current = []
          setPlaylist([])

          setTimeout(() => {
            setMusicErrorKey('')
          }, 3000)
        }
      })
    },
    [selectSong, excludeVipSongs],
  )

  // 加载音乐配置
  const loadMusicConfig = useCallback(async () => {
    try {
      preloadErrorCountRef.current = 0
      preloadDisabledUntilRef.current = 0

      // 走去重缓存：启动时与 Home/壁纸/页脚共享同一次 /api/config/ui 请求
      const data = await getUIConfigDeduped()

      const enabled = data.music_enabled === 'true'
      const source = data.music_source || 'netease'
      const { normalizeMusicPlaylistId } = await import('../utils/musicPlaylistId')
      const plistId = normalizeMusicPlaylistId(data.music_playlist_id || '')

      setMusicEnabled(enabled)
      setMusicSource(source as MusicSource)
      setPlaylistId(plistId)

      if (enabled && plistId) {
        loadPlaylist(source as MusicSource, plistId)
      }

      broadcastStateChange()
    } catch (_error) {
      // 静默处理
    }
  }, [loadPlaylist, broadcastStateChange])

  // 播放/暂停
  const togglePlay = useCallback(async () => {
    if (!audioRef.current || !currentSong) return

    if (isPlaying) {
      userWantsPlayingRef.current = false
      audioRef.current.pause()
      setIsPlaying(false)
      audioManager.setPlaybackState('paused')
      void import('../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.MUSIC_PAUSE, {
            target: currentSong.source || musicSource,
            throttleMs: 3000,
          })
        },
      )
    } else {
      userWantsPlayingRef.current = true
      const maxRetries = 3
      let retries = 0

      while (retries < maxRetries) {
        try {
          await audioRef.current.play()
          setIsPlaying(true)
          audioManager.setPlaybackState('playing')
          void import('../utils/analyticsEvents').then(
            ({ trackProductEvent, AnalyticsEvents }) => {
              trackProductEvent(AnalyticsEvents.MUSIC_PLAY, {
                target: currentSong.source || musicSource,
                throttleMs: 3000,
              })
            },
          )
          break
        } catch (error) {
          retries++
          console.warn(`播放失败，重试 ${retries}/${maxRetries}:`, error)

          if (retries >= maxRetries) {
            console.error('播放失败，已达到最大重试次数:', error)
            setMusicErrorKey('playFailed')
            setTimeout(setMusicErrorKey, 3000, '')
            setIsPlaying(false)
            userWantsPlayingRef.current = false
          } else {
            await new Promise((resolve) => setTimeout(resolve, 1000 * retries))
          }
        }
      }
    }
    // 不在此处调用 broadcastStateChange()：
    // 闭包捕获的是旧 isPlaying 值，await audio.play() 后执行会覆盖
    // useEffect 已在 isPlaying 变化时自动广播正确状态（line ~1513）
  }, [isPlaying, currentSong, musicSource])

  // 上一首
  const playPrevious = useCallback(() => {
    if (playlist.length === 0) return

    // 读同步 ref：连点时 React 的 currentSongIndex 可能还是上一次的
    const fromIndex = currentSongIndexRef.current
    let newIndex: number

    if (playMode === 'shuffle') {
      newIndex = generateNextShuffleIndex(fromIndex)
    } else {
      newIndex = fromIndex === 0 ? playlist.length - 1 : fromIndex - 1
      let attempts = 0

      if (excludeVipSongs) {
        while (playlist[newIndex]?.isVip && attempts < playlist.length) {
          newIndex = newIndex === 0 ? playlist.length - 1 : newIndex - 1
          attempts++
        }

        if (attempts >= playlist.length) {
          console.warn('所有歌曲都是VIP，无法播放')
          return
        }
      }
    }

    // 乐观推进 ref，使紧随其后的 next/prev 基于最新位置
    currentSongIndexRef.current = newIndex
    selectSong(playlist[newIndex], newIndex, true)
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.MUSIC_PREV, {
          target: musicSource,
          throttleMs: 2000,
        })
      },
    )
  }, [
    playlist,
    selectSong,
    excludeVipSongs,
    playMode,
    generateNextShuffleIndex,
    musicSource,
  ])

  // 下一首
  const playNext = useCallback(() => {
    if (playlist.length === 0) return

    // 读同步 ref：连点时 React 的 currentSongIndex 可能还是上一次的
    const fromIndex = currentSongIndexRef.current
    let newIndex: number

    if (playMode === 'shuffle') {
      newIndex =
        nextShuffleIndexRef.current !== -1
          ? nextShuffleIndexRef.current
          : generateNextShuffleIndex(fromIndex)
      // 消费预计算的下一首，避免连点反复落到同一预选索引
      nextShuffleIndexRef.current = -1
    } else {
      newIndex = (fromIndex + 1) % playlist.length
      let attempts = 0

      if (excludeVipSongs) {
        while (playlist[newIndex]?.isVip && attempts < playlist.length) {
          newIndex = (newIndex + 1) % playlist.length
          attempts++
        }

        if (attempts >= playlist.length) {
          console.warn('所有歌曲都是VIP，无法播放')
          return
        }
      }
    }

    // 乐观推进 ref，使紧随其后的 next/prev 基于最新位置
    currentSongIndexRef.current = newIndex
    selectSong(playlist[newIndex], newIndex, true)
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.MUSIC_NEXT, {
          target: musicSource,
          throttleMs: 2000,
        })
      },
    )
  }, [
    playlist,
    selectSong,
    excludeVipSongs,
    playMode,
    generateNextShuffleIndex,
    musicSource,
  ])

  // 调整音量
  const handleVolumeChange = useCallback((newVolume: number) => {
    const clampedVolume = Math.max(0, Math.min(1, newVolume))
    setVolume(clampedVolume)

    if (audioRef.current) {
      try {
        audioRef.current.volume = clampedVolume
      } catch (error) {
        console.warn('Failed to set audio volume:', error)
      }
    }

    if (preloadAudioRef.current) {
      try {
        preloadAudioRef.current.volume = clampedVolume
      } catch (_error) {
        // 静默处理
      }
    }
  }, [])

  // 调整播放进度（优先 live audio.duration / audioDuration，避免元数据时长过期）
  const handleSeek = useCallback(
    (time: number) => {
      const audio = audioRef.current
      if (!audio || !currentSong) return

      const liveDur =
        Number.isFinite(audio.duration) && audio.duration > 0
          ? audio.duration
          : 0
      const duration =
        liveDur > 0
          ? liveDur
          : audioDuration > 0
            ? audioDuration
            : currentSong.duration > 0
              ? currentSong.duration
              : 0

      let safeTime = Math.max(0, time)
      if (duration > 0) {
        const maxSeekTime =
          duration > 1 ? duration - 1 : Math.max(0, duration * 0.95)
        safeTime = Math.min(safeTime, maxSeekTime)
      }

      audio.currentTime = safeTime
      setCurrentTime(safeTime)
    },
    [currentSong, audioDuration],
  )

  // 进度条拖动开始
  const handleSeekStart = useCallback(() => {
    seekingRef.current = true
  }, [])

  // 进度条拖动结束
  const handleSeekEnd = useCallback(() => {
    setTimeout(() => {
      seekingRef.current = false
    }, 100)
  }, [])

  // 切换播放模式
  const togglePlayMode = useCallback(() => {
    setPlayMode((prev) => {
      if (prev === 'loop') return 'single'
      if (prev === 'single') return 'shuffle'
      return 'loop'
    })
  }, [])

  // 获取播放模式信息
  const getPlayModeInfo = useCallback(() => {
    switch (playMode) {
      case 'single':
        return {
          icon: (
            <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
              <path d="M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4zm-4-2V9h-1l-2 1v1h1.5v4H13z" />
            </svg>
          ),
          textKey: 'singleRepeat' as const,
        }
      case 'shuffle':
        return {
          icon: (
            <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
              <path d="M10.59 9.17L5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z" />
            </svg>
          ),
          textKey: 'shuffle' as const,
        }
      case 'loop':
      default:
        return {
          icon: (
            <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
              <path d="M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z" />
            </svg>
          ),
          textKey: 'listRepeat' as const,
        }
    }
  }, [playMode])

  // 初始化音频元素和事件监听
  useEffect(() => {
    if (!audioRef.current) {
      // 使用挂入 DOM 的 Audio，提升移动端后台播放稳定性
      audioRef.current = createPlaybackAudioElement(volume)
      audioManager.setCurrentAudio(audioRef.current, currentSong)
    }

    if (!preloadAudioRef.current) {
      preloadAudioRef.current = new Audio()
      preloadAudioRef.current.preload = 'auto'
      preloadAudioRef.current.volume = volume
    }

    const audio = audioRef.current
    let errorAdvanceTimer: ReturnType<typeof setTimeout> | null = null

    const handleTimeUpdate = throttle(() => {
      const currentTime = audio.currentTime
      // 进度条不可见时跳过 React 状态更新，避免宿主组件每 200ms 重渲染；
      // 下方的全局状态 / Media Session / 进度事件广播照常执行
      if (progressUiVisibleRef.current) {
        setCurrentTime(currentTime)
      }

      // 更新 Media Session 位置状态（移动端后台播放关键）
      if (audio.duration && Number.isFinite(audio.duration)) {
        audioManager.updatePositionState(
          audio.duration,
          currentTime,
          audio.playbackRate,
        )
      }

      // 🎯 实时更新全局状态并广播进度给 Tapp
      const globalState = (
        window as { __musicPlayerState?: Record<string, unknown> }
      ).__musicPlayerState
      if (globalState) {
        globalState.currentTime = currentTime
        globalState.audioDuration = audio.duration || 0
      }

      // 直接 dispatch 轻量进度事件，绕过 broadcastStateChange 节流链。
      // 带上 songId：切歌瞬间旧 audio 的 timeupdate 可被下游丢弃，避免进度串曲。
      window.dispatchEvent(
        new CustomEvent('music-player-progress', {
          detail: {
            currentTime,
            audioDuration: audio.duration || 0,
            songId: currentSongRef.current?.id ?? null,
          },
        }),
      )

      if (lyricsRef.current.length > 0) {
        const index = getCurrentLyricIndex(lyricsRef.current, currentTime)
        if (index !== currentLyricIndexRef.current) {
          // 同步写 ref，避免 throttle 期间重复 setState / 全局态滞后
          currentLyricIndexRef.current = index
          const g = (window as { __musicPlayerState?: Record<string, unknown> })
            .__musicPlayerState
          if (g) g.currentLyricIndex = index
          setCurrentLyricIndex(index)
        }
      }

      // 智能预加载触发
      if (currentSongLoadedRef.current && !preloadTriggeredRef.current) {
        const playTime = Date.now() - currentSongStartTimeRef.current
        if (playTime >= 30000) {
          if (playlist.length > 1) {
            if (playMode === 'loop') {
              let nextIndex = (currentSongIndex + 1) % playlist.length
              if (excludeVipSongs) {
                let attempts = 0
                while (
                  playlist[nextIndex]?.isVip &&
                  attempts < playlist.length
                ) {
                  nextIndex = (nextIndex + 1) % playlist.length
                  attempts++
                }
              }
              if (
                nextIndex !== currentSongIndex &&
                !playlist[nextIndex]?.isVip
              ) {
                preloadNextSong(nextIndex)
              }
            } else if (playMode === 'shuffle') {
              const nextIndex = generateNextShuffleIndex(currentSongIndex)
              if (nextIndex !== -1 && nextIndex !== currentSongIndex) {
                nextShuffleIndexRef.current = nextIndex
                preloadNextSong(nextIndex)
              }
            }
          }
        }
      }
    }, 200)

    const handleCanPlay = () => {
      if (
        selectGenerationRef.current !== audioLoadGenerationRef.current ||
        currentSongRef.current?.id !== audioLoadSongIdRef.current
      ) {
        return
      }
      if (!currentSongLoadedRef.current) {
        currentSongLoadedRef.current = true
        currentSongStartTimeRef.current = Date.now()
      }
      setIsAudioLoading(false)
      patchPlaybackFlags({
        isAudioLoading: false,
        lastPlaybackError: null,
        generation: selectGenerationRef.current,
      })

      // 用户点了播但首包 play() 因未缓冲失败：数据就绪后补一次真实播放
      if (userWantsPlayingRef.current && audio.paused) {
        void audio.play().then(
          () => {
            if (
              selectGenerationRef.current !== audioLoadGenerationRef.current ||
              currentSongRef.current?.id !== audioLoadSongIdRef.current
            ) {
              return
            }
            if (audio.paused) return
            setIsPlaying(true)
            audioManager.setPlaybackState('playing')
          },
          () => {
            /* 仍失败则等 error / 用户手势 */
          },
        )
      }

      // 连点后停在本曲：若封面色仍未命中缓存，再补一次取色（canplay 时机网络较稳）
      const settled = currentSongRef.current
      const gen = selectGenerationRef.current
      if (settled?.cover) {
        const hit =
          colorCacheRef.current.get(settled.cover) ||
          getCachedPalette(settled.cover)
        if (!hit || isDefaultPalette(hit)) {
          extractCoverColorsForSong(
            settled,
            currentSongIndexRef.current,
            gen,
            0,
          )
        }
      }
    }

    const handleLoadedMetadata = () => {
      if (audio.duration && Number.isFinite(audio.duration)) {
        setAudioDuration(audio.duration)
      }
    }

    const handleError = () => {
      console.error('音频播放错误:', audio.error)

      // 切歌后旧 load 的 abort/error：不降级、不跳曲
      const song = currentSongRef.current
      if (
        !song ||
        song.id !== audioLoadSongIdRef.current ||
        selectGenerationRef.current !== audioLoadGenerationRef.current
      ) {
        return
      }

      // Media element errors hide HTTP status; re-probe proxy URLs for 429 toast
      const probeUrl = song.url || ''
      if (
        probeUrl.includes('/api/proxy/music/') ||
        probeUrl.includes('/proxy/music/')
      ) {
        void fetch(probeUrl, { method: 'GET', cache: 'no-store' })
          .then(async (res) => {
            if (res.status !== 429) return
            let body: unknown
            try {
              body = await res.clone().json()
            } catch {
              body = undefined
            }
            notifyHttpRateLimit(res, body)
          })
          .catch(() => {
            /* ignore probe failures */
          })
      }

      // 方案 C：网易/QQ 直连（play-url / CDN）失败 → 同一首切全量代理再试一次
      if (song.source === 'netease' || song.source === 'qq') {
        const fallback =
          song.source === 'netease'
            ? getNeteaseProxyFallbackUrl(song)
            : getQQProxyFallbackUrl(song)
        const tried = neteaseProxyFallbackTriedRef.current
        if (fallback && !tried.has(song.id)) {
          tried.add(song.id)
          console.warn(
            `[MusicPlayer] ${song.source} 直连失败，降级全量代理: ${song.name} (${song.id})`,
          )

          const updated: Song = { ...song, url: fallback }
          currentSongRef.current = updated
          setCurrentSong(updated)
          setPlaylist((prev) =>
            prev.map((s) =>
              s.id === song.id && s.source === song.source ? updated : s,
            ),
          )

          setIsAudioLoading(true)
          audio.pause()
          audio.currentTime = 0
          // 仍属同一 generation；更新 load 标记，代理 error 可继续处理
          audioLoadSongIdRef.current = updated.id
          audio.src = fallback
          audio.load()
          audioManager.setCurrentAudio(audio, updated)

          if (userWantsPlayingRef.current) {
            void audio.play().then(
              () => {
                if (
                  selectGenerationRef.current !==
                    audioLoadGenerationRef.current ||
                  currentSongRef.current?.id !== updated.id
                ) {
                  return
                }
                setIsPlaying(true)
                audioManager.setPlaybackState('playing')
              },
              () => {
                // 代理可加载但 play 被策略拒绝时，由后续 error/用户手势处理
                if (
                  selectGenerationRef.current !==
                    audioLoadGenerationRef.current ||
                  currentSongRef.current?.id !== updated.id
                ) {
                  return
                }
                setIsPlaying(false)
                audioManager.setPlaybackState('paused')
              },
            )
          }
          return
        }
      }

      setIsPlaying(false)
      setIsAudioLoading(false)
      userWantsPlayingRef.current = false
      patchPlaybackFlags({
        isAudioLoading: false,
        lastPlaybackError: 'playback_failed',
        generation: selectGenerationRef.current,
      })
      // 单曲临时播放（资料库 VIP 等）失败时给用户可见反馈，避免「假在播」
      setMusicErrorKey(song.isVip ? 'vipPlayFailed' : 'playFailed')
      setTimeout(setMusicErrorKey, 4000, '')

      if (playlist.length > 1 && playMode !== 'single') {
        if (errorAdvanceTimer !== null) clearTimeout(errorAdvanceTimer)
        errorAdvanceTimer = setTimeout(() => {
          // 定时器触发时若已切歌，不要替用户跳下一首
          if (
            selectGenerationRef.current !== audioLoadGenerationRef.current ||
            currentSongRef.current?.id !== audioLoadSongIdRef.current
          ) {
            return
          }
          const nextIndex = (currentSongIndex + 1) % playlist.length
          if (playlist[nextIndex]) {
            selectSong(playlist[nextIndex], nextIndex, true)
          }
        }, 1000)
      }
    }

    const handleEnded = async () => {
      if (seekingRef.current) return
      if (audio !== audioRef.current) return

      // 临时播放模式处理
      if (tempPlayModeRef.current.enabled) {
        const {
          originalPlaylist,
          originalIndex,
          originalSource,
          originalPlaylistId,
        } = tempPlayModeRef.current

        tempPlayModeRef.current.enabled = false
        setIsTempPlayMode(false)

        if (audioRef.current) {
          audioRef.current.pause()
          audioRef.current.currentTime = 0
        }

        playlistRef.current = originalPlaylist
        setPlaylist(originalPlaylist)
        setMusicSource(originalSource)
        setPlaylistId(originalPlaylistId)

        if (originalPlaylist.length > 0 && originalPlaylist[originalIndex]) {
          await selectSong(
            originalPlaylist[originalIndex],
            originalIndex,
            false,
          )
        }

        return
      }

      if (playlist.length > 0) {
        let newIndex: number
        let attempts = 0

        if (playMode === 'single') {
          newIndex = currentSongIndex
        } else if (playMode === 'shuffle') {
          if (nextShuffleIndexRef.current !== -1) {
            newIndex = nextShuffleIndexRef.current
          } else {
            newIndex = generateNextShuffleIndex(currentSongIndex)
            if (newIndex === -1) {
              newIndex = 0
            }
          }
        } else {
          newIndex = (currentSongIndex + 1) % playlist.length

          if (excludeVipSongs) {
            while (playlist[newIndex]?.isVip && attempts < playlist.length) {
              newIndex = (newIndex + 1) % playlist.length
              attempts++
            }
          }
        }

        if (
          attempts >= playlist.length &&
          excludeVipSongs &&
          playlist[newIndex]?.isVip
        ) {
          console.warn('没有可播放的歌曲')
          setIsPlaying(false)
          return
        }

        const nextSong = playlist[newIndex]

        // 如果下一首已预加载：复用缓冲音频，主题/取色走与 selectSong 相同的非阻塞路径
        if (
          preloadedSongIndex === newIndex &&
          preloadAudioRef.current &&
          preloadAudioRef.current.readyState >= 2
        ) {
          const generation = ++selectGenerationRef.current
          const isCurrentSelect = () =>
            selectGenerationRef.current === generation

          setIsAudioLoading(false)
          currentSongLoadedRef.current = false
          currentSongStartTimeRef.current = 0
          preloadTriggeredRef.current = false
          currentSongIndexRef.current = newIndex
          currentSongRef.current = nextSong
          audioLoadSongIdRef.current = nextSong.id
          audioLoadGenerationRef.current = generation

          setCurrentSong(nextSong)
          setCurrentSongIndex(newIndex)
          setCurrentTime(0)
          setAudioDuration(0)
          resetLyrics()

          // 同步主题（缓存命中立刻；否则默认色，后台补真色）
          let immediateColors: MusicColors | null = null
          if (nextSong.cover) {
            immediateColors =
              colorCacheRef.current.get(nextSong.cover) ||
              (getCachedPalette(nextSong.cover) as MusicColors | null)
            if (immediateColors) {
              rememberCoverColors(nextSong.cover, immediateColors)
            }
          }
          // 同上：先推未播放态，等 play() 真正成功再亮 isPlaying
          pushSongTheme(nextSong, newIndex, immediateColors, false, {
            resetProgress: true,
          })
          setIsPlaying(false)
          patchPlaybackFlags({
            isAudioLoading: true,
            lastPlaybackError: null,
            generation,
          })

          if (nextSong.cover) {
            try {
              const warm = new Image()
              warm.decoding = 'async'
              warm.fetchPriority = 'high'
              warm.referrerPolicy = 'no-referrer'
              warm.src = nextSong.cover
            } catch {
              /* ignore */
            }
          }

          if (audioRef.current) {
            audioRef.current.pause()
            audioRef.current.currentTime = 0
            audioRef.current.src = preloadAudioRef.current.src
            audioRef.current.volume = volume
            audioRef.current.load()
            try {
              userWantsPlayingRef.current = true
              await audioRef.current.play()
              if (!isCurrentSelect()) return
              if (audioRef.current.paused) {
                setIsPlaying(false)
                return
              }
              setIsPlaying(true)
              audioManager.setCurrentAudio(audioRef.current, nextSong)
            } catch {
              if (!isCurrentSelect()) return
              // 保留意图，等 canplay 补播
              setIsPlaying(false)
            }
          }

          loadLyricsForSong(nextSong)

          if (nextSong.cover && !immediateColors) {
            extractCoverColorsForSong(nextSong, newIndex, generation, 0)
          }
          // 无封面：保留上一首主题色

          if (playMode === 'shuffle' && playlist.length > 1) {
            const nextIndex = generateNextShuffleIndex(newIndex)
            if (nextIndex !== -1 && nextIndex !== newIndex) {
              nextShuffleIndexRef.current = nextIndex
            }
          }
          prefetchAroundIndex(newIndex)
        } else {
          selectSong(playlist[newIndex], newIndex, true)
        }
      } else {
        setIsPlaying(false)
      }
    }

    // 处理暂停事件（用户暂停 或 系统切后台强制 pause）
    const handlePause = () => {
      setIsPlaying(false)
      // 立即同步 globalState，避免进度 tick 读到旧 isPlaying 导致 tapp 状态闪烁
      const g = (window as any).__musicPlayerState
      if (g) g.isPlaying = false

      // 仅前台暂停视为用户意图；后台被系统掐断时保留 userWantsPlaying 以便回前台恢复
      if (!document.hidden) {
        userWantsPlayingRef.current = false
        audioManager.setPlaybackState('paused')
      } else if (userWantsPlayingRef.current) {
        // 后台仍希望播放：媒体会话保持 playing，避免锁屏控件被打成暂停态
        audioManager.setPlaybackState('playing')
      } else {
        audioManager.setPlaybackState('paused')
      }
    }

    // 处理播放事件（UI / 系统媒体控制 / 自动恢复）
    const handlePlay = () => {
      setIsPlaying(true)
      userWantsPlayingRef.current = true
      const g = (window as any).__musicPlayerState
      if (g) g.isPlaying = true
      audioManager.setPlaybackState('playing')
      // 频谱：仅桌面接入 Web Audio。移动端 createMediaElementSource 会劫持输出，
      // 进后台 AudioContext suspend 后无法后台播放（见 shouldPreserveNativeAudioOutput）。
      if (!shouldPreserveNativeAudioOutput()) {
        audioManager.connectAudioToAnalyser(audio)
        void audioManager.resumeAudioContext()
      }
    }

    audio.addEventListener('timeupdate', handleTimeUpdate)
    audio.addEventListener('ended', handleEnded)
    audio.addEventListener('error', handleError)
    audio.addEventListener('canplay', handleCanPlay)
    audio.addEventListener('loadedmetadata', handleLoadedMetadata)
    audio.addEventListener('pause', handlePause)
    audio.addEventListener('play', handlePlay)

    return () => {
      handleTimeUpdate.cancel()
      if (errorAdvanceTimer !== null) clearTimeout(errorAdvanceTimer)
      audio.removeEventListener('timeupdate', handleTimeUpdate)
      audio.removeEventListener('ended', handleEnded)
      audio.removeEventListener('error', handleError)
      audio.removeEventListener('canplay', handleCanPlay)
      audio.removeEventListener('loadedmetadata', handleLoadedMetadata)
      audio.removeEventListener('pause', handlePause)
      audio.removeEventListener('play', handlePlay)
    }
  }, [
    volume,
    playlist,
    currentSongIndex,
    selectSong,
    preloadedSongIndex,
    preloadNextSong,
    loadLyricsForSong,
    playMode,
    generateNextShuffleIndex,
    excludeVipSongs,
    pushSongTheme,
    prefetchAroundIndex,
    rememberCoverColors,
    resetLyrics,
    extractCoverColorsForSong,
  ])

  // 播放列表变化时清除预加载缓存
  useEffect(() => {
    preloadCacheRef.current.clear()
    setPreloadedSongIndex(-1)
  }, [playlist])

  // 移动端后台播放：可见性变化时维持 Media Session，并在回前台时按用户意图恢复
  useEffect(() => {
    const handleVisibilityChange = async () => {
      const audio = audioRef.current
      if (!audio) return

      if (document.hidden) {
        // 进入后台：记下意图，刷新系统媒体会话位置
        // 注意：此时浏览器可能尚未把 audio.paused 置 true
        if (isPlayingRef.current || !audio.paused) {
          userWantsPlayingRef.current = true
        }

        if (userWantsPlayingRef.current) {
          audioManager.setPlaybackState('playing')
          if (audio.duration && Number.isFinite(audio.duration)) {
            audioManager.updatePositionState(
              audio.duration,
              audio.currentTime,
              audio.playbackRate,
            )
          }
          // 部分 Android 会在 hidden 时 pause：若仍有意图，尝试在后台立刻续播
          // （需此前已有用户手势启动的播放会话；失败则静默，等回前台再试）
          if (audio.paused) {
            try {
              await audio.play()
            } catch {
              // 后台 play 可能被拒，回前台时再恢复
            }
          }
        }
      } else {
        // 回到前台：恢复 AudioContext（桌面频谱），按意图 resume
        await audioManager.resumeAudioContext()

        if (
          userWantsPlayingRef.current &&
          audio.paused &&
          currentSongRef.current
        ) {
          try {
            await audio.play()
            setIsPlaying(true)
            audioManager.setPlaybackState('playing')
          } catch (error) {
            console.warn(
              'Failed to resume playback after visibility change:',
              error,
            )
            setIsPlaying(false)
            audioManager.setPlaybackState('paused')
          }
        } else {
          const actuallyPlaying = !audio.paused
          setIsPlaying(actuallyPlaying)
          if (!actuallyPlaying) {
            userWantsPlayingRef.current = false
          }
          audioManager.setPlaybackState(
            actuallyPlaying ? 'playing' : 'paused',
          )
        }
      }
    }

    document.addEventListener('visibilitychange', handleVisibilityChange)
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, []) // 只在挂载时设置一次

  // 初始化 Media Session API - 使用 ref 存储回调避免频繁重建
  const playPreviousRef = useRef(playPrevious)
  const playNextRef = useRef(playNext)
  playPreviousRef.current = playPrevious
  playNextRef.current = playNext

  useEffect(() => {
    audioManager.setMediaSessionHandlers({
      play: async () => {
        if (audioRef.current) {
          userWantsPlayingRef.current = true
          try {
            await audioRef.current.play()
            // 状态由 handlePlay 事件同步
          } catch {
            userWantsPlayingRef.current = false
          }
        }
      },
      pause: () => {
        // 锁屏/控制中心明确暂停 → 清除用户意图（勿被回前台逻辑误恢复）
        userWantsPlayingRef.current = false
        if (audioRef.current) {
          audioRef.current.pause()
        }
      },
      previoustrack: () => playPreviousRef.current(),
      nexttrack: () => playNextRef.current(),
      seekbackward: () => {
        if (audioRef.current) {
          audioRef.current.currentTime = Math.max(
            0,
            audioRef.current.currentTime - 10,
          )
        }
      },
      seekforward: () => {
        if (audioRef.current) {
          audioRef.current.currentTime = Math.min(
            audioRef.current.duration || 0,
            audioRef.current.currentTime + 10,
          )
        }
      },
      seekto: (details) => {
        if (audioRef.current && details.seekTime !== undefined) {
          audioRef.current.currentTime = details.seekTime
          setCurrentTime(details.seekTime)
        }
      },
    })
  }, []) // 只在挂载时初始化一次

  // 监听播放歌曲事件 - 使用 ref 避免频繁重建监听器
  const playSongRef = useRef(playSong)
  playSongRef.current = playSong

  useEffect(() => {
    const handlePlaySong = (e: Event) => {
      const customEvent = e as CustomEvent
      const song = customEvent.detail?.song
      if (song) {
        playSongRef.current(song)
      }
    }

    window.addEventListener('play-song', handlePlaySong)
    return () => {
      window.removeEventListener('play-song', handlePlaySong)
    }
  }, []) // 只在挂载时设置一次

  // 元数据补丁（embed 后台补歌名/封面）：同 id 合并进 React 态，不重载音频
  useEffect(() => {
    const handlePatchCurrentSong = (e: Event) => {
      const patch = (e as CustomEvent).detail?.song as Song | undefined
      if (!patch?.id) return
      const cur = currentSongRef.current
      if (!cur || String(cur.id) !== String(patch.id)) return
      const merged: Song = {
        ...cur,
        ...patch,
        // 保留正在播的 url，避免触发 audio 重载
        url: cur.url || patch.url,
      }
      currentSongRef.current = merged
      setCurrentSong(merged)
      // 临时单曲列表里同步显示名/封面
      if (tempPlayModeRef.current.enabled && playlistRef.current.length === 1) {
        const nextList = [merged]
        playlistRef.current = nextList
        setPlaylist(nextList)
      } else {
        setPlaylist((prev) => {
          const next = prev.map((s) =>
            String(s.id) === String(merged.id)
              ? { ...s, ...merged, url: s.url || merged.url }
              : s,
          )
          playlistRef.current = next
          return next
        })
      }
    }

    window.addEventListener(
      'music-player-patch-current-song',
      handlePatchCurrentSong,
    )
    return () => {
      window.removeEventListener(
        'music-player-patch-current-song',
        handlePatchCurrentSong,
      )
    }
  }, [])

  // 在当前播放列表中按索引播放（不进入临时单曲模式）
  // play-song-at-index / jump-to-index 语义一致：保留歌单，仅 selectSong
  // ⚠️ 旧实现误走 playSong → 把整表换成 [单曲] 临时播放，Tapp playTrack(trackIndex) 会毁列表
  const selectSongRef = useRef(selectSong)
  selectSongRef.current = selectSong

  useEffect(() => {
    const handlePlayInPlaylist = (e: Event) => {
      const customEvent = e as CustomEvent
      const { index, song } = customEvent.detail || {}
      if (
        typeof index === 'number' &&
        index >= 0 &&
        index < playlistRef.current.length
      ) {
        const targetSong = song || playlistRef.current[index]
        if (targetSong) {
          selectSongRef.current(targetSong as Song, index, true)
        }
      }
    }

    window.addEventListener('play-song-at-index', handlePlayInPlaylist)
    window.addEventListener('jump-to-index', handlePlayInPlaylist)
    return () => {
      window.removeEventListener('play-song-at-index', handlePlayInPlaylist)
      window.removeEventListener('jump-to-index', handlePlayInPlaylist)
    }
  }, []) // 只在挂载时设置一次

  // 监听切换播放/暂停事件 - 使用 ref 避免频繁重建监听器
  const togglePlayRef = useRef(togglePlay)
  togglePlayRef.current = togglePlay

  useEffect(() => {
    const handleTogglePlayPause = () => {
      togglePlayRef.current()
    }

    window.addEventListener('toggle-play-pause', handleTogglePlayPause)
    return () => {
      window.removeEventListener('toggle-play-pause', handleTogglePlayPause)
    }
  }, []) // 只在挂载时设置一次

  // 监听音乐状态同步请求 - 使用 ref 避免频繁重建监听器
  const broadcastStateChangeRef = useRef(broadcastStateChange)
  broadcastStateChangeRef.current = broadcastStateChange

  useEffect(() => {
    const handleSyncRequest = () => {
      broadcastStateChangeRef.current()
    }

    window.addEventListener('request-music-state-sync', handleSyncRequest)
    return () => {
      window.removeEventListener('request-music-state-sync', handleSyncRequest)
    }
  }, []) // 只在挂载时设置一次

  // 发送音乐播放器状态变化事件
  // currentTime 进度通过 handleTimeUpdate → music-player-progress 事件实时推送
  // 这里只处理关键状态变化（切歌、播放暂停、列表变化等），立即广播
  const prevKeyStateRef = useRef('')

  useEffect(() => {
    // 构建关键状态快照（不含 currentTime，进度由 music-player-progress 实时推送）
    const keyState = `${currentSong?.id}|${musicEnabled}|${isPlaying}|${musicColors?.primary}|${currentSongIndex}|${playlist.length}|${volume}|${playMode}|${lyrics.length}|${currentLyricIndex}|${verbatimLyrics.length}|${verbatimLyricsSource}|${isAudioLoading}|${selectGenerationRef.current}`

    if (prevKeyStateRef.current === keyState) {
      return
    }
    prevKeyStateRef.current = keyState

    // 关键状态变化，立即广播
    broadcastStateChange()
  }, [
    currentSong?.id,
    musicEnabled,
    isPlaying,
    musicColors?.primary,
    currentSongIndex,
    playlist.length,
    volume,
    playMode,
    lyrics.length,
    currentLyricIndex,
    verbatimLyrics.length,
    verbatimLyricsSource,
    isAudioLoading,
    broadcastStateChange,
  ])

  // 监听停止临时播放事件 - 使用 ref 避免频繁重建监听器
  const stopTempPlayRef = useRef(stopTempPlay)
  stopTempPlayRef.current = stopTempPlay

  useEffect(() => {
    const handleStopTempPlay = () => {
      stopTempPlayRef.current()
    }

    window.addEventListener('stop-temp-play', handleStopTempPlay)
    return () => {
      window.removeEventListener('stop-temp-play', handleStopTempPlay)
    }
  }, []) // 只在挂载时设置一次

  // 监听 Tapp 媒体控制事件 - 使用 ref 避免频繁重建监听器
  const handleSeekRef = useRef(handleSeek)
  const handleVolumeChangeRef = useRef(handleVolumeChange)
  const setPlayModeRef = useRef(setPlayMode)
  const loadPlaylistRef = useRef(loadPlaylist)
  handleSeekRef.current = handleSeek
  handleVolumeChangeRef.current = handleVolumeChange
  setPlayModeRef.current = setPlayMode
  loadPlaylistRef.current = loadPlaylist

  useEffect(() => {
    const handleTappNext = () => {
      playNextRef.current()
    }
    const handleTappPrev = () => {
      playPreviousRef.current()
    }
    const handleTappSeek = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && typeof detail.position === 'number') {
        handleSeekRef.current(detail.position)
      }
    }
    const handleTappVolume = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && typeof detail.volume === 'number') {
        // Tapp 发送的是 0-100，需要转换为 0-1
        const normalizedVolume =
          detail.volume <= 1 ? detail.volume : detail.volume / 100
        handleVolumeChangeRef.current(normalizedVolume)
      }
    }
    const handleTappMute = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail) {
        handleVolumeChangeRef.current(detail.muted ? 0 : 0.7)
      }
    }
    const handleTappMode = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && detail.mode) {
        // API 模式: 'sequence' | 'loop' | 'shuffle' | 'single'
        // 内部模式: 'loop' | 'single' | 'shuffle'
        const modeMap: Record<string, 'loop' | 'single' | 'shuffle'> = {
          sequence: 'loop',
          loop: 'loop',
          shuffle: 'shuffle',
          single: 'single',
        }
        const mappedMode = modeMap[detail.mode] || 'loop'
        setPlayModeRef.current(mappedMode)
      }
    }

    // 跳过/禁止播放 VIP 歌曲开关（供 Tapp 控制）
    const handleTappSkipVip = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && typeof detail.value === 'boolean') {
        setExcludeVipSongs(detail.value)
      }
    }

    window.addEventListener('music-player-next', handleTappNext)
    window.addEventListener('music-player-prev', handleTappPrev)
    window.addEventListener('music-player-seek', handleTappSeek)
    window.addEventListener('music-player-volume', handleTappVolume)
    window.addEventListener('music-player-mute', handleTappMute)
    window.addEventListener('music-player-mode', handleTappMode)
    window.addEventListener('music-player-set-skip-vip', handleTappSkipVip)

    return () => {
      window.removeEventListener('music-player-next', handleTappNext)
      window.removeEventListener('music-player-prev', handleTappPrev)
      window.removeEventListener('music-player-seek', handleTappSeek)
      window.removeEventListener('music-player-volume', handleTappVolume)
      window.removeEventListener('music-player-mute', handleTappMute)
      window.removeEventListener('music-player-mode', handleTappMode)
      window.removeEventListener('music-player-set-skip-vip', handleTappSkipVip)
    }
  }, []) // 只在挂载时设置一次

  // 同步「跳过 VIP」开关到全局状态，供 Tapp media API 读取
  useEffect(() => {
    setGlobalState({ excludeVipSongs })
  }, [excludeVipSongs])

  // 封面 <img> onload：显示图已解码时补 DOM 取色（网络二次请求失败时的主路径）
  useEffect(() => {
    const handleCoverLoaded = (e: Event) => {
      const detail = (e as CustomEvent<{ songId?: string; cover?: string }>)
        .detail
      const song = currentSongRef.current
      if (!song?.cover || !detail?.cover) return
      if (song.id !== detail.songId && song.cover !== detail.cover) return
      const hit =
        colorCacheRef.current.get(song.cover) || getCachedPalette(song.cover)
      if (hit && !isDefaultPalette(hit)) return
      extractCoverColorsForSong(
        song,
        currentSongIndexRef.current,
        selectGenerationRef.current,
        0,
      )
    }
    window.addEventListener('music-cover-loaded', handleCoverLoaded)
    return () => {
      window.removeEventListener('music-cover-loaded', handleCoverLoaded)
    }
  }, [extractCoverColorsForSong])

  // 监听 Tapp/Agent 加载歌单事件
  useEffect(() => {
    const handleLoadPlaylist = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && detail.playlistId) {
        const source = (detail.source as MusicSource) || 'netease'
        const autoPlay = detail.autoPlay !== false // 默认自动播放
        loadPlaylistRef.current(source, detail.playlistId, autoPlay)
      }
    }

    window.addEventListener('music-player-load-playlist', handleLoadPlaylist)
    return () => {
      window.removeEventListener(
        'music-player-load-playlist',
        handleLoadPlaylist,
      )
    }
  }, [])

  // 组件卸载时清理
  useEffect(() => {
    return () => {
      timeoutIdsRef.current.forEach(clearTimeout)
      timeoutIdsRef.current = []

      userWantsPlayingRef.current = false
      audioManager.stopCurrentAudio()

      if (audioRef.current) {
        destroyPlaybackAudioElement(audioRef.current)
        audioRef.current = null
      }

      if (preloadAudioRef.current) {
        preloadAudioRef.current.pause()
        preloadAudioRef.current.src = ''
        preloadAudioRef.current = null
      }

      colorCacheRef.current.clear()
    }
  }, [])

  return {
    // 基本状态
    playlist,
    currentSongIndex,
    currentSong,
    isPlaying,
    isAudioLoading,
    currentTime,
    audioDuration,
    volume,
    lyrics,
    verbatimLyrics,
    hasVerbatimLyrics: verbatimLyrics.length > 0,
    verbatimLyricsSource,
    currentLyricIndex,
    musicEnabled,
    musicSource,
    playlistId,
    musicErrorKey,
    musicPlayerView,
    playMode,
    musicColors,

    // 搜索和过滤
    playlistSearchQuery,
    excludeVipSongs,
    filteredPlaylist,

    // 临时播放模式（state，保证关闭钮即时切换）
    isTempPlayMode,

    // 控制方法
    togglePlay,
    playPrevious,
    playNext,
    handleSeek,
    handleSeekStart,
    handleSeekEnd,
    handleVolumeChange,
    togglePlayMode,
    selectSong,
    playSong,
    stopTempPlay,
    setMusicPlayerView,
    setPlaylistSearchQuery,
    setExcludeVipSongs,
    loadMusicConfig,

    // Refs
    audioRef,
    playlistScrollRef,
    progressBarRef,
    musicContainerRef,
    volumeControlRef,

    // 音量弹出控制
    showVolumePopup,
    setShowVolumePopup,

    // 播放模式相关
    getPlayModeInfo,

    // 进度条 UI 可见性
    setProgressUiVisible,
  }
}
