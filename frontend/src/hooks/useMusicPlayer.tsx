import type { MusicSource, Song } from '../utils/musicPlayer'
import type {
  MusicColors,
  MusicPlayerView,
  PlayMode,
  TempPlayMode,
  UseMusicPlayerReturn,
} from './musicPlayer/types'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { emitAppEvent } from '../utils/appEvents'
import { notifyHttpRateLimit } from '../utils/httpRateLimitToast'
import {
  classifyMusicLoadError,
  musicProxyFailureKey,
} from '../utils/musicError'
import {
  audioManager,
  clampSeekTime,
  createPlaybackAudioElement,
  destroyPlaybackAudioElement,
  filterPlaylist,
  getCurrentLyricIndex,
  getLocalPlaylist,
  getMusicProxyFallbackUrl,
  getNeteasePlaylist,
  getQQPlaylist,
  pickAdjacentIndex,
  pickShuffleIndex,
  setMusicStreamProxyEnabled,
  shouldPreserveNativeAudioOutput,
  throttle,
  withSpectrumSafePlaybackUrl,
} from '../utils/musicPlayer'
import {
  readLiveAudioProgress,
  resolveMusicPalette,
} from '../utils/musicPlayerState'
import { proxyImageUrlOr } from '../utils/proxyImageUrl'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { loadResource } from '../utils/resourceLoader'
import {
  getGlobalState,
  patchLiveGlobalState,
  patchPlaybackFlags,
  publishMusicPlayerSnapshot,
  setGlobalState,
} from './musicPlayer/globalState'
import { useCoverColors, warmCoverImage } from './musicPlayer/useCoverColors'
import { useMusicPlayerHostEvents } from './musicPlayer/useHostEvents'
import { useLyrics } from './musicPlayer/useLyrics'
import { usePreload } from './musicPlayer/usePreload'

export type {
  MusicColors,
  MusicPlayerView,
  PlayMode,
  UseMusicPlayerReturn,
} from './musicPlayer/types'

/** Default-on flags: only an explicit false / "false" / "0" turns them off. */
function configFlagOn(value: unknown): boolean {
  return value !== false && value !== 0 && value !== 'false' && value !== '0'
}

export function useMusicPlayer(): UseMusicPlayerReturn {
  const [playlist, setPlaylist] = useState<Song[]>([])
  const playlistRef = useRef<Song[]>([])
  playlistRef.current = playlist
  const [currentSongIndex, setCurrentSongIndex] = useState(0)
  const [currentSong, setCurrentSong] = useState<Song | null>(null)
  const [isPlaying, setIsPlaying] = useState(false)
  const [isAudioLoading, setIsAudioLoading] = useState(false)
  const [currentTime, setCurrentTime] = useState(0)
  const [audioDuration, setAudioDuration] = useState(0)
  const [volume, setVolume] = useState(0.7)
  const [musicEnabled, setMusicEnabled] = useState(false)
  const [musicSource, setMusicSource] = useState<MusicSource>('netease')
  const [playlistId, setPlaylistId] = useState('')
  const [musicErrorKey, setMusicErrorKey] = useState('')
  const [musicErrorDetail, setMusicErrorDetail] = useState('')
  const musicErrorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const musicErrorSeqRef = useRef(0)

  const flashMusicError = useCallback((key: string, detail = '') => {
    musicErrorSeqRef.current += 1
    if (musicErrorTimerRef.current) {
      clearTimeout(musicErrorTimerRef.current)
    }
    setMusicErrorKey(key)
    setMusicErrorDetail(detail)
    musicErrorTimerRef.current = setTimeout(() => {
      setMusicErrorKey('')
      setMusicErrorDetail('')
      musicErrorTimerRef.current = null
    }, 4000)
  }, [])
  useEffect(
    () => () => {
      if (musicErrorTimerRef.current) {
        clearTimeout(musicErrorTimerRef.current)
      }
    },
    [],
  )

  const [musicPlayerView, setMusicPlayerView] =
    useState<MusicPlayerView>('info')
  const [playMode, setPlayMode] = useState<PlayMode>('loop')

  const initializedRef = useRef(false)
  useEffect(() => {
    if (initializedRef.current) return
    initializedRef.current = true

    const globalState = getGlobalState()
    if (globalState) {
      if (globalState.playlist) setPlaylist(globalState.playlist as Song[])
      if (typeof globalState.currentSongIndex === 'number') {
        setCurrentSongIndex(globalState.currentSongIndex)
      }
      if (globalState.currentSong) {
        setCurrentSong(globalState.currentSong as Song)
      }
      if (typeof globalState.isEnabled === 'boolean') {
        setMusicEnabled(globalState.isEnabled)
      }
    }
  }, [])

  const [playlistSearchQuery, setPlaylistSearchQuery] = useState('')
  const [excludeVipSongs, setExcludeVipSongs] = useState(true)
  const [preloadEnabled, setPreloadEnabled] = useState(true)
  const [showVolumePopup, setShowVolumePopup] = useState(false)

  const audioRef = useRef<HTMLAudioElement | null>(null)
  const playlistScrollRef = useRef<HTMLDivElement>(null)
  const progressBarRef = useRef<HTMLInputElement>(null)
  const musicContainerRef = useRef<HTMLDivElement>(null)
  const volumeControlRef = useRef<HTMLDivElement>(null)
  const seekingRef = useRef(false)
  const progressUiVisibleRef = useRef(false)
  const nextShuffleIndexRef = useRef(-1)
  const tempPlayModeRef = useRef<TempPlayMode>({
    enabled: false,
    originalPlaylist: [],
    originalIndex: 0,
    originalSource: 'netease',
    originalPlaylistId: '',
  })
  const [isTempPlayMode, setIsTempPlayMode] = useState(false)

  const userWantsPlayingRef = useRef(false)
  const isPlayingRef = useRef(false)
  const currentSongRef = useRef<Song | null>(null)
  const currentSongIndexRef = useRef(0)
  const selectGenerationRef = useRef(0)
  const pendingPlayTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const neteaseProxyFallbackTriedRef = useRef<Set<string>>(new Set())
  const audioLoadSongIdRef = useRef<string | null>(null)
  const audioLoadGenerationRef = useRef(0)
  const errorSkipCountRef = useRef(0)
  isPlayingRef.current = isPlaying
  currentSongRef.current = currentSong
  currentSongIndexRef.current = currentSongIndex

  const {
    lyrics,
    verbatimLyrics,
    verbatimLyricsSource,
    currentLyricIndex,
    lyricsRef,
    currentLyricIndexRef,
    setCurrentLyricIndex,
    resetLyrics,
    loadLyricsForSong,
  } = useLyrics()

  const {
    musicColors,
    musicColorsRef,
    peekCoverColors,
    isUsableCoverPalette,
    rememberCoverColors,
    extractCoverColorsForSong,
    prefetchAroundIndex,
    pushSongTheme,
  } = useCoverColors({
    musicContainerRef,
    audioRef,
    playlistRef,
    currentSongRef,
    currentSongIndexRef,
    selectGenerationRef,
    tempPlayModeRef,
    musicEnabled,
    preloadEnabled,
    volume,
    playMode,
  })

  const {
    preloadAudioRef,
    preloadedSongIndex,
    currentSongLoadedRef,
    currentSongStartTimeRef,
    resetPreloadForNewSong,
    resetPreloadBackoff,
    maybeTriggerPreload,
  } = usePreload({
    enabled: preloadEnabled,
    playlist,
    excludeVipSongs,
    volume,
    setPlaylist,
    neteaseProxyFallbackTriedRef,
  })

  const filteredPlaylist = useMemo(
    () =>
      filterPlaylist(playlist, playlistSearchQuery, {
        hideVip: excludeVipSongs && !isTempPlayMode,
      }),
    [playlist, playlistSearchQuery, excludeVipSongs, isTempPlayMode],
  )

  const setProgressUiVisible = useCallback((visible: boolean) => {
    progressUiVisibleRef.current = visible
    if (visible && audioRef.current) {
      setCurrentTime(audioRef.current.currentTime)
    }
  }, [])

  const lastBroadcastRef = useRef('')
  const broadcastStateChange = useCallback(() => {
    const audio = audioRef.current
    const live = readLiveAudioProgress(audio)
    const liveCurrentTime =
      audio && Number.isFinite(audio.currentTime)
        ? live.currentTime
        : currentTime
    const liveDuration =
      live.audioDuration > 0 ? live.audioDuration : audioDuration

    const stateSnapshot = JSON.stringify({
      songId: currentSong?.id,
      isEnabled: musicEnabled,
      isPlaying,
      color: musicColors?.primary,
      isTempPlay: tempPlayModeRef.current.enabled,
      index: currentSongIndex,
      length: playlist.length,
      time: Math.floor(liveCurrentTime),
      volume: Math.round(volume * 100),
      mode: playMode,
      lyrics: lyrics.length,
      lyricIndex: currentLyricIndex,
      verbatim: verbatimLyrics.length,
      verbatimSource: verbatimLyricsSource,
    })

    if (lastBroadcastRef.current === stateSnapshot) {
      return
    }
    lastBroadcastRef.current = stateSnapshot

    const prevG = getGlobalState()
    const resolvedPalette = resolveMusicPalette(
      musicColors,
      (prevG?.musicColors as MusicColors | null | undefined) ??
        musicColorsRef.current,
    )

    publishMusicPlayerSnapshot({
      song: currentSong,
      index: currentSongIndex,
      colors: resolvedPalette,
      isPlaying,
      isEnabled: musicEnabled,
      volume,
      playMode,
      playlist,
      isTempPlay: tempPlayModeRef.current.enabled,
      resetProgress: false,
      liveCurrentTime,
      liveDuration,
      lyrics,
      verbatimLyrics,
      hasVerbatimLyrics: verbatimLyrics.length > 0,
      verbatimLyricsSource,
      currentLyricIndex,
      generation: selectGenerationRef.current,
      isLoading: isAudioLoading,
    })
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
    musicColorsRef,
  ])

  const selectSong = useCallback(
    async (songIn: Song, index: number, autoPlay: boolean = false) => {
      if (
        excludeVipSongs &&
        songIn.isVip &&
        !tempPlayModeRef.current.enabled
      ) {
        return
      }

      const proxiedCover = proxyImageUrlOr(songIn.cover, songIn.cover || '')
      const withCover: Song =
        proxiedCover && proxiedCover !== songIn.cover
          ? { ...songIn, cover: proxiedCover }
          : songIn.cover
            ? songIn
            : { ...songIn, cover: proxiedCover }
      const song = withSpectrumSafePlaybackUrl(withCover)

      const generation = ++selectGenerationRef.current
      const isCurrentSelect = () => selectGenerationRef.current === generation

      if (pendingPlayTimeoutRef.current !== null) {
        clearTimeout(pendingPlayTimeoutRef.current)
        pendingPlayTimeoutRef.current = null
      }

      resetPreloadForNewSong()
      currentSongIndexRef.current = index
      currentSongRef.current = song

      setCurrentSong(song)
      setCurrentSongIndex(index)
      setAudioDuration(0)
      setCurrentTime(0)
      resetLyrics()

      let immediateColors: MusicColors | null = null
      if (song.cover) {
        immediateColors = peekCoverColors(song.cover)
        if (immediateColors) {
          rememberCoverColors(song.cover, immediateColors)
        }
      }
      pushSongTheme(song, index, immediateColors, false, {
        resetProgress: true,
      })
      setIsPlaying(false)
      patchPlaybackFlags({
        isAudioLoading: true,
        lastPlaybackError: null,
        generation,
      })

      if (song.cover) {
        warmCoverImage(song.cover, 'high')
      }

      loadLyricsForSong(song)

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
              if (el.paused) {
                setIsPlaying(false)
                audioManager.setPlaybackState('paused')
                return
              }
              setIsPlaying(true)
              audioManager.setPlaybackState('playing')
            } catch {
              if (!isCurrentSelect()) return
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

      if (playMode === 'shuffle' && playlist.length > 1) {
        const nextIndex = pickShuffleIndex(playlist, index, excludeVipSongs)
        if (nextIndex !== -1 && nextIndex !== index) {
          nextShuffleIndexRef.current = nextIndex
        }
      }

      if (song.cover && !immediateColors) {
        extractCoverColorsForSong(song, index, generation, 0)
      }

      prefetchAroundIndex(index)
    },
    [
      playlist,
      playMode,
      excludeVipSongs,
      loadLyricsForSong,
      resetLyrics,
      pushSongTheme,
      prefetchAroundIndex,
      rememberCoverColors,
      extractCoverColorsForSong,
      peekCoverColors,
      resetPreloadForNewSong,
    ],
  )

  const playSong = useCallback(
    (songIn: Song) => {
      const cover = proxyImageUrlOr(songIn.cover, songIn.cover || '')
      const song: Song = { ...songIn, cover }

      if (!audioRef.current) {
        audioRef.current = createPlaybackAudioElement(volume)
        audioManager.setCurrentAudio(audioRef.current, song)
      }

      if (!tempPlayModeRef.current.enabled) {
        tempPlayModeRef.current = {
          enabled: true,
          originalPlaylist: Iterator.from(playlistRef.current).toArray(),
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

      const tempList = [song]
      playlistRef.current = tempList
      setPlaylist(tempList)

      void selectSong(song, 0, true)
    },
    [musicEnabled, volume, musicSource, playlistId, selectSong],
  )

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

  const loadPlaylist = useCallback(
    async (source: MusicSource, plistId: string, autoPlay: boolean = false) => {
      // Local must re-run every time (resourceLoader dedupes by completed id).
      const taskKey
        = source === 'local'
          ? `music-playlist-local-${Date.now()}`
          : `music-playlist-${source}-${plistId}`
      loadResource.medium(taskKey, async () => {
        try {
          setMusicErrorKey('')
          setMusicErrorDetail('')
          neteaseProxyFallbackTriedRef.current.clear()
          const songs =
            source === 'local'
              ? await getLocalPlaylist(plistId || 'local')
              : source === 'netease'
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
            void selectSong(songs[firstSongIndex], firstSongIndex, autoPlay)
          } else {
            flashMusicError('playlistEmpty')
          }
        } catch (error) {
          console.error('Failed to load music playlist:', error)
          const classified = classifyMusicLoadError(error)
          flashMusicError(classified.key, classified.detail)
          playlistRef.current = []
          setPlaylist([])
        }
      })
    },
    [selectSong, excludeVipSongs, flashMusicError],
  )

  // Config panel / local library can request a fresh playlist load.
  useEffect(() => {
    const onReload = (event: Event) => {
      const detail = (event as CustomEvent<{
        playlistId?: string
        source?: string
        autoPlay?: boolean
      }>).detail
      const source = (detail?.source as MusicSource) || musicSource
      const id = detail?.playlistId || (source === 'local' ? 'local' : playlistId)
      if (!id) return
      setMusicSource(source)
      setPlaylistId(id)
      if (musicEnabled) void loadPlaylist(source, id, Boolean(detail?.autoPlay))
    }
    window.addEventListener('music-player-load-playlist', onReload)
    return () => window.removeEventListener('music-player-load-playlist', onReload)
  }, [loadPlaylist, musicEnabled, musicSource, playlistId])

  const loadMusicConfig = useCallback(async () => {
    try {
      resetPreloadBackoff()

      const data = await getUIConfigDeduped()

      const enabled = data.music_enabled === 'true'
      const source = data.music_source || 'netease'
      const { normalizeMusicPlaylistId } = await import(
        '../utils/musicPlaylistId',
      )
      const rawId = normalizeMusicPlaylistId(data.music_playlist_id || '')
      // Local library always plays the on-site catalog when no id is set.
      const plistId = source === 'local' && !rawId ? 'local' : rawId

      // Must land before loadPlaylist: playback URLs are built from it.
      setMusicStreamProxyEnabled(configFlagOn(data.music_proxy_enabled))
      setPreloadEnabled(configFlagOn(data.music_preload_enabled))
      setMusicEnabled(enabled)
      setMusicSource(source as MusicSource)
      setPlaylistId(plistId)

      if (enabled && plistId) {
        void loadPlaylist(source as MusicSource, plistId)
      }

      broadcastStateChange()
    } catch {

    }
  }, [loadPlaylist, broadcastStateChange, resetPreloadBackoff])

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
            flashMusicError('playFailed')
            setIsPlaying(false)
            userWantsPlayingRef.current = false
          } else {
            await new Promise((resolve) => setTimeout(resolve, 1000 * retries))
          }
        }
      }
    }
  }, [isPlaying, currentSong, musicSource, flashMusicError])

  const playPrevious = useCallback(() => {
    if (playlist.length === 0) return

    const fromIndex = currentSongIndexRef.current
    let newIndex: number

    if (playMode === 'shuffle') {
      newIndex = pickShuffleIndex(playlist, fromIndex, excludeVipSongs)
    } else {
      const adjacent = pickAdjacentIndex(
        playlist,
        fromIndex,
        -1,
        excludeVipSongs,
      )
      if (adjacent === null) {
        console.warn('所有歌曲都是VIP，无法播放')
        return
      }
      newIndex = adjacent
    }

    currentSongIndexRef.current = newIndex
    void selectSong(playlist[newIndex], newIndex, true)
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
    musicSource,
  ])

  const playNext = useCallback(() => {
    if (playlist.length === 0) return

    const fromIndex = currentSongIndexRef.current
    let newIndex: number

    if (playMode === 'shuffle') {
      newIndex =
        nextShuffleIndexRef.current !== -1
          ? nextShuffleIndexRef.current
          : pickShuffleIndex(playlist, fromIndex, excludeVipSongs)
      nextShuffleIndexRef.current = -1
    } else {
      const adjacent = pickAdjacentIndex(
        playlist,
        fromIndex,
        1,
        excludeVipSongs,
      )
      if (adjacent === null) {
        console.warn('所有歌曲都是VIP，无法播放')
        return
      }
      newIndex = adjacent
    }

    currentSongIndexRef.current = newIndex
    void selectSong(playlist[newIndex], newIndex, true)
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
    musicSource,
  ])

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
      } catch {

      }
    }
  }, [])

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

      const safeTime = clampSeekTime(time, duration)
      audio.currentTime = safeTime
      setCurrentTime(safeTime)
    },
    [currentSong, audioDuration],
  )

  const handleSeekStart = useCallback(() => {
    seekingRef.current = true
  }, [])

  const handleSeekEnd = useCallback(() => {
    setTimeout(() => {
      seekingRef.current = false
    }, 100)
  }, [])

  const togglePlayMode = useCallback(() => {
    setPlayMode((prev) => {
      if (prev === 'loop') return 'single'
      if (prev === 'single') return 'shuffle'
      return 'loop'
    })
  }, [])

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

  useEffect(() => {
    if (!audioRef.current) {
      audioRef.current = createPlaybackAudioElement(volume)
      audioManager.setCurrentAudio(audioRef.current, currentSong)
    }

    const audio = audioRef.current
    let errorAdvanceTimer: ReturnType<typeof setTimeout> | null = null

    const handleTimeUpdate = throttle(() => {
      const t = audio.currentTime
      if (progressUiVisibleRef.current) {
        setCurrentTime(t)
      }

      if (audio.duration && Number.isFinite(audio.duration)) {
        audioManager.updatePositionState(
          audio.duration,
          t,
          audio.playbackRate,
        )
      }

      patchLiveGlobalState({
        currentTime: t,
        audioDuration: audio.duration || 0,
      })

      emitAppEvent('music-player-progress', {
            currentTime: t,
            audioDuration: audio.duration || 0,
            songId: currentSongRef.current?.id ?? null,
          })

      if (lyricsRef.current.length > 0) {
        const index = getCurrentLyricIndex(lyricsRef.current, t)
        if (index !== currentLyricIndexRef.current) {
          currentLyricIndexRef.current = index
          patchLiveGlobalState({ currentLyricIndex: index })
          setCurrentLyricIndex(index)
        }
      }

      maybeTriggerPreload(playMode, currentSongIndex, nextShuffleIndexRef)
    }, 200)

    const handleCanPlay = () => {
      if (
        selectGenerationRef.current !== audioLoadGenerationRef.current ||
        currentSongRef.current?.id !== audioLoadSongIdRef.current
      ) {
        return
      }
      errorSkipCountRef.current = 0
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

      const settled = currentSongRef.current
      const gen = selectGenerationRef.current
      if (settled?.cover) {
        const hit = peekCoverColors(settled.cover)
        if (!isUsableCoverPalette(hit)) {
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

      const song = currentSongRef.current
      if (
        !song ||
        song.id !== audioLoadSongIdRef.current ||
        selectGenerationRef.current !== audioLoadGenerationRef.current
      ) {
        return
      }

      const probeUrl = song.url || ''
      let failureKeyProbe: Promise<string | null> | null = null
      if (
        probeUrl.includes('/api/proxy/music/') ||
        probeUrl.includes('/proxy/music/')
      ) {
        failureKeyProbe = fetch(probeUrl, { method: 'GET', cache: 'no-store' })
          .then(async (res) => {
            if (res.ok) return null
            let body: unknown
            try {
              body = await res.clone().json()
            } catch {
              body = undefined
            }
            if (res.status === 429) {
              notifyHttpRateLimit(res, body)
              return null
            }
            return musicProxyFailureKey(body)
          })
          .catch(() => null)
      }

      if (song.source === 'netease' || song.source === 'qq') {
        const fallback = getMusicProxyFallbackUrl(song)
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
      flashMusicError(song.isVip ? 'vipPlayFailed' : 'playFailed')
      if (failureKeyProbe) {
        // Only replace the generic flash we just showed, never a newer one.
        const flashSeq = musicErrorSeqRef.current
        void failureKeyProbe.then((key) => {
          if (!key || musicErrorSeqRef.current !== flashSeq) return
          if (!musicErrorTimerRef.current) return
          flashMusicError(key)
        })
      }

      if (playlist.length > 1 && playMode !== 'single') {
        if (errorAdvanceTimer !== null) clearTimeout(errorAdvanceTimer)
        errorAdvanceTimer = setTimeout(() => {
          if (
            selectGenerationRef.current !== audioLoadGenerationRef.current ||
            currentSongRef.current?.id !== audioLoadSongIdRef.current
          ) {
            return
          }
          // Stop auto-skip after a few consecutive failures (dead URLs would loop forever).
          errorSkipCountRef.current += 1
          if (errorSkipCountRef.current > 3) {
            errorSkipCountRef.current = 0
            userWantsPlayingRef.current = false
            return
          }
          const nextIndex = (currentSongIndex + 1) % playlist.length
          if (playlist[nextIndex]) {
            void selectSong(playlist[nextIndex], nextIndex, true)
          }
        }, 1000)
      }
    }

    const handleEnded = async () => {
      if (seekingRef.current) return
      if (audio !== audioRef.current) return

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
            newIndex = pickShuffleIndex(
              playlist,
              currentSongIndex,
              excludeVipSongs,
            )
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

        if (
          preloadedSongIndex === newIndex &&
          preloadAudioRef.current &&
          preloadAudioRef.current.readyState >= 2
        ) {
          const generation = ++selectGenerationRef.current
          const isCurrentSelect = () =>
            selectGenerationRef.current === generation

          setIsAudioLoading(false)
          resetPreloadForNewSong()
          currentSongIndexRef.current = newIndex
          currentSongRef.current = nextSong
          audioLoadSongIdRef.current = nextSong.id
          audioLoadGenerationRef.current = generation

          setCurrentSong(nextSong)
          setCurrentSongIndex(newIndex)
          setCurrentTime(0)
          setAudioDuration(0)
          resetLyrics()

          let immediateColors: MusicColors | null = null
          if (nextSong.cover) {
            immediateColors = peekCoverColors(nextSong.cover)
            if (immediateColors) {
              rememberCoverColors(nextSong.cover, immediateColors)
            }
          }
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
            warmCoverImage(nextSong.cover, 'high')
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
              setIsPlaying(false)
            }
          }

          loadLyricsForSong(nextSong)

          if (nextSong.cover && !immediateColors) {
            extractCoverColorsForSong(nextSong, newIndex, generation, 0)
          }

          if (playMode === 'shuffle' && playlist.length > 1) {
            const nextIndex = pickShuffleIndex(
              playlist,
              newIndex,
              excludeVipSongs,
            )
            if (nextIndex !== -1 && nextIndex !== newIndex) {
              nextShuffleIndexRef.current = nextIndex
            }
          }
          prefetchAroundIndex(newIndex)
        } else {
          void selectSong(playlist[newIndex], newIndex, true)
        }
      } else {
        setIsPlaying(false)
      }
    }

    const handlePause = () => {
      setIsPlaying(false)
      patchLiveGlobalState({ isPlaying: false })

      if (!document.hidden) {
        userWantsPlayingRef.current = false
        audioManager.setPlaybackState('paused')
      } else if (userWantsPlayingRef.current) {
        audioManager.setPlaybackState('playing')
      } else {
        audioManager.setPlaybackState('paused')
      }
    }

    const handlePlay = () => {
      setIsPlaying(true)
      userWantsPlayingRef.current = true
      patchLiveGlobalState({ isPlaying: true })
      audioManager.setPlaybackState('playing')
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
    loadLyricsForSong,
    playMode,
    excludeVipSongs,
    pushSongTheme,
    prefetchAroundIndex,
    rememberCoverColors,
    resetLyrics,
    extractCoverColorsForSong,
    flashMusicError,
    peekCoverColors,
    isUsableCoverPalette,
    resetPreloadForNewSong,
    maybeTriggerPreload,
  ])

  const prevKeyStateRef = useRef('')

  useEffect(() => {
    const keyState = `${currentSong?.id}|${musicEnabled}|${isPlaying}|${musicColors?.primary}|${currentSongIndex}|${playlist.length}|${volume}|${playMode}|${lyrics.length}|${currentLyricIndex}|${verbatimLyrics.length}|${verbatimLyricsSource}|${isAudioLoading}|${selectGenerationRef.current}`

    if (prevKeyStateRef.current === keyState) {
      return
    }
    prevKeyStateRef.current = keyState

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

  useEffect(() => {
    setGlobalState({ excludeVipSongs })
  }, [excludeVipSongs])

  useMusicPlayerHostEvents({
    audioRef,
    currentSongRef,
    currentSongIndexRef,
    playlistRef,
    tempPlayModeRef,
    userWantsPlayingRef,
    isPlayingRef,
    setCurrentSong,
    setPlaylist,
    setCurrentTime,
    setIsPlaying,
    setPlayMode,
    setExcludeVipSongs,
    playSong,
    selectSong,
    togglePlay,
    playPrevious,
    playNext,
    stopTempPlay,
    handleSeek,
    handleVolumeChange,
    loadPlaylist,
    broadcastStateChange,
  })

  useEffect(() => {
    return () => {
      userWantsPlayingRef.current = false
      audioManager.stopCurrentAudio()

      if (audioRef.current) {
        destroyPlaybackAudioElement(audioRef.current)
        audioRef.current = null
      }
    }
  }, [])

  return {
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
    musicErrorDetail,
    musicPlayerView,
    playMode,
    musicColors,

    playlistSearchQuery,
    excludeVipSongs,
    filteredPlaylist,

    isTempPlayMode,

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

    audioRef,
    playlistScrollRef,
    progressBarRef,
    musicContainerRef,
    volumeControlRef,

    showVolumePopup,
    setShowVolumePopup,

    getPlayModeInfo,
    setProgressUiVisible,
  }
}
