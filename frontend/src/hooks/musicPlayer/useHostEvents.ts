import type { Dispatch, MutableRefObject, RefObject, SetStateAction } from 'react'
import type { MusicSource, Song } from '../../utils/musicPlayer'
import type { PlayMode, TempPlayMode } from './types'

import { useEffect, useRef } from 'react'
import { audioManager } from '../../utils/musicPlayer'

export interface MusicPlayerHostEventOpts {
  audioRef: RefObject<HTMLAudioElement | null>
  currentSongRef: MutableRefObject<Song | null>
  currentSongIndexRef: MutableRefObject<number>
  playlistRef: MutableRefObject<Song[]>
  tempPlayModeRef: MutableRefObject<TempPlayMode>
  userWantsPlayingRef: MutableRefObject<boolean>
  isPlayingRef: MutableRefObject<boolean>
  setCurrentSong: Dispatch<SetStateAction<Song | null>>
  setPlaylist: Dispatch<SetStateAction<Song[]>>
  setCurrentTime: Dispatch<SetStateAction<number>>
  setIsPlaying: Dispatch<SetStateAction<boolean>>
  setPlayMode: Dispatch<SetStateAction<PlayMode>>
  setExcludeVipSongs: Dispatch<SetStateAction<boolean>>
  playSong: (song: Song) => void
  selectSong: (song: Song, index: number, autoPlay?: boolean) => Promise<void>
  togglePlay: () => Promise<void>
  playPrevious: () => void
  playNext: () => void
  stopTempPlay: () => Promise<void>
  handleSeek: (time: number) => void
  handleVolumeChange: (volume: number) => void
  loadPlaylist: (
    source: MusicSource,
    playlistId: string,
    autoPlay?: boolean,
  ) => void
  broadcastStateChange: () => void
}

/**
 * 窗口 / Media Session / 可见性事件。回调经 optsRef 读最新闭包，监听器只绑一次。
 */
export function useMusicPlayerHostEvents(opts: MusicPlayerHostEventOpts): void {
  const optsRef = useRef(opts)
  optsRef.current = opts

  useEffect(() => {
    const handleVisibilityChange = async () => {
      const {
        audioRef,
        userWantsPlayingRef,
        isPlayingRef,
        currentSongRef,
        setIsPlaying,
      } = optsRef.current
      const audio = audioRef.current
      if (!audio) return

      if (document.hidden) {
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
          if (audio.paused) {
            try {
              await audio.play()
            } catch {
              // 后台 play 可能被拒，回前台时再恢复
            }
          }
        }
      } else {
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
  }, [])

  useEffect(() => {
    audioManager.setMediaSessionHandlers({
      play: async () => {
        const { audioRef, userWantsPlayingRef } = optsRef.current
        if (audioRef.current) {
          userWantsPlayingRef.current = true
          try {
            await audioRef.current.play()
          } catch {
            userWantsPlayingRef.current = false
          }
        }
      },
      pause: () => {
        const { audioRef, userWantsPlayingRef } = optsRef.current
        userWantsPlayingRef.current = false
        if (audioRef.current) {
          audioRef.current.pause()
        }
      },
      previoustrack: () => optsRef.current.playPrevious(),
      nexttrack: () => optsRef.current.playNext(),
      seekbackward: () => {
        const audio = optsRef.current.audioRef.current
        if (audio) {
          audio.currentTime = Math.max(0, audio.currentTime - 10)
        }
      },
      seekforward: () => {
        const audio = optsRef.current.audioRef.current
        if (audio) {
          audio.currentTime = Math.min(
            audio.duration || 0,
            audio.currentTime + 10,
          )
        }
      },
      seekto: (details) => {
        const { audioRef, setCurrentTime } = optsRef.current
        if (audioRef.current && details.seekTime !== undefined) {
          audioRef.current.currentTime = details.seekTime
          setCurrentTime(details.seekTime)
        }
      },
    })
  }, [])

  useEffect(() => {
    const handlePlaySong = (e: Event) => {
      const song = (e as CustomEvent).detail?.song
      if (song) optsRef.current.playSong(song)
    }
    window.addEventListener('play-song', handlePlaySong)
    return () => window.removeEventListener('play-song', handlePlaySong)
  }, [])

  useEffect(() => {
    const handlePatchCurrentSong = (e: Event) => {
      const {
        currentSongRef,
        tempPlayModeRef,
        playlistRef,
        setCurrentSong,
        setPlaylist,
      } = optsRef.current
      const patch = (e as CustomEvent).detail?.song as Song | undefined
      if (!patch?.id) return
      const cur = currentSongRef.current
      if (!cur || String(cur.id) !== String(patch.id)) return
      const merged: Song = {
        ...cur,
        ...patch,
        url: cur.url || patch.url,
      }
      currentSongRef.current = merged
      setCurrentSong(merged)
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

  useEffect(() => {
    const handlePlayInPlaylist = (e: Event) => {
      const { playlistRef, selectSong } = optsRef.current
      const { index, song } = (e as CustomEvent).detail || {}
      if (
        typeof index === 'number' &&
        index >= 0 &&
        index < playlistRef.current.length
      ) {
        const targetSong = song || playlistRef.current[index]
        if (targetSong) {
          void selectSong(targetSong as Song, index, true)
        }
      }
    }

    window.addEventListener('play-song-at-index', handlePlayInPlaylist)
    window.addEventListener('jump-to-index', handlePlayInPlaylist)
    return () => {
      window.removeEventListener('play-song-at-index', handlePlayInPlaylist)
      window.removeEventListener('jump-to-index', handlePlayInPlaylist)
    }
  }, [])

  useEffect(() => {
    const handleTogglePlayPause = () => {
      void optsRef.current.togglePlay()
    }
    window.addEventListener('toggle-play-pause', handleTogglePlayPause)
    return () => {
      window.removeEventListener('toggle-play-pause', handleTogglePlayPause)
    }
  }, [])

  useEffect(() => {
    const handleSyncRequest = () => {
      optsRef.current.broadcastStateChange()
    }
    window.addEventListener('request-music-state-sync', handleSyncRequest)
    return () => {
      window.removeEventListener('request-music-state-sync', handleSyncRequest)
    }
  }, [])

  useEffect(() => {
    const handleStopTempPlay = () => {
      void optsRef.current.stopTempPlay()
    }
    window.addEventListener('stop-temp-play', handleStopTempPlay)
    return () => {
      window.removeEventListener('stop-temp-play', handleStopTempPlay)
    }
  }, [])

  useEffect(() => {
    const handleTappNext = () => optsRef.current.playNext()
    const handleTappPrev = () => optsRef.current.playPrevious()
    const handleTappSeek = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && typeof detail.position === 'number') {
        optsRef.current.handleSeek(detail.position)
      }
    }
    const handleTappVolume = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && typeof detail.volume === 'number') {
        const normalizedVolume =
          detail.volume <= 1 ? detail.volume : detail.volume / 100
        optsRef.current.handleVolumeChange(normalizedVolume)
      }
    }
    const handleTappMute = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail) {
        optsRef.current.handleVolumeChange(detail.muted ? 0 : 0.7)
      }
    }
    const handleTappMode = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && detail.mode) {
        const modeMap: Record<string, PlayMode> = {
          sequence: 'loop',
          loop: 'loop',
          shuffle: 'shuffle',
          single: 'single',
        }
        optsRef.current.setPlayMode(modeMap[detail.mode] || 'loop')
      }
    }
    const handleTappSkipVip = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && typeof detail.value === 'boolean') {
        optsRef.current.setExcludeVipSongs(detail.value)
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
  }, [])

  useEffect(() => {
    const handleLoadPlaylist = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail && detail.playlistId) {
        const source = (detail.source as MusicSource) || 'netease'
        const autoPlay = detail.autoPlay !== false
        optsRef.current.loadPlaylist(source, detail.playlistId, autoPlay)
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
}
