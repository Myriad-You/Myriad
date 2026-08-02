import type { ReactNode } from 'react'
import type {
  LyricLine,
  Song,
  VerbatimLyricsSource,
  WordLyricLine,
} from '../utils/musicPlayer'
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from 'react'

/**
 * 全局音乐播放器状态管理 - 使用 React Context 实现实时状态同步
 *
 * GlobalControlPanel 通过 Context 暴露状态，其他组件通过 useMusicPlayerControl hook 访问
 * 这样可以确保状态实时同步，无需依赖事件
 */

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

interface MusicPlayerContextType extends MusicPlayerState {
  playSong: (song: Song) => void
  togglePlayPause: () => void
  stopTempPlay: () => void
  updateState: (state: Partial<MusicPlayerState>) => void
}

const MusicPlayerContext = createContext<MusicPlayerContextType | null>(null)

// Provider 组件 - 在 AppLayout 或 App 中使用
export function MusicPlayerProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<MusicPlayerState>({
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
  })

  // 在客户端初始化时从全局状态读取
  useEffect(() => {
    if (typeof window !== 'undefined') {
      const globalState = (window as any).__musicPlayerState
      if (globalState) {
        setState({
          currentSong: globalState.currentSong || null,
          isEnabled: globalState.isEnabled || false,
          isPlaying: globalState.isPlaying || false,
          musicColor: globalState.musicColor || '#ef4444',
          isTempPlay: globalState.isTempPlay || false,
          currentSongIndex: globalState.currentSongIndex || 0,
          playlistLength: globalState.playlistLength || 0,
          playlist: globalState.playlist || [],
          lyrics: globalState.lyrics || [],
          verbatimLyrics: globalState.verbatimLyrics || [],
          hasVerbatimLyrics: globalState.hasVerbatimLyrics || false,
          verbatimLyricsSource: globalState.verbatimLyricsSource || '',
          currentLyricIndex: globalState.currentLyricIndex ?? -1,
        })
      }
    }
  }, [])

  // 监听音乐播放器状态变化事件（向后兼容）
  // ⚠️ 必须按字段合并：detail 可能是 partial（如 embed 只带 currentSong），
  // 整表 setState 会把 lyrics/isPlaying 等冲成默认空值。
  useEffect(() => {
    const handleMusicStateChange = (e: Event) => {
      const detail = (e as CustomEvent).detail as
        | Record<string, unknown>
        | undefined
      if (!detail) return

      setState((prev) => {
        const next = { ...prev }
        if ('currentSong' in detail) {
          next.currentSong = (detail.currentSong as Song | null) ?? null
        }
        if ('isEnabled' in detail) next.isEnabled = Boolean(detail.isEnabled)
        if ('isPlaying' in detail) next.isPlaying = Boolean(detail.isPlaying)
        if ('musicColor' in detail) {
          next.musicColor = String(detail.musicColor || '#ef4444')
        }
        if ('isTempPlay' in detail) {
          next.isTempPlay = Boolean(detail.isTempPlay)
        }
        if ('currentSongIndex' in detail) {
          next.currentSongIndex = Number(detail.currentSongIndex) || 0
        }
        if ('playlistLength' in detail) {
          next.playlistLength = Number(detail.playlistLength) || 0
        }
        if ('playlist' in detail) {
          next.playlist = (detail.playlist as Song[]) || []
        }
        if ('lyrics' in detail) {
          next.lyrics = (detail.lyrics as LyricLine[]) || []
        }
        if ('verbatimLyrics' in detail) {
          next.verbatimLyrics = (detail.verbatimLyrics as WordLyricLine[]) || []
        }
        if ('hasVerbatimLyrics' in detail) {
          next.hasVerbatimLyrics = Boolean(detail.hasVerbatimLyrics)
        }
        if ('verbatimLyricsSource' in detail) {
          next.verbatimLyricsSource = (detail.verbatimLyricsSource ||
            '') as VerbatimLyricsSource
        }
        if ('currentLyricIndex' in detail) {
          next.currentLyricIndex =
            typeof detail.currentLyricIndex === 'number'
              ? detail.currentLyricIndex
              : -1
        }
        return next
      })
    }

    window.addEventListener('music-player-state-change', handleMusicStateChange)
    return () => {
      window.removeEventListener(
        'music-player-state-change',
        handleMusicStateChange,
      )
    }
  }, [])

  // 更新状态的方法
  const updateState = useCallback((newState: Partial<MusicPlayerState>) => {
    setState((prev) => ({ ...prev, ...newState }))
  }, [])

  // 播放歌曲
  const playSong = useCallback((song: Song) => {
    window.dispatchEvent(new CustomEvent('play-song', { detail: { song } }))
  }, [])

  // 切换播放/暂停
  const togglePlayPause = useCallback(() => {
    window.dispatchEvent(new CustomEvent('toggle-play-pause'))
  }, [])

  // 停止临时播放并恢复原播放列表
  const stopTempPlay = useCallback(() => {
    window.dispatchEvent(new CustomEvent('stop-temp-play'))
  }, [])

  const value = useMemo(
    () => ({
      ...state,
      playSong,
      togglePlayPause,
      stopTempPlay,
      updateState,
    }),
    [state, playSong, togglePlayPause, stopTempPlay, updateState],
  )

  return (
    <MusicPlayerContext.Provider value={value}>
      {children}
    </MusicPlayerContext.Provider>
  )
}

// Hook 供其他组件使用
export function useMusicPlayerControl() {
  const context = useContext(MusicPlayerContext)

  if (!context) {
    // 如果没有 Provider，使用降级方案（事件监听）
    console.warn(
      'MusicPlayerProvider not found, using fallback event-based approach',
    )
    return useFallbackMusicPlayerControl()
  }

  return context
}

// ============================================
// 🔧 性能优化：使用 useSyncExternalStore 实现外部状态订阅
// 避免不必要的重渲染，只在实际使用的状态变化时更新组件
// ============================================

/** 全局音乐播放器状态存储 */
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

/** 状态变化监听器集合 */
const musicStateListeners = new Set<() => void>()
let isMusicEventListenerAttached = false

/** 通知所有监听器状态已变化 */
function emitMusicStateChange() {
  musicStateListeners.forEach((listener) => listener())
}

/** 订阅状态变化 */
function subscribeMusicState(listener: () => void) {
  musicStateListeners.add(listener)
  if (typeof window !== 'undefined') {
    const currentState = (window as any).__musicPlayerState
    if (currentState) {
      globalMusicState = { ...globalMusicState, ...currentState }
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

/** 获取当前状态快照 */
function getMusicStateSnapshot() {
  return globalMusicState
}

/**
 * 更新 Context 订阅层状态并通知监听器。
 *
 * 注意：__musicPlayerState 由 useMusicPlayer 独占写入。
 * 这里禁止把事件里的 currentTime:0 / 缺字段碎片回写全局态，
 * 否则颜色补丁或 Context 合并会把进度/歌词冲坏。
 */
function updateGlobalMusicState(newState: Partial<MusicPlayerState>) {
  const next = { ...globalMusicState, ...newState }
  const changed =
    next.currentSong !== globalMusicState.currentSong ||
    next.isEnabled !== globalMusicState.isEnabled ||
    next.isPlaying !== globalMusicState.isPlaying ||
    next.musicColor !== globalMusicState.musicColor ||
    next.isTempPlay !== globalMusicState.isTempPlay ||
    next.currentSongIndex !== globalMusicState.currentSongIndex ||
    next.playlistLength !== globalMusicState.playlistLength ||
    next.playlist !== globalMusicState.playlist ||
    next.lyrics !== globalMusicState.lyrics ||
    next.verbatimLyrics !== globalMusicState.verbatimLyrics ||
    next.hasVerbatimLyrics !== globalMusicState.hasVerbatimLyrics ||
    next.verbatimLyricsSource !== globalMusicState.verbatimLyricsSource ||
    next.currentLyricIndex !== globalMusicState.currentLyricIndex

  if (!changed) return
  globalMusicState = next
  emitMusicStateChange()
}

function handleGlobalMusicStateChange(event: Event) {
  const detail = (event as CustomEvent).detail as
    | Record<string, unknown>
    | undefined
  if (!detail) return
  // 只吸收 Context 关心的字段，忽略 currentTime / musicColors 等宿主专属字段
  const patch: Partial<MusicPlayerState> = {}
  if ('currentSong' in detail)
    patch.currentSong = (detail.currentSong as Song | null) ?? null
  if ('isEnabled' in detail) patch.isEnabled = Boolean(detail.isEnabled)
  if ('isPlaying' in detail) patch.isPlaying = Boolean(detail.isPlaying)
  if ('musicColor' in detail)
    patch.musicColor = String(detail.musicColor || '#ef4444')
  if ('isTempPlay' in detail) patch.isTempPlay = Boolean(detail.isTempPlay)
  if ('currentSongIndex' in detail)
    patch.currentSongIndex = Number(detail.currentSongIndex) || 0
  if ('playlistLength' in detail)
    patch.playlistLength = Number(detail.playlistLength) || 0
  if ('playlist' in detail)
    patch.playlist = (detail.playlist as Song[]) || []
  if ('lyrics' in detail)
    patch.lyrics = (detail.lyrics as LyricLine[]) || []
  if ('verbatimLyrics' in detail)
    patch.verbatimLyrics = (detail.verbatimLyrics as WordLyricLine[]) || []
  if ('hasVerbatimLyrics' in detail)
    patch.hasVerbatimLyrics = Boolean(detail.hasVerbatimLyrics)
  if ('verbatimLyricsSource' in detail) {
    patch.verbatimLyricsSource = (detail.verbatimLyricsSource ||
      '') as VerbatimLyricsSource
}
  if ('currentLyricIndex' in detail) {
    patch.currentLyricIndex =
      typeof detail.currentLyricIndex === 'number'
        ? detail.currentLyricIndex
        : -1
}
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

// 初始化：监听事件并更新全局状态
if (typeof window !== 'undefined') {
  // 暴露 audioManager 到 window（供 Tapp SDK 获取频谱数据）
  import('../utils/musicPlayer').then(({ audioManager }) => {
    ;(window as any).audioManager = audioManager
  })

  // 从 window 对象读取初始状态
  const initialState = (window as any).__musicPlayerState
  if (initialState) {
    globalMusicState = { ...globalMusicState, ...initialState }
  }
}

// 降级方案：基于 useSyncExternalStore 的实现（高性能版本）
function useFallbackMusicPlayerControl() {
  // 🔧 使用 useSyncExternalStore 订阅外部状态
  // 这比 useState + useEffect 更高效，因为它：
  // 1. 避免了初始化时的额外渲染
  // 2. 自动处理并发模式
  // 3. 只在快照变化时触发重渲染
  const state = useSyncExternalStore(
    subscribeMusicState,
    getMusicStateSnapshot,
    getMusicStateSnapshot, // SSR 快照
  )

  const playSong = useCallback((song: Song) => {
    window.dispatchEvent(new CustomEvent('play-song', { detail: { song } }))
  }, [])

  const togglePlayPause = useCallback(() => {
    window.dispatchEvent(new CustomEvent('toggle-play-pause'))
  }, [])

  const stopTempPlay = useCallback(() => {
    window.dispatchEvent(new CustomEvent('stop-temp-play'))
  }, [])

  const updateState = useCallback((newState: Partial<MusicPlayerState>) => {
    updateGlobalMusicState(newState)
  }, [])

  // 🔧 使用 useMemo 避免每次都创建新对象
  return useMemo(
    () => ({
      ...state,
      playSong,
      togglePlayPause,
      stopTempPlay,
      updateState,
    }),
    [state, playSong, togglePlayPause, stopTempPlay, updateState],
  )
}

// ============================================
// 窄订阅：仅 lyrics + currentLyricIndex（资料库卡片歌词等）
// 避免 playlist / isPlaying 等无关字段触发重渲染
// ============================================

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

/**
 * 仅订阅歌词列表与当前句索引。
 * 与 useMusicPlayerControl 不同：不会因 isPlaying / playlist 等变化重渲染。
 */
export function useMusicLyricsSlice(): MusicLyricsSlice {
  return useSyncExternalStore(
    subscribeMusicState,
    getMusicLyricsSliceSnapshot,
    getMusicLyricsSliceSnapshot,
  )
}
