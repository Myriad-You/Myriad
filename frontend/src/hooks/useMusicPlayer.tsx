/**
 * 音乐播放器状态管理 Hook
 * 从 GlobalControlPanel 分离出来的音乐播放器核心逻辑
 */

import type {
  LyricLine,
  MusicSource,
  Song,
} from '../utils/musicPlayer'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { API_URL } from '../config'
import { extractColorsFromImage } from '../utils/colorExtractor'
import {
  audioManager,
  filterPlaylist,
  getCurrentLyricIndex,
  getNeteaseLyrics,
  getNeteasePlaylist,
  getQQLyrics,
  getQQPlaylist,
  throttle,
} from '../utils/musicPlayer'
import { loadResource } from '../utils/resourceLoader'
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
  lyricsScrollRef: React.RefObject<HTMLDivElement>
  playlistScrollRef: React.RefObject<HTMLDivElement>
  progressBarRef: React.RefObject<HTMLInputElement>
  musicContainerRef: React.RefObject<HTMLDivElement>
  volumeControlRef: React.RefObject<HTMLDivElement>

  // 音量弹出控制
  showVolumePopup: boolean
  setShowVolumePopup: (show: boolean) => void

  // 播放模式相关
  getPlayModeInfo: () => { icon: React.ReactNode, textKey: 'singleRepeat' | 'shuffle' | 'listRepeat' }
}

// 全局状态恢复（跨页面切换）- SSR 安全
const isBrowser = typeof window !== 'undefined'

function getGlobalState() {
  if (!isBrowser)
    return null
  return (window as any).__musicPlayerState
}

function setGlobalState(state: any) {
  if (!isBrowser)
    return;
  (window as any).__musicPlayerState = state
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
  const [currentLyricIndex, setCurrentLyricIndex] = useState(-1)
  const [musicEnabled, setMusicEnabled] = useState(false)
  const [musicSource, setMusicSource] = useState<MusicSource>('netease')
  const [playlistId, setPlaylistId] = useState('')
  const [musicErrorKey, setMusicErrorKey] = useState<string>('')
  const [musicPlayerView, setMusicPlayerView] = useState<MusicPlayerView>('info')
  const [playMode, setPlayMode] = useState<PlayMode>('loop')
  const [musicColors, setMusicColors] = useState<MusicColors | null>(null)

  // 在客户端从全局状态恢复
  const initializedRef = useRef(false)
  useEffect(() => {
    if (initializedRef.current)
      return
    initializedRef.current = true

    const globalState = getGlobalState()
    if (globalState) {
      if (globalState.playlist)
        setPlaylist(globalState.playlist)
      if (typeof globalState.currentSongIndex === 'number')
        setCurrentSongIndex(globalState.currentSongIndex)
      if (globalState.currentSong)
        setCurrentSong(globalState.currentSong)
      if (typeof globalState.isEnabled === 'boolean')
        setMusicEnabled(globalState.isEnabled)
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

  // 歌词相关 Refs（避免频繁触发 effect）
  const lyricsRef = useRef<LyricLine[]>([])
  const currentLyricIndexRef = useRef<number>(-1)

  // 封面颜色缓存
  const colorCacheRef = useRef<Map<string, MusicColors>>(new Map())

  // Timeout 追踪
  const timeoutIdsRef = useRef<number[]>([])

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

  // 过滤后的播放列表
  const filteredPlaylist = useMemo(() => {
    let filtered = filterPlaylist(playlist, playlistSearchQuery)
    if (excludeVipSongs) {
      filtered = filtered.filter(song => !song.isVip)
    }
    return filtered
  }, [playlist, playlistSearchQuery, excludeVipSongs])

  // 同步 lyrics 和 currentLyricIndex 到 ref
  useEffect(() => {
    lyricsRef.current = lyrics
  }, [lyrics])

  useEffect(() => {
    currentLyricIndexRef.current = currentLyricIndex
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
      root.style.setProperty('--music-primary', normalizeColor(musicColors.primary))
      root.style.setProperty('--music-secondary', normalizeColor(musicColors.secondary))
      root.style.setProperty('--music-accent', normalizeColor(musicColors.accent))
      root.style.setProperty('--music-light', normalizeColor(musicColors.light))
      root.style.setProperty('--music-dark', normalizeColor(musicColors.dark))
    }
    else {
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
    if (!progressBarRef.current)
      return

    // ⚠️ 使用统一的性能检测
    const perf = getPerformanceProfileSync()

    // 移动端或低端设备直接返回,不启动动画
    if (perf.isMobile || perf.lowEndDevice) {
      return
    }

    if (breathAnimationRef.current !== null) {
      cancelAnimationFrame(breathAnimationRef.current)
    }

    const progressBar = progressBarRef.current
    const startTime = Date.now()
    const duration = 1500

    // 预先获取主题色并缓存，避免在动画循环中频繁调用 getComputedStyle 导致强制重排
    let cachedPrimaryColor = getComputedStyle(document.documentElement)
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
        cachedPrimaryColor = getComputedStyle(document.documentElement)
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
      progressBar.style.setProperty('--thumb-shadow', `0 ${2 + 2 * (scale - 1) / 0.3}px ${6 + 6 * (scale - 1) / 0.3}px ${hexToRgba(cachedPrimaryColor, shadowIntensity)}, 0 0 ${20 * (scale - 1) / 0.3}px ${hexToRgba(cachedPrimaryColor, shadowIntensity * 0.6)}`,
      )

      breathAnimationRef.current = requestAnimationFrame(animate)
    }

    breathAnimationRef.current = requestAnimationFrame(animate)
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
    }
    else {
      stopProgressBreathAnimation()
    }

    return () => {
      stopProgressBreathAnimation()
    }
  }, [isAudioLoading, startProgressBreathAnimation, stopProgressBreathAnimation])

  // 为随机模式生成下一首歌曲索引
  const generateNextShuffleIndex = useCallback((currentIndex: number) => {
    if (playlist.length <= 1)
      return -1

    const availableSongs = excludeVipSongs
      ? playlist.map((song, idx) => ({ song, idx })).filter(item => !item.song.isVip)
      : playlist.map((song, idx) => ({ song, idx }))

    if (availableSongs.length === 0)
      return -1

    const availableOptions = availableSongs.filter(item => item.idx !== currentIndex)
    if (availableOptions.length === 0)
      return availableSongs[0].idx

    const randomItem = availableOptions[Math.floor(Math.random() * availableOptions.length)]
    return randomItem.idx
  }, [playlist, excludeVipSongs])

  // 预加载下一首歌曲
  const preloadNextSong = useCallback((nextIndex: number, force: boolean = false) => {
    if (preloadDisabledUntilRef.current > Date.now()) {
      return
    }

    if (!preloadAudioRef.current || nextIndex < 0 || nextIndex >= playlist.length) {
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
    if (!nextSong)
      return

    if (excludeVipSongs && nextSong.isVip) {
      return
    }

    preloadTriggeredRef.current = true

    loadResource.low(`music-preload-${nextIndex}`, async () => {
      const preloadAudio = preloadAudioRef.current
      if (!preloadAudio)
        return

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
  }, [playlist, excludeVipSongs])

  // 广播状态变化事件 - 使用 ref 避免重复广播
  const lastBroadcastRef = useRef<string>('')
  const broadcastStateChange = useCallback(() => {
    // 创建状态快照用于比较（currentTime 按秒取整，避免过于频繁的更新）
    const stateSnapshot = JSON.stringify({
      songId: currentSong?.id,
      isEnabled: musicEnabled,
      isPlaying,
      color: musicColors?.primary,
      isTempPlay: tempPlayModeRef.current.enabled,
      index: currentSongIndex,
      length: playlist.length,
      time: Math.floor(currentTime), // 按秒取整
      volume: Math.round(volume * 100),
      mode: playMode,
    })

    // 如果状态没有变化，跳过广播
    if (lastBroadcastRef.current === stateSnapshot) {
      return
    }
    lastBroadcastRef.current = stateSnapshot

    // 🎯 同步更新全局状态（供 Tapp API 读取）
    const globalState = (window as { __musicPlayerState?: Record<string, unknown> }).__musicPlayerState
    if (globalState) {
      globalState.musicColor = musicColors?.primary || '#ef4444'
      globalState.musicColors = musicColors // 存储完整的颜色对象
      globalState.isPlaying = isPlaying
      globalState.volume = volume
      globalState.playMode = playMode
      globalState.lyrics = lyrics
      globalState.currentLyricIndex = currentLyricIndex
    }

    window.dispatchEvent(new CustomEvent('music-player-state-change', {
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
        currentTime,
        audioDuration,
        volume,
        playMode,
        // 🎯 添加歌词信息
        lyrics,
        currentLyricIndex,
      },
    }))
  }, [currentSong, musicEnabled, isPlaying, musicColors, currentSongIndex, playlist, currentTime, audioDuration, volume, playMode, lyrics, currentLyricIndex])

  // 选择歌曲
  const selectSong = useCallback(async (song: Song, index: number, autoPlay: boolean = false) => {
    if (excludeVipSongs && song.isVip) {
      return
    }

    // 重置预加载状态
    currentSongLoadedRef.current = false
    currentSongStartTimeRef.current = 0
    preloadTriggeredRef.current = false

    setCurrentSong(song)
    setCurrentSongIndex(index)
    setAudioDuration(0)

    // 立即触发状态更新
    window.dispatchEvent(new CustomEvent('music-player-state-change', {
      detail: {
        currentSong: song,
        isEnabled: musicEnabled,
        isPlaying: false,
        musicColor: musicColors?.primary || '#ef4444',
        isTempPlay: tempPlayModeRef.current.enabled,
        currentSongIndex: index,
        playlistLength: playlist.length,
        playlist,
      },
    }))

    // 提取封面颜色
    if (song.cover) {
      try {
        const musicContainer = musicContainerRef.current

        if (colorCacheRef.current.has(song.cover)) {
          const cachedColors = colorCacheRef.current.get(song.cover)!
          setMusicColors(cachedColors)
        }
        else {
          if (musicContainer) {
            musicContainer.classList.add('color-transitioning')
          }

          const colors = await extractColorsFromImage(song.cover, { context: 'music' })

          if (colorCacheRef.current.size >= 50) {
            const firstKey = colorCacheRef.current.keys().next().value
            if (firstKey !== undefined) {
              colorCacheRef.current.delete(firstKey)
            }
          }
          colorCacheRef.current.set(song.cover, colors)

          const tid1 = window.setTimeout(() => {
            setMusicColors(colors)
            if (musicContainer) {
              const tid2 = window.setTimeout(() => {
                musicContainer.classList.remove('color-transitioning')
              }, 50)
              timeoutIdsRef.current.push(tid2)
            }
          }, 300)
          timeoutIdsRef.current.push(tid1)
        }
      }
      catch (error) {
        console.warn('Failed to extract colors from cover:', error)
        setMusicColors(null)
        const musicContainer = musicContainerRef.current
        if (musicContainer) {
          musicContainer.classList.remove('color-transitioning')
        }
      }
    }
    else {
      setMusicColors(null)
    }

    // 加载歌词（低优先级）
    setLyrics([])
    setCurrentLyricIndex(-1)

    loadResource.low(`lyrics-${song.id}`, async () => {
      try {
        const fetchedLyrics = song.source === 'netease'
          ? await getNeteaseLyrics(song.id)
          : await getQQLyrics(song.id)

        if (fetchedLyrics && fetchedLyrics.length > 0) {
          setLyrics(fetchedLyrics)
          setCurrentLyricIndex(-1)
        }
        else {
          setLyrics([])
        }
      }
      catch (error) {
        setLyrics([])
        setCurrentLyricIndex(-1)
      }
    })

    // 加载歌曲
    if (audioRef.current) {
      setIsAudioLoading(true)

      audioRef.current.pause()
      audioRef.current.currentTime = 0
      audioRef.current.src = song.url
      audioRef.current.load()

      audioManager.setCurrentAudio(audioRef.current, song)

      if (autoPlay) {
        // 🔧 简化：延迟后尝试播放，状态由 audio 事件处理器同步
        setTimeout(async () => {
          try {
            await audioRef.current?.play()
            // 播放成功后才设置状态（handlePlay 事件也会设置，这里确保一致）
            setIsPlaying(true)
            audioManager.setPlaybackState('playing')
          }
          catch (error) {
            // 播放失败
            setIsPlaying(false)
            audioManager.setPlaybackState('paused')
          }
        }, 100)
      }
      else {
        setIsPlaying(false)
        audioManager.setPlaybackState('paused')
      }
      setCurrentTime(0)
    }

    // 随机模式需要提前确定下一首
    if (playMode === 'shuffle' && playlist.length > 1) {
      const nextIndex = generateNextShuffleIndex(index)
      if (nextIndex !== -1 && nextIndex !== index) {
        nextShuffleIndexRef.current = nextIndex
      }
    }

    // 更新全局状态
    setGlobalState({
      playlist,
      currentSongIndex: index,
      currentSong: song,
      isEnabled: musicEnabled,
      musicColor: musicColors?.primary || '#ef4444',
    })

    // 触发状态更新事件
    window.dispatchEvent(new CustomEvent('music-player-state-change', {
      detail: {
        currentSong: song,
        isEnabled: musicEnabled,
        isPlaying: autoPlay,
        musicColor: musicColors?.primary || '#ef4444',
        isTempPlay: tempPlayModeRef.current.enabled,
        currentSongIndex: index,
        playlistLength: playlist.length,
        playlist,
      },
    }))
  }, [musicEnabled, musicColors, playlist, playMode, excludeVipSongs, generateNextShuffleIndex])

  // 播放单首歌曲（临时播放模式）
  const playSong = useCallback((song: Song) => {
    // 确保音频元素已初始化
    if (!audioRef.current) {
      audioRef.current = new Audio()
      audioRef.current.volume = volume
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
  }, [musicEnabled, volume, playlist, currentSongIndex, musicSource, playlistId, selectSong])

  // 停止临时播放
  const stopTempPlay = useCallback(async () => {
    if (!tempPlayModeRef.current.enabled)
      return

    const { originalPlaylist, originalIndex, originalSource, originalPlaylistId } = tempPlayModeRef.current

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
    }
    else {
      setCurrentSong(null)
    }
  }, [selectSong])

  // 加载歌单
  const loadPlaylist = useCallback(async (source: MusicSource, plistId: string) => {
    loadResource.medium(`music-playlist-${plistId}`, async () => {
      try {
        setMusicErrorKey('')
        const songs = source === 'netease'
          ? await getNeteasePlaylist(plistId)
          : await getQQPlaylist(plistId)

        setPlaylist(songs)

        if (songs.length > 0) {
          let firstSongIndex = 0
          if (excludeVipSongs) {
            const nonVipIndex = songs.findIndex(song => !song.isVip)
            if (nonVipIndex !== -1) {
              firstSongIndex = nonVipIndex
            }
          }
          selectSong(songs[firstSongIndex], firstSongIndex)
        }
      }
      catch (error) {
        console.error('Failed to load music playlist:', error)
        setMusicErrorKey('loadPlaylistFailed')
        setPlaylist([])

        setTimeout(() => {
          setMusicErrorKey('')
        }, 3000)
      }
    })
  }, [selectSong, excludeVipSongs])

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
    }
    catch (error) {
      // 静默处理
    }
  }, [loadPlaylist, broadcastStateChange])

  // 播放/暂停
  const togglePlay = useCallback(async () => {
    if (!audioRef.current || !currentSong)
      return

    if (isPlaying) {
      audioRef.current.pause()
      setIsPlaying(false)
      audioManager.setPlaybackState('paused')
    }
    else {
      const maxRetries = 3
      let retries = 0

      while (retries < maxRetries) {
        try {
          await audioRef.current.play()
          setIsPlaying(true)
          audioManager.setPlaybackState('playing')
          break
        }
        catch (error) {
          retries++
          console.warn(`播放失败，重试 ${retries}/${maxRetries}:`, error)

          if (retries >= maxRetries) {
            console.error('播放失败，已达到最大重试次数:', error)
            setMusicErrorKey('playFailed')
            setTimeout(() => setMusicErrorKey(''), 3000)
            setIsPlaying(false)
          }
          else {
            await new Promise(resolve => setTimeout(resolve, 1000 * retries))
          }
        }
      }
    }

    broadcastStateChange()
  }, [isPlaying, currentSong, broadcastStateChange])

  // 上一首
  const playPrevious = useCallback(() => {
    if (playlist.length === 0)
      return

    let newIndex: number

    if (playMode === 'shuffle') {
      newIndex = generateNextShuffleIndex(currentSongIndex)
    }
    else {
      newIndex = currentSongIndex === 0 ? playlist.length - 1 : currentSongIndex - 1
      let attempts = 0

      while (excludeVipSongs && playlist[newIndex]?.isVip && attempts < playlist.length) {
        newIndex = newIndex === 0 ? playlist.length - 1 : newIndex - 1
        attempts++
      }

      if (attempts >= playlist.length) {
        console.warn('所有歌曲都是VIP，无法播放')
        return
      }
    }

    selectSong(playlist[newIndex], newIndex, true)
  }, [playlist, currentSongIndex, selectSong, excludeVipSongs, playMode, generateNextShuffleIndex])

  // 下一首
  const playNext = useCallback(() => {
    if (playlist.length === 0)
      return

    let newIndex: number

    if (playMode === 'shuffle') {
      newIndex = nextShuffleIndexRef.current !== -1
        ? nextShuffleIndexRef.current
        : generateNextShuffleIndex(currentSongIndex)
    }
    else {
      newIndex = (currentSongIndex + 1) % playlist.length
      let attempts = 0

      while (excludeVipSongs && playlist[newIndex]?.isVip && attempts < playlist.length) {
        newIndex = (newIndex + 1) % playlist.length
        attempts++
      }

      if (attempts >= playlist.length) {
        console.warn('所有歌曲都是VIP，无法播放')
        return
      }
    }

    selectSong(playlist[newIndex], newIndex, true)
  }, [playlist, currentSongIndex, selectSong, excludeVipSongs, playMode, generateNextShuffleIndex])

  // 调整音量
  const handleVolumeChange = useCallback((newVolume: number) => {
    const clampedVolume = Math.max(0, Math.min(1, newVolume))
    setVolume(clampedVolume)

    if (audioRef.current) {
      try {
        audioRef.current.volume = clampedVolume
      }
      catch (error) {
        console.warn('Failed to set audio volume:', error)
      }
    }

    if (preloadAudioRef.current) {
      try {
        preloadAudioRef.current.volume = clampedVolume
      }
      catch (error) {
        // 静默处理
      }
    }
  }, [])

  // 调整播放进度
  const handleSeek = useCallback((time: number) => {
    if (audioRef.current && currentSong) {
      const maxSeekTime = currentSong.duration > 1 ? currentSong.duration - 1 : currentSong.duration * 0.95
      const safeTime = Math.min(time, maxSeekTime)

      audioRef.current.currentTime = safeTime
      setCurrentTime(safeTime)
    }
  }, [currentSong])

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
      if (prev === 'loop')
        return 'single'
      if (prev === 'single')
        return 'shuffle'
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
      audioRef.current = new Audio()
      audioRef.current.volume = volume
      audioManager.setCurrentAudio(audioRef.current, currentSong)
    }

    if (!preloadAudioRef.current) {
      preloadAudioRef.current = new Audio()
      preloadAudioRef.current.preload = 'auto'
      preloadAudioRef.current.volume = volume
    }

    const audio = audioRef.current

    const handleTimeUpdate = throttle(() => {
      const currentTime = audio.currentTime
      setCurrentTime(currentTime)

      // 更新 Media Session 位置状态（移动端后台播放关键）
      if (audio.duration && isFinite(audio.duration)) {
        audioManager.updatePositionState(audio.duration, currentTime, audio.playbackRate)
      }

      // 🎯 广播进度更新给 Tapp（使用较低频率以避免性能问题）
      // 更新全局状态中的 currentTime 和 audioDuration
      const globalState = (window as { __musicPlayerState?: Record<string, unknown> }).__musicPlayerState
      if (globalState) {
        globalState.currentTime = currentTime
        globalState.audioDuration = audio.duration || 0
      }

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
                while (playlist[nextIndex]?.isVip && attempts < playlist.length) {
                  nextIndex = (nextIndex + 1) % playlist.length
                  attempts++
                }
              }
              if (nextIndex !== currentSongIndex && !playlist[nextIndex]?.isVip) {
                preloadNextSong(nextIndex)
              }
            }
            else if (playMode === 'shuffle') {
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
      if (audio.duration && isFinite(audio.duration)) {
        setAudioDuration(audio.duration)
      }
    }

    const handleError = () => {
      console.error('音频播放错误:', audio.error)
      setIsPlaying(false)
      setIsAudioLoading(false)

      if (playlist.length > 1 && playMode !== 'single') {
        setTimeout(() => {
          const nextIndex = (currentSongIndex + 1) % playlist.length
          if (playlist[nextIndex]) {
            selectSong(playlist[nextIndex], nextIndex, true)
          }
        }, 1000)
      }
    }

    const handleEnded = async () => {
      if (seekingRef.current)
        return
      if (audio !== audioRef.current)
        return

      // 临时播放模式处理
      if (tempPlayModeRef.current.enabled) {
        const { originalPlaylist, originalIndex, originalSource, originalPlaylistId } = tempPlayModeRef.current

        tempPlayModeRef.current.enabled = false

        if (audioRef.current) {
          audioRef.current.pause()
          audioRef.current.currentTime = 0
        }

        setPlaylist(originalPlaylist)
        setMusicSource(originalSource)
        setPlaylistId(originalPlaylistId)

        if (originalPlaylist.length > 0 && originalPlaylist[originalIndex]) {
          await selectSong(originalPlaylist[originalIndex], originalIndex, false)
        }

        return
      }

      if (playlist.length > 0) {
        let newIndex: number
        let attempts = 0

        if (playMode === 'single') {
          newIndex = currentSongIndex
        }
        else if (playMode === 'shuffle') {
          if (nextShuffleIndexRef.current !== -1) {
            newIndex = nextShuffleIndexRef.current
          }
          else {
            newIndex = generateNextShuffleIndex(currentSongIndex)
            if (newIndex === -1) {
              newIndex = 0
            }
          }
        }
        else {
          newIndex = (currentSongIndex + 1) % playlist.length

          while (excludeVipSongs && playlist[newIndex]?.isVip && attempts < playlist.length) {
            newIndex = (newIndex + 1) % playlist.length
            attempts++
          }
        }

        if (attempts >= playlist.length && excludeVipSongs && playlist[newIndex]?.isVip) {
          console.warn('没有可播放的歌曲')
          setIsPlaying(false)
          return
        }

        const nextSong = playlist[newIndex]

        // 如果下一首已预加载
        if (preloadedSongIndex === newIndex && preloadAudioRef.current && preloadAudioRef.current.readyState >= 2) {
          setIsAudioLoading(false)

          if (audioRef.current) {
            audioRef.current.pause()
            audioRef.current.currentTime = 0
            audioRef.current.src = preloadAudioRef.current.src
            audioRef.current.volume = volume
            audioRef.current.load()
            // 🔧 使用 async/await 确保播放成功后才更新状态
            try {
              await audioRef.current.play()
              setIsPlaying(true)
              audioManager.setCurrentAudio(audioRef.current, nextSong)
            }
            catch {
              setIsPlaying(false)
            }
          }

          setCurrentSong(nextSong)
          setCurrentSongIndex(newIndex)
          setCurrentTime(0)

          // 提取颜色
          if (nextSong.cover) {
            try {
              const musicContainer = musicContainerRef.current
              if (musicContainer) {
                musicContainer.classList.add('color-transitioning')
              }

              const colors = await extractColorsFromImage(nextSong.cover, { context: 'music' })

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
            }
            catch (error) {
              setMusicColors(null)
            }
          }
          else {
            setMusicColors(null)
          }

          // 加载歌词
          setLyrics([])
          setCurrentLyricIndex(-1)
          loadResource.low(`lyrics-${nextSong.id}`, async () => {
            try {
              const fetchedLyrics = nextSong.source === 'netease'
                ? await getNeteaseLyrics(nextSong.id)
                : await getQQLyrics(nextSong.id)

              if (fetchedLyrics && fetchedLyrics.length > 0) {
                setLyrics(fetchedLyrics)
                setCurrentLyricIndex(-1)
              }
            }
            catch (error) {
              setLyrics([])
            }
          })

          // 随机模式确定下一首
          if (playMode === 'shuffle' && playlist.length > 1) {
            const nextIndex = generateNextShuffleIndex(newIndex)
            if (nextIndex !== -1 && nextIndex !== newIndex) {
              nextShuffleIndexRef.current = nextIndex
            }
          }
        }
        else {
          selectSong(playlist[newIndex], newIndex, true)
        }
      }
      else {
        setIsPlaying(false)
      }
    }

    // 处理系统级暂停事件（移动端浏览器切后台时可能触发）
    const handlePause = () => {
      setIsPlaying(false)
      audioManager.setPlaybackState('paused')
    }

    // 处理系统级播放事件（从系统媒体控制恢复播放）
    const handlePlay = () => {
      setIsPlaying(true)
      audioManager.setPlaybackState('playing')
    }

    audio.addEventListener('timeupdate', handleTimeUpdate)
    audio.addEventListener('ended', handleEnded)
    audio.addEventListener('error', handleError)
    audio.addEventListener('canplay', handleCanPlay)
    audio.addEventListener('loadedmetadata', handleLoadedMetadata)
    audio.addEventListener('pause', handlePause)
    audio.addEventListener('play', handlePlay)

    return () => {
      audio.removeEventListener('timeupdate', handleTimeUpdate)
      audio.removeEventListener('ended', handleEnded)
      audio.removeEventListener('error', handleError)
      audio.removeEventListener('canplay', handleCanPlay)
      audio.removeEventListener('loadedmetadata', handleLoadedMetadata)
      audio.removeEventListener('pause', handlePause)
      audio.removeEventListener('play', handlePlay)
    }
  }, [volume, playlist, currentSongIndex, selectSong, preloadedSongIndex, preloadNextSong, playMode, generateNextShuffleIndex, excludeVipSongs])

  // 播放列表变化时清除预加载缓存
  useEffect(() => {
    preloadCacheRef.current.clear()
    setPreloadedSongIndex(-1)
  }, [playlist])

  // 移动端后台播放恢复 - 页面可见性变化时检查音频状态
  // 🔧 简化逻辑：只处理 AudioContext 恢复，不自动恢复播放
  // 用户通过系统媒体控制暂停后，不应该在页面恢复时自动播放
  useEffect(() => {
    const handleVisibilityChange = async () => {
      const audio = audioRef.current
      if (!audio)
        return

      if (!document.hidden) {
        // 页面恢复到前台：只恢复 AudioContext（用于频谱分析）
        await audioManager.resumeAudioContext()

        // 🔧 同步播放状态到 React 状态（以音频元素实际状态为准）
        // 不主动恢复播放，尊重用户的暂停操作
        const actuallyPlaying = !audio.paused
        setIsPlaying(actuallyPlaying)
        audioManager.setPlaybackState(actuallyPlaying ? 'playing' : 'paused')
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
          try {
            await audioRef.current.play()
            // 状态由 handlePlay 事件同步，这里不需要额外设置
          }
          catch {
            // 播放失败，静默处理
          }
        }
      },
      pause: () => {
        if (audioRef.current) {
          audioRef.current.pause()
          // 状态由 handlePause 事件同步，这里不需要额外设置
        }
      },
      previoustrack: () => playPreviousRef.current(),
      nexttrack: () => playNextRef.current(),
      seekbackward: () => {
        if (audioRef.current) {
          audioRef.current.currentTime = Math.max(0, audioRef.current.currentTime - 10)
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
      if (typeof index === 'number' && index >= 0 && index < playlistRef.current.length) {
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
      if (typeof index === 'number' && index >= 0 && index < playlistRef.current.length) {
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

  // 发送音乐播放器状态变化事件 - 使用节流避免频繁触发
  const broadcastThrottleRef = useRef<number | null>(null)
  // 🎯 使用 ref 跟踪上次广播的秒数，只在秒数变化时触发
  const lastBroadcastSecondRef = useRef<number>(-1)

  useEffect(() => {
    // 对于 currentTime，只在秒数变化时触发（减少广播频率）
    const currentSecond = Math.floor(currentTime)
    const shouldBroadcastTime = isPlaying && currentSecond !== lastBroadcastSecondRef.current

    if (shouldBroadcastTime) {
      lastBroadcastSecondRef.current = currentSecond
    }

    // 使用节流，最多每 500ms 广播一次
    if (broadcastThrottleRef.current) {
      return
    }
    broadcastThrottleRef.current = window.setTimeout(() => {
      broadcastThrottleRef.current = null
      broadcastStateChange()
    }, shouldBroadcastTime ? 500 : 200)

    return () => {
      if (broadcastThrottleRef.current) {
        clearTimeout(broadcastThrottleRef.current)
        broadcastThrottleRef.current = null
      }
    }
  }, [currentSong?.id, musicEnabled, isPlaying, musicColors?.primary, currentSongIndex, playlist.length, currentTime, volume, playMode])

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
        const normalizedVolume = detail.volume <= 1 ? detail.volume : detail.volume / 100
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

    window.addEventListener('music-player-next', handleTappNext)
    window.addEventListener('music-player-prev', handleTappPrev)
    window.addEventListener('music-player-seek', handleTappSeek)
    window.addEventListener('music-player-volume', handleTappVolume)
    window.addEventListener('music-player-mute', handleTappMute)
    window.addEventListener('music-player-mode', handleTappMode)

    return () => {
      window.removeEventListener('music-player-next', handleTappNext)
      window.removeEventListener('music-player-prev', handleTappPrev)
      window.removeEventListener('music-player-seek', handleTappSeek)
      window.removeEventListener('music-player-volume', handleTappVolume)
      window.removeEventListener('music-player-mute', handleTappMute)
      window.removeEventListener('music-player-mode', handleTappMode)
    }
  }, []) // 只在挂载时设置一次

  // 监听 Tapp 加载歌单事件
  useEffect(() => {
    const handleLoadPlaylist = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && detail.playlistId) {
        const source = (detail.source as MusicSource) || 'netease'
        loadPlaylistRef.current(source, detail.playlistId)
      }
    }

    window.addEventListener('music-player-load-playlist', handleLoadPlaylist)
    return () => {
      window.removeEventListener('music-player-load-playlist', handleLoadPlaylist)
    }
  }, [])

  // 组件卸载时清理
  useEffect(() => {
    return () => {
      timeoutIdsRef.current.forEach(clearTimeout)
      timeoutIdsRef.current = []

      audioManager.stopCurrentAudio()

      if (audioRef.current) {
        audioRef.current.pause()
        audioRef.current.src = ''
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
  }
}
