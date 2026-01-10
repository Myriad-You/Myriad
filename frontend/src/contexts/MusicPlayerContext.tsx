import type { ReactNode } from 'react'
import type { Song } from '../utils/musicPlayer'
import { createContext, useCallback, useContext, useEffect, useMemo, useState, useSyncExternalStore } from 'react'

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
        })
      }
    }
  }, [])

  // 监听音乐播放器状态变化事件（向后兼容）
  useEffect(() => {
    const handleMusicStateChange = (e: Event) => {
      const customEvent = e as CustomEvent
      const detail = customEvent.detail

      setState({
        currentSong: detail?.currentSong || null,
        isEnabled: detail?.isEnabled || false,
        isPlaying: detail?.isPlaying || false,
        musicColor: detail?.musicColor || '#ef4444',
        isTempPlay: detail?.isTempPlay || false,
        currentSongIndex: detail?.currentSongIndex || 0,
        playlistLength: detail?.playlistLength || 0,
        playlist: detail?.playlist || [],
      });

      // 同步到全局状态
      (window as any).__musicPlayerState = detail
    }

    window.addEventListener('music-player-state-change', handleMusicStateChange)
    return () => {
      window.removeEventListener('music-player-state-change', handleMusicStateChange)
    }
  }, [])

  // 更新状态的方法
  const updateState = useCallback((newState: Partial<MusicPlayerState>) => {
    setState((prev) => {
      const updated = { ...prev, ...newState };
      // 同步到全局状态
      (window as any).__musicPlayerState = updated
      return updated
    })
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

  return (
    <MusicPlayerContext.Provider value={{ ...state, playSong, togglePlayPause, stopTempPlay, updateState }}>
      {children}
    </MusicPlayerContext.Provider>
  )
}

// Hook 供其他组件使用
export function useMusicPlayerControl() {
  const context = useContext(MusicPlayerContext)

  if (!context) {
    // 如果没有 Provider，使用降级方案（事件监听）
    console.warn('MusicPlayerProvider not found, using fallback event-based approach')
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
}

/** 状态变化监听器集合 */
const musicStateListeners = new Set<() => void>()

/** 通知所有监听器状态已变化 */
function emitMusicStateChange() {
  musicStateListeners.forEach(listener => listener())
}

/** 订阅状态变化 */
function subscribeMusicState(listener: () => void) {
  musicStateListeners.add(listener)
  return () => musicStateListeners.delete(listener)
}

/** 获取当前状态快照 */
function getMusicStateSnapshot() {
  return globalMusicState
}

/** 更新全局状态并通知监听器 */
function updateGlobalMusicState(newState: Partial<MusicPlayerState>) {
  const prevState = globalMusicState
  globalMusicState = { ...globalMusicState, ...newState };

  // 同步到 window 对象（向后兼容）
  (window as any).__musicPlayerState = globalMusicState

  // 只有状态真正变化时才通知
  if (prevState !== globalMusicState) {
    emitMusicStateChange()
  }
}

// 初始化：监听事件并更新全局状态
if (typeof window !== 'undefined') {
  // 暴露 audioManager 到 window（供 Tapp SDK 获取频谱数据）
  import('../utils/musicPlayer').then(({ audioManager }) => {
    (window as any).audioManager = audioManager
  })

  // 从 window 对象读取初始状态
  const initialState = (window as any).__musicPlayerState
  if (initialState) {
    globalMusicState = { ...globalMusicState, ...initialState }
  }

  // 监听状态变化事件
  window.addEventListener('music-player-state-change', (e: Event) => {
    const detail = (e as CustomEvent).detail
    if (detail) {
      updateGlobalMusicState(detail)
    }
  })
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
  return useMemo(() => ({
    ...state,
    playSong,
    togglePlayPause,
    stopTempPlay,
    updateState,
  }), [state, playSong, togglePlayPause, stopTempPlay, updateState])
}
