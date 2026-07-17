/**
 * 音乐播放器状态管理 Hook
 * 从 GlobalControlPanel 分离出来的音乐播放器核心逻辑
 */

import type {
  LyricLine,
  MusicSource,
  Song,
  VerbatimLyricsSource,
  WordLyricLine,
} from '../utils/musicPlayer'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { API_URL } from '../config'

import { extractColorsFromImage } from '../utils/colorExtractor'
import {
  audioManager,
  createPlaybackAudioElement,
  destroyPlaybackAudioElement,
  filterPlaylist,
  getCurrentLyricIndex,
  getLyricsWithVerbatim,
  getNeteasePlaylist,
  getQQPlaylist,
  shouldPreserveNativeAudioOutput,
  throttle,
} from '../utils/musicPlayer'
import { loadResource } from '../utils/resourceLoader'
import { getCurrentAnimationConfig } from './useAnimationLevel'
import { getPerformanceProfileSync } from './usePerformanceProfile'

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
  lyricsScrollRef: React.RefObject<HTMLDivElement | null>
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

export function useMusicPlayer(): UseMusicPlayerReturn {
  // 基本状态 - 使用默认值初始化，避免 SSR 问题
  const [playlist, setPlaylist] = useState<Song[]>([])
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
  const lyricsScrollRef = useRef<HTMLDivElement>(null)
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

  // 进度条呼吸动画
  const breathAnimationRef = useRef<number | null>(null)

  // 临时播放模式
  const tempPlayModeRef = useRef<TempPlayMode>({
    enabled: false,
    originalPlaylist: [],
    originalIndex: 0,
    originalSource: 'netease',
    originalPlaylistId: '',
  })

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
  isPlayingRef.current = isPlaying
  currentSongRef.current = currentSong
  currentSongIndexRef.current = currentSongIndex

  // 过滤后的播放列表
  const filteredPlaylist = useMemo(() => {
    let filtered = filterPlaylist(playlist, playlistSearchQuery)
    if (excludeVipSongs) {
      filtered = filtered.filter((song) => !song.isVip)
    }
    return filtered
  }, [playlist, playlistSearchQuery, excludeVipSongs])

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

  // 应用音乐颜色到全局作用域
  useEffect(() => {
    const root = document.documentElement
    if (musicColors) {
      root.style.setProperty(
        '--music-primary',
        normalizeColor(musicColors.primary),
      )
      root.style.setProperty(
        '--music-secondary',
        normalizeColor(musicColors.secondary),
      )
      root.style.setProperty(
        '--music-accent',
        normalizeColor(musicColors.accent),
      )
      root.style.setProperty('--music-light', normalizeColor(musicColors.light))
      root.style.setProperty('--music-dark', normalizeColor(musicColors.dark))
    } else {
      root.style.removeProperty('--music-primary')
      root.style.removeProperty('--music-secondary')
      root.style.removeProperty('--music-accent')
      root.style.removeProperty('--music-light')
      root.style.removeProperty('--music-dark')
    }

    return () => {
      root.style.removeProperty('--music-primary')
      root.style.removeProperty('--music-secondary')
      root.style.removeProperty('--music-accent')
      root.style.removeProperty('--music-light')
      root.style.removeProperty('--music-dark')
    }
  }, [musicColors, normalizeColor])

  // 进度条呼吸动画控制
  // ⚠️ 关键优化: 在移动端/低端设备禁用呼吸动画,减少 RAF 负担
  const startProgressBreathAnimation = useCallback(() => {
    if (!progressBarRef.current) return

    // ⚠️ 使用统一的性能检测（含用户手动性能模式）
    const perf = getPerformanceProfileSync()
    const anim = getCurrentAnimationConfig()

    // 移动端、低端设备或低性能模式直接返回,不启动动画
    if (
      perf.isMobile ||
      perf.lowEndDevice ||
      anim.level === 'light' ||
      anim.level === 'none'
    ) {
      return
    }

    if (breathAnimationRef.current !== null) {
      cancelAnimationFrame(breathAnimationRef.current)
    }

    const progressBar = progressBarRef.current
    const startTime = Date.now()
    const duration = 1500

    // 预先获取主题色并缓存，避免在动画循环中频繁调用 getComputedStyle 导致强制重排
    let cachedPrimaryColor =
      getComputedStyle(document.documentElement)
        .getPropertyValue('--music-primary')
        .trim() || '#ec4899'

    // 每秒更新一次颜色缓存（而不是每帧）
    let lastColorUpdate = Date.now()
    const colorUpdateInterval = 1000

    const animate = () => {
      const now = Date.now()
      const elapsed = now - startTime
      const progress = (elapsed % duration) / duration

      // 仅在间隔后更新颜色，而非每帧
      if (now - lastColorUpdate > colorUpdateInterval) {
        cachedPrimaryColor =
          getComputedStyle(document.documentElement)
            .getPropertyValue('--music-primary')
            .trim() || '#ec4899'
        lastColorUpdate = now
      }

      const scale = 1 + 0.3 * Math.sin(progress * Math.PI * 2)
      const opacity = 0.85 + 0.15 * Math.sin(progress * Math.PI * 2)
      const shadowIntensity = 0.3 + 0.25 * Math.sin(progress * Math.PI * 2)

      const hexToRgba = (hex: string, alpha: number) => {
        const cleanHex = hex.replace('#', '')
        const r = Number.parseInt(cleanHex.substring(0, 2), 16)
        const g = Number.parseInt(cleanHex.substring(2, 4), 16)
        const b = Number.parseInt(cleanHex.substring(4, 6), 16)
        return `rgba(${r}, ${g}, ${b}, ${alpha})`
      }

      progressBar.style.setProperty('--thumb-scale', scale.toString())
      progressBar.style.setProperty('--thumb-opacity', opacity.toString())
      progressBar.style.setProperty(
        '--thumb-shadow',
        `0 ${2 + (2 * (scale - 1)) / 0.3}px ${6 + (6 * (scale - 1)) / 0.3}px ${hexToRgba(cachedPrimaryColor, shadowIntensity)}, 0 0 ${(20 * (scale - 1)) / 0.3}px ${hexToRgba(cachedPrimaryColor, shadowIntensity * 0.6)}`,
      )

      breathAnimationRef.current = requestAnimationFrame(animate)
    }

    breathAnimationRef.current = requestAnimationFrame(animate)
  }, [])

  // 进度条 UI 可见性开关；恢复可见时立即同步一次进度，避免展示过期值
  const setProgressUiVisible = useCallback((visible: boolean) => {
    progressUiVisibleRef.current = visible
    if (visible && audioRef.current) {
      setCurrentTime(audioRef.current.currentTime)
    }
  }, [])

  const stopProgressBreathAnimation = useCallback(() => {
    if (breathAnimationRef.current !== null) {
      cancelAnimationFrame(breathAnimationRef.current)
      breathAnimationRef.current = null
    }

    if (progressBarRef.current) {
      progressBarRef.current.style.removeProperty('--thumb-scale')
      progressBarRef.current.style.removeProperty('--thumb-opacity')
      progressBarRef.current.style.removeProperty('--thumb-shadow')
    }
  }, [])

  // 控制进度条呼吸动画
  useEffect(() => {
    if (isAudioLoading) {
      startProgressBreathAnimation()
    } else {
      stopProgressBreathAnimation()
    }

    return () => {
      stopProgressBreathAnimation()
    }
  }, [
    isAudioLoading,
    startProgressBreathAnimation,
    stopProgressBreathAnimation,
  ])

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
          const handleError = () => {
            preloadErrorCountRef.current += 1

            if (preloadErrorCountRef.current >= 3) {
              preloadDisabledUntilRef.current = Date.now() + 5 * 60 * 1000
              console.warn('音乐预加载已临时禁用5分钟')
            }

            cleanup()
            reject(new Error('Preload failed'))
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

    // 🎯 同步更新全局状态（供 Tapp API 读取）— 写入完整状态，不依赖 Context 转手
    ;(window as any).__musicPlayerState = {
      ...((window as any).__musicPlayerState || {}),
      currentSong,
      isEnabled: musicEnabled,
      isPlaying,
      musicColor: musicColors?.primary || '#ef4444',
      musicColors,
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
    }

    window.dispatchEvent(
      new CustomEvent('music-player-state-change', {
        detail: {
          currentSong,
          isEnabled: musicEnabled,
          isPlaying,
          musicColor: musicColors?.primary || '#ef4444',
          musicColors, // 完整的颜色对象
          isTempPlay: tempPlayModeRef.current.enabled,
          currentSongIndex,
          playlistLength: playlist.length,
          playlist,
          // 🎯 添加实时播放信息（供 Tapp 使用）
          currentTime: liveCurrentTime,
          audioDuration: liveDuration,
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

      loadResource.low(`lyrics-${requestKey}`, async () => {
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
      })
    },
    [resetLyrics],
  )

  // 选择歌曲
  const selectSong = useCallback(
    async (song: Song, index: number, autoPlay: boolean = false) => {
      if (excludeVipSongs && song.isVip) {
        return
      }

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

      const baseDetail = {
        currentSong: song,
        isEnabled: musicEnabled,
        isPlaying: false,
        musicColor: musicColors?.primary || '#ef4444',
        musicColors,
        isTempPlay: tempPlayModeRef.current.enabled,
        currentSongIndex: index,
        playlistLength: playlist.length,
        playlist,
        currentTime: 0,
        audioDuration: 0,
        volume,
        playMode,
        lyrics: [] as LyricLine[],
        verbatimLyrics: [] as WordLyricLine[],
        hasVerbatimLyrics: false,
        verbatimLyricsSource: '' as VerbatimLyricsSource | '',
        currentLyricIndex: -1,
      }

      // 同步写入全局状态，供 Tapp media API 读取
      setGlobalState(baseDetail)

      // 加载歌词（低优先级）：逐字优先，逐行兜底；先重置，避免切歌时残留上一首
      loadLyricsForSong(song)

      // 立即触发状态更新（带空歌词/零进度，防止 bridge 合并出旧曲内容）
      window.dispatchEvent(
        new CustomEvent('music-player-state-change', {
          detail: { ...baseDetail },
        }),
      )

      // 提取封面颜色
      if (song.cover) {
        try {
          const musicContainer = musicContainerRef.current

          if (colorCacheRef.current.has(song.cover)) {
            if (!isCurrentSelect()) return
            const cachedColors = colorCacheRef.current.get(song.cover)!
            setMusicColors(cachedColors)
          } else {
            if (musicContainer) {
              musicContainer.classList.add('color-transitioning')
            }

            const colors = await extractColorsFromImage(song.cover, {
              context: 'music',
            })

            // 快速切歌：丢弃过期取色结果，避免主题色串曲
            if (!isCurrentSelect()) {
              if (musicContainer) {
                musicContainer.classList.remove('color-transitioning')
              }
              return
            }

            if (colorCacheRef.current.size >= 50) {
              const firstKey = colorCacheRef.current.keys().next().value
              if (firstKey !== undefined) {
                colorCacheRef.current.delete(firstKey)
              }
            }
            colorCacheRef.current.set(song.cover, colors)

            const tid1 = window.setTimeout(() => {
              if (!isCurrentSelect()) return
              setMusicColors(colors)
              if (musicContainer) {
                const tid2 = window.setTimeout(() => {
                  if (!isCurrentSelect()) return
                  musicContainer.classList.remove('color-transitioning')
                }, 50)
                timeoutIdsRef.current.push(tid2)
              }
            }, 300)
            timeoutIdsRef.current.push(tid1)
          }
        } catch (error) {
          if (!isCurrentSelect()) return
          console.warn('Failed to extract colors from cover:', error)
          setMusicColors(null)
          const musicContainer = musicContainerRef.current
          if (musicContainer) {
            musicContainer.classList.remove('color-transitioning')
          }
        }
      } else {
        setMusicColors(null)
      }

      if (!isCurrentSelect()) return

      // 加载歌曲
      if (audioRef.current) {
        setIsAudioLoading(true)

        audioRef.current.pause()
        audioRef.current.currentTime = 0
        audioRef.current.src = song.url
        audioRef.current.load()

        audioManager.setCurrentAudio(audioRef.current, song)

        if (autoPlay) {
          userWantsPlayingRef.current = true
          // 延迟后尝试播放，状态由 audio 事件处理器同步
          pendingPlayTimeoutRef.current = setTimeout(async () => {
            pendingPlayTimeoutRef.current = null
            // 期间又切歌了：不要 play 旧曲
            if (!isCurrentSelect()) return
            if (currentSongRef.current?.id !== song.id) return
            try {
              await audioRef.current?.play()
              if (!isCurrentSelect()) return
              // 播放成功后才设置状态（handlePlay 事件也会设置，这里确保一致）
              setIsPlaying(true)
              audioManager.setPlaybackState('playing')
            } catch (_error) {
              if (!isCurrentSelect()) return
              // 播放失败
              userWantsPlayingRef.current = false
              setIsPlaying(false)
              audioManager.setPlaybackState('paused')
            }
          }, 100)
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

      if (!isCurrentSelect()) return

      // 更新全局状态（仍显式带上清空后的进度/歌词，防止被旧字段回填）
      setGlobalState({
        playlist,
        currentSongIndex: index,
        currentSong: song,
        isEnabled: musicEnabled,
        musicColor: musicColors?.primary || '#ef4444',
        musicColors,
        currentTime: 0,
        audioDuration: 0,
        isPlaying: autoPlay,
        volume,
        playMode,
      })

      // 触发状态更新事件
      window.dispatchEvent(
        new CustomEvent('music-player-state-change', {
          detail: {
            currentSong: song,
            isEnabled: musicEnabled,
            isPlaying: autoPlay,
            musicColor: musicColors?.primary || '#ef4444',
            musicColors,
            isTempPlay: tempPlayModeRef.current.enabled,
            currentSongIndex: index,
            playlistLength: playlist.length,
            playlist,
            currentTime: 0,
            audioDuration: 0,
            volume,
            playMode,
          },
        }),
      )
    },
    [
      musicEnabled,
      musicColors,
      playlist,
      playMode,
      volume,
      excludeVipSongs,
      generateNextShuffleIndex,
      loadLyricsForSong,
      resetLyrics,
    ],
  )

  // 播放单首歌曲（临时播放模式）
  const playSong = useCallback(
    (song: Song) => {
      // 确保音频元素已初始化
      if (!audioRef.current) {
        audioRef.current = createPlaybackAudioElement(volume)
        audioManager.setCurrentAudio(audioRef.current, song)
      }

      if (!tempPlayModeRef.current.enabled) {
        tempPlayModeRef.current = {
          enabled: true,
          originalPlaylist: [...playlist],
          originalIndex: currentSongIndex,
          originalSource: musicSource,
          originalPlaylistId: playlistId,
        }
      }

      if (!musicEnabled) {
        setMusicEnabled(true)
        setMusicSource(song.source || 'netease')
      }

      // 先设置播放列表，再延迟调用 selectSong 确保状态已更新
      setPlaylist([song])

      // 使用 setTimeout 确保 React 状态更新已完成
      setTimeout(() => {
        selectSong(song, 0, true)
      }, 0)
    },
    [
      musicEnabled,
      volume,
      playlist,
      currentSongIndex,
      musicSource,
      playlistId,
      selectSong,
    ],
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

    if (audioRef.current) {
      audioRef.current.pause()
      audioRef.current.currentTime = 0
    }
    setIsPlaying(false)

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
          const songs =
            source === 'netease'
              ? await getNeteasePlaylist(plistId)
              : await getQQPlaylist(plistId)

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

      const response = await fetch(`${API_URL}/api/config/ui?t=${Date.now()}`)
      const data = await response.json()

      const enabled = data.music_enabled === 'true'
      const source = data.music_source || 'netease'
      const plistId = data.music_playlist_id || ''

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
    } else {
      userWantsPlayingRef.current = true
      const maxRetries = 3
      let retries = 0

      while (retries < maxRetries) {
        try {
          await audioRef.current.play()
          setIsPlaying(true)
          audioManager.setPlaybackState('playing')
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
  }, [isPlaying, currentSong])

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
  }, [
    playlist,
    selectSong,
    excludeVipSongs,
    playMode,
    generateNextShuffleIndex,
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
  }, [
    playlist,
    selectSong,
    excludeVipSongs,
    playMode,
    generateNextShuffleIndex,
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

  // 调整播放进度
  const handleSeek = useCallback(
    (time: number) => {
      if (audioRef.current && currentSong) {
        const maxSeekTime =
          currentSong.duration > 1
            ? currentSong.duration - 1
            : currentSong.duration * 0.95
        const safeTime = Math.min(time, maxSeekTime)

        audioRef.current.currentTime = safeTime
        setCurrentTime(safeTime)
      }
    },
    [currentSong],
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
      if (!currentSongLoadedRef.current) {
        currentSongLoadedRef.current = true
        currentSongStartTimeRef.current = Date.now()
      }
      setIsAudioLoading(false)
    }

    const handleLoadedMetadata = () => {
      if (audio.duration && Number.isFinite(audio.duration)) {
        setAudioDuration(audio.duration)
      }
    }

    const handleError = () => {
      console.error('音频播放错误:', audio.error)
      setIsPlaying(false)
      setIsAudioLoading(false)

      if (playlist.length > 1 && playMode !== 'single') {
        if (errorAdvanceTimer !== null) clearTimeout(errorAdvanceTimer)
        errorAdvanceTimer = setTimeout(() => {
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

        if (audioRef.current) {
          audioRef.current.pause()
          audioRef.current.currentTime = 0
        }

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

        // 如果下一首已预加载
        if (
          preloadedSongIndex === newIndex &&
          preloadAudioRef.current &&
          preloadAudioRef.current.readyState >= 2
        ) {
          setIsAudioLoading(false)

          if (audioRef.current) {
            audioRef.current.pause()
            audioRef.current.currentTime = 0
            audioRef.current.src = preloadAudioRef.current.src
            audioRef.current.volume = volume
            audioRef.current.load()
            // 使用 async/await 确保播放成功后才更新状态
            try {
              userWantsPlayingRef.current = true
              await audioRef.current.play()
              setIsPlaying(true)
              audioManager.setCurrentAudio(audioRef.current, nextSong)
            } catch {
              userWantsPlayingRef.current = false
              setIsPlaying(false)
            }
          }

          setCurrentSong(nextSong)
          setCurrentSongIndex(newIndex)
          setCurrentTime(0)

          // 加载歌词
          loadLyricsForSong(nextSong)

          // 提取颜色
          if (nextSong.cover) {
            try {
              const musicContainer = musicContainerRef.current
              if (musicContainer) {
                musicContainer.classList.add('color-transitioning')
              }

              const colors = await extractColorsFromImage(nextSong.cover, {
                context: 'music',
              })

              const tid1 = window.setTimeout(() => {
                setMusicColors(colors)
                if (musicContainer) {
                  const tid2 = window.setTimeout(() => {
                    musicContainer.classList.remove('color-transitioning')
                  }, 30)
                  timeoutIdsRef.current.push(tid2)
                }
              }, 150)
              timeoutIdsRef.current.push(tid1)
            } catch (_error) {
              setMusicColors(null)
            }
          } else {
            setMusicColors(null)
          }

          // 随机模式确定下一首
          if (playMode === 'shuffle' && playlist.length > 1) {
            const nextIndex = generateNextShuffleIndex(newIndex)
            if (nextIndex !== -1 && nextIndex !== newIndex) {
              nextShuffleIndexRef.current = nextIndex
            }
          }
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

  // 监听播放指定索引歌曲事件 - 用于 Tapp 调用
  const playlistRef = useRef(playlist)
  playlistRef.current = playlist
  const setCurrentSongIndexRef = useRef(setCurrentSongIndex)
  setCurrentSongIndexRef.current = setCurrentSongIndex

  useEffect(() => {
    const handlePlaySongAtIndex = (e: Event) => {
      const customEvent = e as CustomEvent
      const { index, song } = customEvent.detail || {}
      if (
        typeof index === 'number' &&
        index >= 0 &&
        index < playlistRef.current.length
      ) {
        // 直接设置索引，触发播放
        setCurrentSongIndexRef.current(index)
        const targetSong = song || playlistRef.current[index]
        if (targetSong) {
          playSongRef.current(targetSong)
        }
      }
    }

    window.addEventListener('play-song-at-index', handlePlaySongAtIndex)
    return () => {
      window.removeEventListener('play-song-at-index', handlePlaySongAtIndex)
    }
  }, []) // 只在挂载时设置一次

  // 监听跳转到指定索引事件 - 在当前播放列表中跳转，不触发临时播放
  const selectSongRef = useRef(selectSong)
  selectSongRef.current = selectSong

  useEffect(() => {
    const handleJumpToIndex = (e: Event) => {
      const customEvent = e as CustomEvent
      const { index, song } = customEvent.detail || {}
      if (
        typeof index === 'number' &&
        index >= 0 &&
        index < playlistRef.current.length
      ) {
        const targetSong = song || playlistRef.current[index]
        if (targetSong) {
          // 使用 selectSong 在当前播放列表中选择歌曲，不触发临时播放
          selectSongRef.current(targetSong, index, true)
        }
      }
    }

    window.addEventListener('jump-to-index', handleJumpToIndex)
    return () => {
      window.removeEventListener('jump-to-index', handleJumpToIndex)
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
    const keyState = `${currentSong?.id}|${musicEnabled}|${isPlaying}|${musicColors?.primary}|${currentSongIndex}|${playlist.length}|${volume}|${playMode}|${lyrics.length}|${currentLyricIndex}|${verbatimLyrics.length}|${verbatimLyricsSource}`

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

    // 临时播放模式
    isTempPlayMode: tempPlayModeRef.current.enabled,

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
    lyricsScrollRef,
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
