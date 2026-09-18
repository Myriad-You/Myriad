import type { DynamicContentItem } from '../../../../services/DynamicContentProvider'
import type { BackgroundRequirement, TappInstance } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import type { AnimationConfigRef } from '../types'
import { isExlight } from '../../../../hooks/useAnimationLevel'
import { getDynamicContentProvider } from '../../../../services/DynamicContentProvider'
import { isKnownGuest } from '../../../../utils/authState'
import { analyzeBeatGrid } from '../../../../utils/beatAnalyzer'
import { getClientGeoLocation } from '../../../../utils/geoLocation'
import {
  getLyricsWithVerbatim,
  getNeteaseAudioUrlImmediate,
  getQQAudioUrlImmediate,
} from '../../../../utils/musicPlayer'
import { proxyImageUrlOr } from '../../../../utils/proxyImageUrl'
import { userFacingError } from '../../../../utils/userFacingError'
import * as TappApiService from '../../../services/TappApiService'
import {
  hostBindShortcut,
  hostUnbindAllForBridge,
  hostUnbindShortcut,
} from '../../HostShortcutManager'
import { getTappRuntime } from '../../TappRuntime'

export function registerMediaHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): () => void {
  const analysisController = new AbortController()
  const HIGH_FREQUENCY_ACTIONS = new Set(['seek', 'volume', 'mute', 'unmute'])

  bridge.registerHandler('media.control', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { action, value } = (params || {}) as {
      action?: string
      value?: unknown
    }
    try {
      switch (action) {
        case 'play':
          {
            const globalState = (
              window as { __musicPlayerState?: Record<string, unknown> }
            ).__musicPlayerState
            if (!globalState?.isPlaying) {
              window.dispatchEvent(new CustomEvent('toggle-play-pause'))
            }
          }
          break
        case 'pause':
          {
            const globalState = (
              window as { __musicPlayerState?: Record<string, unknown> }
            ).__musicPlayerState
            if (globalState?.isPlaying) {
              window.dispatchEvent(new CustomEvent('toggle-play-pause'))
            }
          }
          break
        case 'next':
          window.dispatchEvent(new CustomEvent('music-player-next'))
          break
        case 'prev':
          window.dispatchEvent(new CustomEvent('music-player-prev'))
          break
        case 'seek':
          window.dispatchEvent(
            new CustomEvent('music-player-seek', {
              detail: { position: value },
            }),
          )
          break
        case 'volume':
          window.dispatchEvent(
            new CustomEvent('music-player-volume', {
              detail: { volume: value },
            }),
          )
          break
        case 'mute':
          window.dispatchEvent(
            new CustomEvent('music-player-mute', { detail: { muted: true } }),
          )
          break
        case 'unmute':
          window.dispatchEvent(
            new CustomEvent('music-player-mute', { detail: { muted: false } }),
          )
          break
        case 'mode':
          window.dispatchEvent(
            new CustomEvent('music-player-mode', { detail: { mode: value } }),
          )
          break
      }

      if (!HIGH_FREQUENCY_ACTIONS.has(action || '')) {
        const runtimeGrant = await bridge.getRuntimeGrant()
        TappApiService.mediaControl(
          {
            tappId: tappInstance.id,
            action: (action || 'play') as
              | 'play'
              | 'pause'
              | 'next'
              | 'prev'
              | 'seek'
              | 'volume'
              | 'mute'
              | 'unmute'
              | 'mode',
            value,
          },
          runtimeGrant,
        ).catch(() => {})
      }

      return { success: true, data: { action, value } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('media.getStatus', async () => {
    const globalState = (
      window as { __musicPlayerState?: Record<string, unknown> }
    ).__musicPlayerState
    if (globalState) {
      const currentSong = globalState.currentSong as Record<
        string,
        unknown
      > | null
      const currentTime = (globalState.currentTime as number) || 0
      const audioDuration =
        (globalState.audioDuration as number) ||
        (currentSong?.duration as number) ||
        0
      const volume = (globalState.volume as number) ?? 0.7
      const playMode = (globalState.playMode as string) || 'loop'
      const lyrics =
        (globalState.lyrics as Array<{ time: number; text: string }>) || []
      const currentLyricIndex = (globalState.currentLyricIndex as number) ?? -1
      const musicColor = (globalState.musicColor as string) || '#fc3c44'
      const musicColors = globalState.musicColors as {
        primary: string
        secondary: string
        accent: string
        light: string
        dark: string
      } | null

      const modeMap: Record<string, string> = {
        loop: 'loop',
        single: 'single',
        shuffle: 'shuffle',
      }
      const apiMode = modeMap[playMode] || 'sequence'

      return {
        success: true,
        data: {
          isPlaying: globalState.isPlaying || false,
          isPaused: !globalState.isPlaying && currentSong !== null,
          isLoading: Boolean(globalState.isAudioLoading),
          generation:
            typeof globalState.generation === 'number'
              ? globalState.generation
              : 0,
          lastError:
            (globalState.lastPlaybackError as string | null | undefined) ??
            null,
          currentTrack: currentSong
            ? {
                id: currentSong.id || '',
                title: currentSong.name || currentSong.title || '',
                artist: currentSong.artist || '',
                album: currentSong.album || '',
                cover: currentSong.cover || '',
                duration: currentSong.duration || 0,
                source: currentSong.source || 'unknown',
                isVip: currentSong.isVip || false,
                isTrial: currentSong.isTrial || false,
              }
            : null,
          progress: {
            current: currentTime,
            duration: audioDuration,
            percentage:
              audioDuration > 0 ? (currentTime / audioDuration) * 100 : 0,
          },
          playlist: globalState.playlist
            ? {
                id: 'current',
                name: 'Current Playlist',
                tracks:
                  (globalState.playlistLength as number) ||
                  (globalState.playlist as unknown[]).length ||
                  0,
              }
            : null,
          mode: apiMode,
          volume: Math.round(volume * 100),
          muted: volume === 0,
          lyrics,
          currentLyricIndex,
          primaryColor: musicColor,
          secondaryColor: musicColors?.secondary || musicColor,
          accentColor: musicColors?.accent || musicColor,
          lightColor: musicColors?.light || '#ffffff',
          darkColor: musicColors?.dark || '#000000',
        },
      }
    }
    return {
      success: true,
      data: {
        isPlaying: false,
        isPaused: false,
        currentTrack: null,
        progress: { current: 0, duration: 0, percentage: 0 },
        playlist: null,
        mode: 'sequence',
        volume: 70,
        muted: false,
        lyrics: [],
        currentLyricIndex: -1,
        primaryColor: '#fc3c44',
        secondaryColor: '#fc3c44',
        accentColor: '#fc3c44',
        lightColor: '#ffffff',
        darkColor: '#000000',
      },
    }
  })

  bridge.registerHandler('media.getPlaylist', async () => {
    const globalState = (
      window as { __musicPlayerState?: Record<string, unknown> }
    ).__musicPlayerState
    if (globalState?.playlist) {
      const playlist = globalState.playlist as Array<Record<string, unknown>>
      const tracks = playlist.map((song, index) => ({
        id: song.id || String(index),
        index,
        title: song.name || song.title || 'Unknown',
        artist: song.artist || 'Unknown',
        album: song.album || '',
        cover: song.cover || '',
        duration: song.duration || 0,
        source: song.source || 'unknown',
        isVip: song.isVip || false,
        isTrial: song.isTrial || false,
        isCurrent: index === globalState.currentSongIndex,
      }))
      return {
        success: true,
        data: {
          tracks,
          currentIndex: globalState.currentSongIndex || 0,
          total: tracks.length,
        },
      }
    }
    return { success: true, data: { tracks: [], currentIndex: 0, total: 0 } }
  })

  bridge.registerHandler('media.getSkipVip', async () => {
    const globalState = (
      window as { __musicPlayerState?: Record<string, unknown> }
    ).__musicPlayerState
    const skipVip = globalState ? globalState.excludeVipSongs !== false : true
    return { success: true, data: { skipVip } }
  })

  bridge.registerHandler('media.setSkipVip', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { value } = (params || {}) as { value?: boolean }
    const skipVip = !!value
    window.dispatchEvent(
      new CustomEvent('music-player-set-skip-vip', {
        detail: { value: skipVip },
      }),
    )
    return { success: true, data: { skipVip } }
  })

  let spectrumCache: { data: unknown; timestamp: number } | null = null
  const SPECTRUM_CACHE_TTL = 16

  const EMPTY_SPECTRUM = {
    spectrum: [] as number[],
    bands: [] as number[],
    energy: 0,
    bass: 0,
    mid: 0,
    high: 0,
  }

  const readSpectrum = (): unknown => {
    const now = Date.now()
    if (spectrumCache && now - spectrumCache.timestamp < SPECTRUM_CACHE_TTL) {
      return spectrumCache.data
    }

    const audioManager = (
      window as {
        audioManager?: {
          getSpectrumData: () => number[]
          getSpectrumBands?: () => number[]
        }
      }
    ).audioManager
    if (!audioManager || typeof audioManager.getSpectrumData !== 'function') {
      return EMPTY_SPECTRUM
    }

    const spectrum = audioManager.getSpectrumData()
    const bands =
      typeof audioManager.getSpectrumBands === 'function'
        ? audioManager.getSpectrumBands()
        : []
    const energy =
      spectrum.length >= 4
        ? (spectrum[0] + spectrum[1] + spectrum[2] + spectrum[3]) * 0.25
        : 0
    const result = {
      spectrum,
      bands,
      energy,
      bass: bands.length >= 8 ? (bands[0] + bands[1]) * 0.5 : spectrum[0] || 0,
      mid: bands.length >= 8 ? (bands[3] + bands[4]) * 0.5 : spectrum[2] || 0,
      high: bands.length >= 8 ? (bands[6] + bands[7]) * 0.5 : 0,
    }
    spectrumCache = { data: result, timestamp: now }
    return result
  }

  bridge.registerHandler('media.getSpectrum', async () => {
    return { success: true, data: readSpectrum() }
  })

  /** 频谱改宿主按帧 emit；逐帧 request 会打满入站限速。emit 不计入入站配额。 */
  let spectrumRaf: number | null = null
  let spectrumStreaming = false

  const stopSpectrumStream = () => {
    spectrumStreaming = false
    if (spectrumRaf !== null) {
      cancelAnimationFrame(spectrumRaf)
      spectrumRaf = null
    }
  }

  const spectrumTick = () => {
    spectrumRaf = null
    if (!spectrumStreaming) return
    // 桥已销毁则循环必须收尾，否则泄漏到下一实例。
    if (bridge.isDestroyed()) {
      stopSpectrumStream()
      return
    }
    bridge.emit('mediaSpectrum', readSpectrum())
    spectrumRaf = requestAnimationFrame(spectrumTick)
  }

  bridge.registerHandler('media.spectrumStream', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { enabled } = (params || {}) as { enabled?: boolean }
    if (!tappInstance.grantedPermissions?.includes('media:read')) {
      return { success: false, error: 'Missing permission: media:read' }
    }
    if (enabled === false) {
      stopSpectrumStream()
      return { success: true, data: { streaming: false } }
    }
    if (!spectrumStreaming) {
      spectrumStreaming = true
      spectrumRaf = requestAnimationFrame(spectrumTick)
    }
    return { success: true, data: { streaming: true } }
  })

  bridge.registerHandler('media.getLyrics', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { songId, source } = (params || {}) as {
      songId?: string
      source?: string
    }
    const globalState = (
      window as { __musicPlayerState?: Record<string, unknown> }
    ).__musicPlayerState
    const currentSong = globalState?.currentSong as
      | {
          id?: string
          source?: string
          name?: string
          title?: string
          artist?: string
          duration?: number
        }
      | undefined
    const id = songId || currentSong?.id
    const src = source || currentSong?.source || 'netease'

    if (!id) {
      return { success: false, error: 'No song id available' }
    }

    try {
      const lyricSource = src === 'qq' ? 'qq' : 'netease'
      const isCurrent = !songId || String(songId) === String(currentSong?.id)

      if (isCurrent && globalState) {
        const gLines =
          (globalState.lyrics as Array<{
            time: number
            text: string
            translation?: string
          }>) || []
        const gVerbatim =
          (globalState.verbatimLyrics as Array<{
            time: number
            text: string
            words?: unknown[]
            translation?: string
          }>) || []
        if (gLines.length > 0 || gVerbatim.length > 0) {
          const lines =
            gLines.length > 0
              ? gLines
              : gVerbatim.map((v) => ({
                  time: v.time,
                  text: v.text,
                  translation: v.translation,
                }))
          const hasTranslation = lines.some((l) => !!l.translation)
          return {
            success: true,
            data: {
              lines,
              verbatim: gVerbatim,
              hasVerbatim: gVerbatim.length > 0,
              source: src,
              verbatimSource:
                (globalState.verbatimLyricsSource as string) || '',
              hasTranslation,
              translationLang: hasTranslation ? 'zh' : '',
            },
          }
        }
      }

      const result = await getLyricsWithVerbatim({
        id: String(id),
        source: lyricSource,
        name: isCurrent ? currentSong?.name || currentSong?.title || '' : '',
        artist: isCurrent ? currentSong?.artist || '' : '',
        duration: isCurrent ? currentSong?.duration || 0 : 0,
      })

      return {
        success: true,
        data: {
          lines: result.lines,
          verbatim: result.verbatim,
          hasVerbatim: result.hasVerbatim,
          source: src,
          verbatimSource: result.verbatimSource,
          hasTranslation: result.hasTranslation,
          translationLang: result.translationLang,
        },
      }
    } catch (error) {
      return {
        success: false,
        error:
          userFacingError(error),
      }
    }
  })

  bridge.registerHandler('media.getBeatGrid', async () => {
    const globalState = (
      window as { __musicPlayerState?: Record<string, unknown> }
    ).__musicPlayerState
    const currentSong = globalState?.currentSong as
      { id?: string; source?: string; url?: string } | undefined
    if (!currentSong?.url || !currentSong?.id) {
      return { success: true, data: { available: false } }
    }
    const grid = await analyzeBeatGrid(
      currentSong.url,
      `${currentSong.source || 'netease'}-${currentSong.id}`,
      analysisController.signal,
    )
    if (!grid || grid.beats.length < 8) {
      return { success: true, data: { available: false } }
    }
    return {
      success: true,
      data: {
        available: true,
        songId: currentSong.id,
        bpm: grid.bpm,
        beats: grid.beats,
        accents: grid.accents,
        confidence: grid.confidence,
      },
    }
  })

  bridge.registerHandler('media.playTrack', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const raw = (params || {}) as {
      trackId?: string
      trackIndex?: number
      id?: string
      name?: string
      title?: string
      artist?: string
      album?: string
      cover?: string
      image?: string
      url?: string
      duration?: number
      source?: string
      isVip?: boolean
      song?: Record<string, unknown>
    }

    const songIn =
      raw.song && typeof raw.song === 'object'
        ? (raw.song as Record<string, unknown>)
        : raw.id || raw.trackId
          ? (raw as Record<string, unknown>)
          : null
    if (songIn && (songIn.id || songIn.trackId)) {
      const id = String(songIn.id || songIn.trackId || '')
      const source = String(songIn.source || 'netease')
      let url = String(songIn.url || '')
      if (!url) {
        if (source === 'netease') {
          url = getNeteaseAudioUrlImmediate(id)
        } else if (source === 'qq') {
          url = getQQAudioUrlImmediate(id)
        }
      }
      const rawCover = String(songIn.cover || songIn.image || '')
      const song = {
        id,
        name: String(songIn.name || songIn.title || `Track #${id}`),
        artist: String(songIn.artist || ''),
        album: String(songIn.album || ''),
        cover: proxyImageUrlOr(rawCover, rawCover),
        url,
        duration:
          typeof songIn.duration === 'number' && Number.isFinite(songIn.duration)
            ? songIn.duration
            : 0,
        source,
        isVip: !!songIn.isVip,
      }
      window.dispatchEvent(new CustomEvent('play-song', { detail: { song } }))
      window.dispatchEvent(new CustomEvent('open-control-panel'))
      return {
        success: true,
        data: {
          track: {
            id: song.id,
            title: song.name,
            artist: song.artist,
            duration: song.duration,
            cover: song.cover,
          },
        },
      }
    }

    const trackId = raw.trackId
    const trackIndex = raw.trackIndex
    const globalState = (
      window as { __musicPlayerState?: Record<string, unknown> }
    ).__musicPlayerState
    if (globalState?.playlist) {
      const playlist = globalState.playlist as Array<Record<string, unknown>>
      let targetSong: Record<string, unknown> | null = null
      let targetIndex = -1
      if (
        typeof trackIndex === 'number' &&
        trackIndex >= 0 &&
        trackIndex < playlist.length
      ) {
        targetSong = playlist[trackIndex]
        targetIndex = trackIndex
      } else if (trackId) {
        targetIndex = playlist.findIndex((s) => s.id === trackId)
        if (targetIndex >= 0) targetSong = playlist[targetIndex]
      }
      if (targetSong) {
        window.dispatchEvent(
          new CustomEvent('play-song-at-index', {
            detail: { index: targetIndex, song: targetSong },
          }),
        )
        return {
          success: true,
          data: {
            index: targetIndex,
            track: {
              id: targetSong.id,
              title: targetSong.name || targetSong.title,
              artist: targetSong.artist,
              duration: targetSong.duration,
              cover: targetSong.cover,
            },
          },
        }
      }
    }
    return { success: false, error: 'Track not found' }
  })

  bridge.registerHandler('media.jumpToIndex', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { index } = (params || {}) as { index?: number }
    const globalState = (
      window as { __musicPlayerState?: Record<string, unknown> }
    ).__musicPlayerState
    if (globalState?.playlist && typeof index === 'number') {
      const playlist = globalState.playlist as Array<Record<string, unknown>>
      if (index >= 0 && index < playlist.length) {
        const targetSong = playlist[index]
        window.dispatchEvent(
          new CustomEvent('jump-to-index', {
            detail: { index, song: targetSong },
          }),
        )
        return {
          success: true,
          data: {
            index,
            track: {
              id: targetSong.id,
              title: targetSong.name || targetSong.title,
              artist: targetSong.artist,
              duration: targetSong.duration,
              cover: targetSong.cover,
            },
          },
        }
      }
    }
    return { success: false, error: 'Invalid index or playlist not available' }
  })

  bridge.registerHandler('media.loadNeteasePlaylist', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { playlistId } = (params || {}) as { playlistId?: string }

    if (!playlistId) {
      return { success: false, error: 'Playlist ID required' }
    }

    if (!tappInstance.grantedPermissions?.includes('media:control')) {
      return {
        success: false,
        error: 'Permission denied: media:control required',
      }
    }

    try {
      window.dispatchEvent(
        new CustomEvent('music-player-load-playlist', {
          detail: {
            playlistId,
            source: 'netease',
          },
        }),
      )

      return {
        success: true,
        data: { playlistId, source: 'netease', loading: true },
      }
    } catch (error) {
      return {
        success: false,
        error:
          userFacingError(error),
      }
    }
  })

  return () => {
    analysisController.abort()
    stopSpectrumStream()
  }
}

export function registerSpeechHandlers(
  bridge: TappBridge,
  _tappInstance: TappInstance,
): void {
  bridge.registerHandler('speech.tts', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { text, voice_type, speed, volume, codec, sample_rate, emotion } =
      (params || {}) as {
        text?: string
        voice_type?: number
        speed?: number
        volume?: number
        codec?: string
        sample_rate?: number
        emotion?: string
      }

    if (!text) {
      return { success: false, error: 'Text is required' }
    }

    try {
      const { textToSpeech } = await import('../../../../services/speechApi')
      const result = await textToSpeech(
        {
          text,
          voice_type,
          speed,
          volume,
          codec,
          sample_rate,
          emotion,
        },
        await bridge.hostAttributionHeaders(),
      )
      return {
        success: result.success,
        data: result.success
          ? {
              audio: result.audio,
              session_id: result.session_id,
              cached: result.cached,
            }
          : undefined,
        error: result.error,
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('speech.getVoices', async () => {
    try {
      const { getVoiceList } = await import('../../../../services/speechApi')
      const result = await getVoiceList(await bridge.hostAttributionHeaders())
      return { success: true, data: result.voices }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('speech.getStatus', async () => {
    try {
      const { getSpeechStatus } = await import('../../../../services/speechApi')
      const result = await getSpeechStatus(
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('speech.asr', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { audio_data, format, engine, word_info } = (params || {}) as {
      audio_data?: string
      format?: string
      engine?: string
      word_info?: number
    }

    if (!audio_data) {
      return { success: false, error: 'Audio data is required' }
    }

    try {
      const { speechToText } = await import('../../../../services/speechApi')
      const result = await speechToText(
        {
          audio_data,
          format,
          engine,
          word_info,
        },
        await bridge.hostAttributionHeaders(),
      )
      return {
        success: result.success,
        data: result.success
          ? {
              text: result.text,
              duration: result.duration,
              words: result.words,
            }
          : undefined,
        error: result.error,
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })
}

export function registerBackgroundHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  const validRequirements = [
    'media',
    'sync',
    'notification',
    'scheduler',
    'event-listener',
    'realtime',
  ] satisfies BackgroundRequirement[]

  const isValidRequirement = (value: unknown): value is BackgroundRequirement =>
    typeof value === 'string' &&
    validRequirements.includes(value as BackgroundRequirement)

  bridge.registerHandler('background.require', async (message) => {
    const [requirement, reason] =
      (message.payload as { args: unknown[] }).args || []
    if (!requirement) return { success: false, error: 'Requirement required' }
    if (!isValidRequirement(requirement)) {
      return {
        success: false,
        error: `Invalid requirement. Valid: ${validRequirements.join(', ')}`,
      }
    }
    try {
      const runtime = getTappRuntime()
      runtime.registerBackgroundRequirement(tappInstance.id, requirement)
      console.log(
        `[Sandbox] ${tappInstance.id} background: ${requirement}${reason ? ` (${reason})` : ''}`,
      )
      return { success: true, data: { requirement, registered: true } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('background.release', async (message) => {
    const [requirement] = (message.payload as { args: unknown[] }).args || []
    if (!requirement) return { success: false, error: 'Requirement required' }
    if (!isValidRequirement(requirement)) {
      return {
        success: false,
        error: `Invalid requirement. Valid: ${validRequirements.join(', ')}`,
      }
    }
    try {
      const runtime = getTappRuntime()
      runtime.unregisterBackgroundRequirement(tappInstance.id, requirement)
      return { success: true, data: { requirement, released: true } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('background.list', async () => {
    try {
      const runtime = getTappRuntime()
      const requirements = runtime.getBackgroundRequirements(tappInstance.id)
      return { success: true, data: requirements }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('background.has', async (message) => {
    const [requirement] = (message.payload as { args: unknown[] }).args || []
    if (!requirement) return { success: false, error: 'Requirement required' }
    try {
      const runtime = getTappRuntime()
      const requirements = runtime.getBackgroundRequirements(tappInstance.id)
      return {
        success: true,
        data: isValidRequirement(requirement)
          ? requirements.includes(requirement)
          : false,
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })
}

export function registerAnimationHandlers(
  bridge: TappBridge,
  animationConfigRef?: React.RefObject<AnimationConfigRef>,
): void {
  bridge.registerHandler('animation.getLevel', async () => {
    return {
      success: true,
      data: animationConfigRef?.current?.level || 'standard',
    }
  })

  bridge.registerHandler('animation.shouldAnimate', async () => {
    const level = animationConfigRef?.current?.level || 'standard'
    return {
      success: true,
      data: !isExlight(level),
    }
  })

  bridge.registerHandler('animation.getConfig', async () => {
    const cfg = animationConfigRef?.current
    return {
      success: true,
      data: cfg || {
        level: 'standard',
        loop: true,
        spring: { tension: 280, friction: 20 },
        durationScale: 1,
      },
    }
  })

  bridge.registerHandler('animation.getStaggerDelay', async (message) => {
    const [index, baseDelay = 50] =
      (message.payload as { args: unknown[] }).args || []
    if (typeof index !== 'number')
      return { success: false, error: 'Index required' }
    const cfg = animationConfigRef?.current
    if (!cfg) return { success: true, data: index * (baseDelay as number) }
    let delay = baseDelay as number
    if (isExlight(cfg)) delay = 0
    else if (cfg.level === 'light') delay = (baseDelay as number) * 0.5
    return { success: true, data: index * delay * cfg.durationScale }
  })
}

export function registerDynamicContentHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('dynamicContent.set', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    const { icon, text, subtext, priority, showSubtext, expiresAt, i18n } =
      (config || {}) as {
        icon?: string
        text?: string
        subtext?: string
        priority?: number
        showSubtext?: boolean
        expiresAt?: number
        i18n?: unknown
      }
    if (!icon || !text)
      return { success: false, error: 'Icon and text required' }
    try {
      const provider = getDynamicContentProvider()
      const content: Omit<DynamicContentItem, 'sourceTappId'> = {
        type: `tapp-${tappInstance.id}`,
        icon,
        text,
        subtext,
        priority: priority ?? -1,
        showSubtext: showSubtext ?? !!subtext,
        onClick: 'expand',
        expiresAt,
        i18n: i18n as DynamicContentItem['i18n'],
      }
      provider.setTappContent(tappInstance.id, content)
      getTappRuntime().registerBackgroundRequirement(
        tappInstance.id,
        'notification',
      )
      return { success: true, data: { registered: true } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('dynamicContent.update', async (message) => {
    const [updates] = (message.payload as { args: unknown[] }).args || []
    if (!updates) return { success: false, error: 'Updates required' }
    try {
      const provider = getDynamicContentProvider()
      const existing = provider.getTappContent(tappInstance.id)
      if (!existing)
        return { success: false, error: 'No content found. Use set first.' }
      provider.setTappContent(tappInstance.id, {
        ...existing,
        ...(updates as Partial<DynamicContentItem>),
        type: existing.type,
      })
      return { success: true, data: { updated: true } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('dynamicContent.remove', async () => {
    try {
      const provider = getDynamicContentProvider()
      provider.removeTappContent(tappInstance.id)
      getTappRuntime().unregisterBackgroundRequirement(
        tappInstance.id,
        'notification',
      )
      return { success: true, data: { removed: true } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('dynamicContent.get', async () => {
    try {
      const provider = getDynamicContentProvider()
      const content = provider.getTappContent(tappInstance.id)
      return { success: true, data: content || null }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })
}

export function registerAdvancedHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): () => void {
  bridge.registerHandler('component.registerTheme', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.registerComponent(
        tappInstance.id,
        'theme',
        config as TappApiService.ComponentConfig,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('component.registerAgent', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.registerComponent(
        tappInstance.id,
        'agent',
        config as TappApiService.ComponentConfig,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('component.unregister', async (message) => {
    const [type, id] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.unregisterComponent(
        tappInstance.id,
        type as TappApiService.ComponentType,
        id as string,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('component.list', async (message) => {
    const [type] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.listComponents(
        tappInstance.id,
        type as TappApiService.ComponentType | undefined,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  const HOST_SHORTCUTS_CHANGED = 'tapp:host-shortcuts-changed'

  interface HostShortcutsChangedDetail {
    tappId: string
    type: 'register' | 'unregister'
    shortcut?: {
      id: string
      keys: string
      action?: string
      scope?: string
    }
    shortcutId?: string
    originSessionToken?: string
  }

  let rehydrateCancelled = false
  void (async () => {
    if (!tappInstance.grantedPermissions?.includes('shortcut:register')) {
      return
    }
    try {
      const listed = await TappApiService.listShortcuts(
        tappInstance.id,
        await bridge.getRuntimeGrant(),
      )
      if (rehydrateCancelled) return
      const shortcuts = listed?.shortcuts
      if (!Array.isArray(shortcuts)) return
      for (const sc of shortcuts) {
        if (rehydrateCancelled) return
        if (sc && typeof sc.id === 'string' && typeof sc.keys === 'string') {
          hostBindShortcut({
            tappId: tappInstance.id,
            shortcutId: sc.id,
            keys: sc.keys,
            action: String(sc.action || ''),
            scope: sc.scope,
            bridge,
          })
        }
      }
    } catch {
    }
  })()

  const onHostShortcutsChanged = (ev: Event) => {
    const detail = (ev as CustomEvent<HostShortcutsChangedDetail>).detail
    if (!detail || detail.tappId !== tappInstance.id) return
    const selfToken = bridge.getSessionToken()
    if (
      detail.originSessionToken &&
      selfToken &&
      detail.originSessionToken === selfToken
    ) {
      return
    }
    if (detail.type === 'register' && detail.shortcut) {
      const sc = detail.shortcut
      if (typeof sc.id === 'string' && typeof sc.keys === 'string') {
        hostBindShortcut({
          tappId: tappInstance.id,
          shortcutId: sc.id,
          keys: sc.keys,
          action: String(sc.action || ''),
          scope: sc.scope,
          bridge,
        })
      }
      return
    }
    if (detail.type === 'unregister' && typeof detail.shortcutId === 'string') {
      hostUnbindShortcut(tappInstance.id, detail.shortcutId, bridge)
    }
  }
  if (typeof window !== 'undefined') {
    window.addEventListener(HOST_SHORTCUTS_CHANGED, onHostShortcutsChanged)
  }

  const broadcastHostShortcutsChanged = (
    detail: HostShortcutsChangedDetail,
  ) => {
    if (typeof window === 'undefined') return
    window.dispatchEvent(
      new CustomEvent(HOST_SHORTCUTS_CHANGED, { detail }),
    )
  }

  bridge.registerHandler('shortcut.register', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    try {
      const cfg = config as TappApiService.ShortcutConfig
      const result = await TappApiService.registerShortcut(
        tappInstance.id,
        cfg,
        await bridge.getRuntimeGrant(),
      )
      hostBindShortcut({
        tappId: tappInstance.id,
        shortcutId: cfg.id,
        keys: cfg.keys,
        action: cfg.action || '',
        scope: cfg.scope,
        bridge,
      })
      broadcastHostShortcutsChanged({
        tappId: tappInstance.id,
        type: 'register',
        shortcut: {
          id: cfg.id,
          keys: cfg.keys,
          action: cfg.action || '',
          scope: cfg.scope,
        },
        originSessionToken: bridge.getSessionToken(),
      })
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('shortcut.unregister', async (message) => {
    const [id] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.unregisterShortcut(
        tappInstance.id,
        id as string,
        await bridge.getRuntimeGrant(),
      )
      hostUnbindShortcut(tappInstance.id, id as string, bridge)
      broadcastHostShortcutsChanged({
        tappId: tappInstance.id,
        type: 'unregister',
        shortcutId: id as string,
        originSessionToken: bridge.getSessionToken(),
      })
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('shortcut.list', async () => {
    try {
      const result = await TappApiService.listShortcuts(
        tappInstance.id,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  return () => {
    rehydrateCancelled = true
    if (typeof window !== 'undefined') {
      window.removeEventListener(HOST_SHORTCUTS_CHANGED, onHostShortcutsChanged)
    }
    hostUnbindAllForBridge(bridge)
  }
}

export function registerContextHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('context.getApp', async () => {
    try {
      return {
        success: true,
        data: await TappApiService.getContextApp(
          await bridge.getRuntimeGrant(),
        ),
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('context.getUser', async () => {
    try {
      return {
        success: true,
        data: await TappApiService.getContextUser(
          await bridge.getRuntimeGrant(),
        ),
      }
    } catch (grantError) {
      if (isKnownGuest()) {
        return {
          success: false,
          error: userFacingError(grantError),
        }
      }
      try {
        const { fetchSessionUserSnapshot } = await import(
          '../../sessionUserFallback',
        )
        const snap = await fetchSessionUserSnapshot()
        if (snap) {
          const { getDefaultLocale } = await import('../../../../i18n')
          let timezone = 'UTC'
          try {
            timezone =
              new Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
          } catch {
          }
          return {
            success: true,
            data: {
              id: snap.id,
              username: snap.username,
              display_name: snap.display_name,
              avatar: snap.avatar,
              avatar_url: snap.avatar_url,
              isAdmin: snap.isAdmin,
              role: snap.role,
              authenticated: snap.authenticated,
              connectedPlatforms: [],
              preferences: {
                language: getDefaultLocale(),
                timezone,
              },
            },
          }
        }
      } catch {
      }
      return {
        success: false,
        error: userFacingError(grantError),
      }
    }
  })

  bridge.registerHandler('context.getPlayer', async () => {
    const globalState = (
      window as { __musicPlayerState?: Record<string, unknown> }
    ).__musicPlayerState
    if (globalState) {
      const currentSong = globalState.currentSong as Record<
        string,
        unknown
      > | null
      const currentTime = (globalState.currentTime as number) || 0
      const audioDuration =
        (globalState.audioDuration as number) ||
        (currentSong?.duration as number) ||
        0
      const volume = (globalState.volume as number) ?? 0.7
      const playMode = (globalState.playMode as string) || 'loop'
      const modeMap: Record<string, string> = {
        loop: 'loop',
        single: 'single',
        shuffle: 'shuffle',
      }
      return {
        success: true,
        data: {
          isPlaying: globalState.isPlaying || false,
          isPaused: !globalState.isPlaying && currentSong !== null,
          currentTrack: currentSong
            ? {
                id: currentSong.id || '',
                title: currentSong.name || currentSong.title || '',
                artist: currentSong.artist || '',
                album: currentSong.album || '',
                cover: currentSong.cover || '',
                duration: currentSong.duration || 0,
                source: currentSong.source || 'unknown',
                isVip: currentSong.isVip || false,
                isTrial: currentSong.isTrial || false,
              }
            : null,
          progress: {
            current: currentTime,
            duration: audioDuration,
            percentage:
              audioDuration > 0 ? (currentTime / audioDuration) * 100 : 0,
          },
          playlist: globalState.playlist
            ? {
                id: 'current',
                name: 'Current Playlist',
                tracks:
                  (globalState.playlistLength as number) ||
                  (globalState.playlist as unknown[]).length ||
                  0,
              }
            : null,
          mode: modeMap[playMode] || 'sequence',
          volume: Math.round(volume * 100),
          muted: volume === 0,
        },
      }
    }
    return {
      success: true,
      data: {
        isPlaying: false,
        isPaused: false,
        currentTrack: null,
        progress: { current: 0, duration: 0, percentage: 0 },
        playlist: null,
        mode: 'sequence',
        volume: 80,
        muted: false,
      },
    }
  })

  bridge.registerHandler('context.getNavigation', async () => {
    try {
      return {
        success: true,
        data: await TappApiService.getContextNavigation(
          await bridge.getRuntimeGrant(),
        ),
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('context.getSystem', async () => {
    try {
      return {
        success: true,
        data: await TappApiService.getContextSystem(
          await bridge.getRuntimeGrant(),
        ),
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('data.transform', async (message) => {
    const [request] = (message.payload as { args: unknown[] }).args || []
    const req = request as {
      input?: unknown
      pipeline?: unknown
      output?: unknown
    }
    if (!req?.input || !req?.pipeline)
      return { success: false, error: 'Input and pipeline required' }
    try {
      const response = await TappApiService.dataTransform(
        {
          tappId: tappInstance.id,
          input: req.input as TappApiService.DataInput,
          pipeline: req.pipeline as TappApiService.ProcessStep[],
          output: req.output as TappApiService.DataOutput | undefined,
        },
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: response }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('api.execute', async (message) => {
    const [apiName, params] =
      (message.payload as { args: unknown[] }).args || []
    if (!apiName || typeof apiName !== 'string') {
      return { success: false, error: 'API name required' }
    }

    try {
      const response = await TappApiService.executeTappApi(
        tappInstance.id,
        apiName,
        params as Record<string, unknown> | undefined,
        await bridge.getRuntimeGrant(),
      )
      return {
        success: response.success,
        data: response.data,
        error: response.error,
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('api.list', async () => {
    try {
      const apis = await TappApiService.listTappApis(
        tappInstance.id,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: apis }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('context.getGeo', async () => {
    try {
      const geo = await getClientGeoLocation()
      if (!geo) {
        return { success: false, error: 'geolocation unavailable' }
      }
      return {
        success: true,
        data: {
          lat: geo.latitude,
          lon: geo.longitude,
          city: geo.city,
          region: geo.region ?? '',
          country: geo.country ?? '',
          countryCode: geo.countryCode,
        },
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })
}
