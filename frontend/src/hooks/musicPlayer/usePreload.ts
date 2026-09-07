import type { Dispatch, MutableRefObject, SetStateAction } from 'react'
import type { Song } from '../../utils/musicPlayer'
import type { PlayMode } from './types'

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  createPreloadAudioElement,
  ensureSpectrumSafePlaybackUrl,
  getMusicProxyFallbackUrl,
  pickAdjacentIndex,
  pickShuffleIndex,
} from '../../utils/musicPlayer'
import { loadResource } from '../../utils/resourceLoader'

export interface MusicPreloadApi {
  preloadAudioRef: MutableRefObject<HTMLAudioElement | null>
  preloadedSongIndex: number
  currentSongLoadedRef: MutableRefObject<boolean>
  currentSongStartTimeRef: MutableRefObject<number>
  preloadTriggeredRef: MutableRefObject<boolean>
  preloadNextSong: (nextIndex: number, force?: boolean) => void
  resetPreloadForNewSong: () => void
  resetPreloadBackoff: () => void
  maybeTriggerPreload: (
    playMode: PlayMode,
    currentSongIndex: number,
    nextShuffleIndexRef: MutableRefObject<number>,
  ) => void
}

export function usePreload(options: {
  playlist: Song[]
  excludeVipSongs: boolean
  volume: number
  setPlaylist: Dispatch<SetStateAction<Song[]>>
  neteaseProxyFallbackTriedRef: MutableRefObject<Set<string>>
}): MusicPreloadApi {
  const {
    playlist,
    excludeVipSongs,
    volume,
    setPlaylist,
    neteaseProxyFallbackTriedRef,
  } = options
  const initialVolumeRef = useRef(volume)

  const preloadAudioRef = useRef<HTMLAudioElement | null>(null)
  const [preloadedSongIndex, setPreloadedSongIndex] = useState(-1)
  const preloadCacheRef = useRef<Map<number, boolean>>(new Map())
  const preloadErrorCountRef = useRef(0)
  const preloadDisabledUntilRef = useRef(0)
  const currentSongLoadedRef = useRef(false)
  const currentSongStartTimeRef = useRef(0)
  const preloadTriggeredRef = useRef(false)

  useEffect(() => {
    const audio = createPreloadAudioElement(initialVolumeRef.current)
    preloadAudioRef.current = audio
    return () => {
      audio.pause()
      audio.removeAttribute('src')
      audio.load()
      preloadAudioRef.current = null
    }
  }, [])

  useEffect(() => {
    preloadCacheRef.current.clear()
    setPreloadedSongIndex(-1)
  }, [playlist])

  const resetPreloadForNewSong = useCallback(() => {
    currentSongLoadedRef.current = false
    currentSongStartTimeRef.current = 0
    preloadTriggeredRef.current = false
  }, [])

  const resetPreloadBackoff = useCallback(() => {
    preloadErrorCountRef.current = 0
    preloadDisabledUntilRef.current = 0
  }, [])

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
            if (!usedFallback) {
              const fallback = getMusicProxyFallbackUrl(nextSong)
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

          preloadAudio.src = ensureSpectrumSafePlaybackUrl(nextSong)
          preloadAudio.load()
        })
      })
    },
    [playlist, excludeVipSongs, setPlaylist, neteaseProxyFallbackTriedRef],
  )

  const maybeTriggerPreload = useCallback(
    (
      playMode: PlayMode,
      currentSongIndex: number,
      nextShuffleIndexRef: MutableRefObject<number>,
    ) => {
      if (!currentSongLoadedRef.current || preloadTriggeredRef.current) {
        return
      }
      const playTime = Date.now() - currentSongStartTimeRef.current
      if (playTime < 30000) return
      if (playlist.length <= 1) return

      if (playMode === 'loop') {
        const nextIndex = pickAdjacentIndex(
          playlist,
          currentSongIndex,
          1,
          excludeVipSongs,
        )
        if (
          nextIndex !== null &&
          nextIndex !== currentSongIndex &&
          !playlist[nextIndex]?.isVip
        ) {
          preloadNextSong(nextIndex)
        }
      } else if (playMode === 'shuffle') {
        const nextIndex = pickShuffleIndex(
          playlist,
          currentSongIndex,
          excludeVipSongs,
        )
        if (nextIndex !== -1 && nextIndex !== currentSongIndex) {
          nextShuffleIndexRef.current = nextIndex
          preloadNextSong(nextIndex)
        }
      }
    },
    [playlist, excludeVipSongs, preloadNextSong],
  )

  return {
    preloadAudioRef,
    preloadedSongIndex,
    currentSongLoadedRef,
    currentSongStartTimeRef,
    preloadTriggeredRef,
    preloadNextSong,
    resetPreloadForNewSong,
    resetPreloadBackoff,
    maybeTriggerPreload,
  }
}
