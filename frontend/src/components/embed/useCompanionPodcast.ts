import type { PodcastDialogue } from '../../services/phantasiaiApi'
import type { ArticleCacheResponse, TTSEngine, VoiceInfo } from '../../services/speechApi'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { PodcastPlayer } from '../../services/phantasiaiApi'
import {
  CloudPodcastPlayer,
  getArticleCacheInfo,
  getSpeechStatus,
  getTTSSettings,
  getVoiceList,
  saveTTSSettings,
} from '../../services/speechApi'
import { formatUserFacingError } from '../../utils/formatUserFacingError'

export type PodcastState = 'stopped' | 'playing' | 'paused'

interface PodcastCopy {
  cloudUnavailable: string
  cloudCacheIncomplete: string
  audioFailed: string
}

interface Options {
  itemId: number
  sourceId: number
  /** Cloud voices are behind sign-in; visitors hear the browser's. */
  signedIn: boolean
  copy: PodcastCopy
}

/** A complete cache: one voice covers every line, or host takes the even lines and guest the odd. */
export function findCompleteCacheVoices(
  cache: ArticleCacheResponse,
  lines: number,
): { hostVoiceId: number; guestVoiceId: number } | null {
  if (!cache.voices.length || lines <= 0) return null
  const single = cache.voices.find((voice) => voice.file_count >= lines)
  if (single) return { hostVoiceId: single.voice_id, guestVoiceId: single.voice_id }
  const host = cache.voices.find((voice) => voice.role === 'host')
  const guest = cache.voices.find((voice) => voice.role === 'guest')
  if (
    host &&
    guest &&
    host.file_count >= Math.ceil(lines / 2) &&
    guest.file_count >= Math.floor(lines / 2)
  ) {
    return { hostVoiceId: host.voice_id, guestVoiceId: guest.voice_id }
  }
  return null
}

export function useCompanionPodcast({ itemId, sourceId, signedIn, copy }: Options) {
  const [dialogues, setDialogues] = useState<PodcastDialogue[]>([])
  const [language, setLanguage] = useState('')
  const [state, setState] = useState<PodcastState>('stopped')
  const [index, setIndex] = useState(0)
  const [engine, setEngine] = useState<TTSEngine>('system')
  const [cloudAvailable, setCloudAvailable] = useState(false)
  const [voices, setVoices] = useState<VoiceInfo[]>([])
  const [hostVoiceId, setHostVoiceId] = useState(() => getTTSSettings().hostVoiceId)
  const [guestVoiceId, setGuestVoiceId] = useState(() => getTTSSettings().guestVoiceId)
  const [audioLoading, setAudioLoading] = useState(false)
  const [audioProgress, setAudioProgress] = useState({ loaded: 0, total: 0 })
  const [notice, setNotice] = useState('')
  const systemRef = useRef<PodcastPlayer | null>(null)
  const cloudRef = useRef<CloudPodcastPlayer | null>(null)
  const turn = useRef(0)
  const copyRef = useRef(copy)
  copyRef.current = copy

  useEffect(() => {
    const system = new PodcastPlayer()
    system.setCallbacks({
      onProgress: (at) => setIndex(at),
      onEnd: () => {
        setState('stopped')
        setIndex(0)
      },
      onStateChange: setState,
    })
    const cloud = new CloudPodcastPlayer()
    cloud.setOnProgress((at) => setIndex(at))
    cloud.setOnEnd(() => {
      setState('stopped')
      setIndex(0)
    })
    cloud.setOnLoadProgress((loaded, total) => setAudioProgress({ loaded, total }))
    systemRef.current = system
    cloudRef.current = cloud
    return () => {
      turn.current++
      system.destroy()
      cloud.destroy()
      systemRef.current = null
      cloudRef.current = null
    }
  }, [])

  const cloudReady = useMemo(
    () =>
      signedIn
        ? getSpeechStatus().then(
            (status) => status.available && status.tts_enabled,
            () => false,
          )
        : Promise.resolve(false),
    [signedIn],
  )

  useEffect(() => {
    let live = true
    void cloudReady.then((ready) => {
      if (!live) return
      setCloudAvailable(ready)
      if (!ready) return
      getVoiceList().then(
        (response) => live && setVoices(response.voices),
        () => {},
      )
    })
    return () => {
      live = false
    }
  }, [cloudReady])

  const stopAll = () => {
    systemRef.current?.stop()
    cloudRef.current?.stop()
    setState('stopped')
    setIndex(0)
  }

  const readInBrowser = async (lines: PodcastDialogue[], lang: string, mine: number) => {
    setEngine('system')
    saveTTSSettings({ engine: 'system' })
    const player = systemRef.current
    if (!player) return
    player.load(lines, lang)
    const available = await PodcastPlayer.getAvailableVoices()
    if (mine !== turn.current) return
    const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(available, lang)
    if (voiceA && voiceB) player.setVoices(voiceA, voiceB)
  }

  const loadCloud = async (
    lines: PodcastDialogue[],
    host: number | undefined,
    guest: number | undefined,
    mine: number,
    forceRegenerate = false,
  ): Promise<boolean> => {
    const player = cloudRef.current
    if (!player) return false
    setAudioLoading(true)
    setAudioProgress({ loaded: 0, total: lines.length })
    try {
      await player.load(lines, {
        sourceId,
        articleId: itemId,
        hostVoiceId: host,
        guestVoiceId: guest,
        forceRegenerate,
      })
      if (mine !== turn.current) return false
      setEngine('cloud')
      saveTTSSettings({ engine: 'cloud' })
      return true
    } catch (error) {
      const message = await formatUserFacingError(error, copyRef.current.audioFailed)
      if (mine === turn.current) setNotice(message)
      return false
    } finally {
      if (mine === turn.current) setAudioLoading(false)
    }
  }

  /** A complete cloud cache plays in the cloud voice; otherwise the browser reads it. */
  const prepare = useCallback(
    async (lines: PodcastDialogue[], lang: string) => {
      const mine = ++turn.current
      stopAll()
      setNotice('')
      setDialogues(lines)
      setLanguage(lang)
      if (lines.length === 0) return
      if (await cloudReady) {
        if (mine !== turn.current) return
        try {
          const cache = await getArticleCacheInfo(sourceId, itemId)
          if (mine !== turn.current) return
          const complete = findCompleteCacheVoices(cache, lines.length)
          if (complete) {
            setHostVoiceId(complete.hostVoiceId)
            setGuestVoiceId(complete.guestVoiceId)
            saveTTSSettings(complete)
            if (await loadCloud(lines, complete.hostVoiceId, complete.guestVoiceId, mine)) return
          } else if (cache.voices.length > 0) {
            setNotice(copyRef.current.cloudCacheIncomplete)
          }
        } catch {
          if (mine !== turn.current) return
        }
      }
      await readInBrowser(lines, lang, mine)
    },
    [cloudReady, itemId, sourceId],
  )

  const switchEngine = async (next: TTSEngine) => {
    if (next === engine || audioLoading) return
    const mine = ++turn.current
    stopAll()
    setNotice('')
    if (next === 'system') {
      await readInBrowser(dialogues, language, mine)
      return
    }
    if (!cloudAvailable) {
      setNotice(copyRef.current.cloudUnavailable)
      return
    }
    if (cloudRef.current?.hasAudio()) {
      setEngine('cloud')
      saveTTSSettings({ engine: 'cloud' })
      return
    }
    await loadCloud(dialogues, hostVoiceId, guestVoiceId, mine)
  }

  const changeVoice = async (role: 'host' | 'guest', voiceId: number | undefined) => {
    const host = role === 'host' ? voiceId : hostVoiceId
    const guest = role === 'guest' ? voiceId : guestVoiceId
    setHostVoiceId(host)
    setGuestVoiceId(guest)
    saveTTSSettings({ hostVoiceId: host, guestVoiceId: guest })
    if (engine !== 'cloud') return
    const mine = ++turn.current
    stopAll()
    await loadCloud(dialogues, host, guest, mine)
  }

  const regenerateAudio = async () => {
    if (!cloudAvailable || audioLoading || dialogues.length === 0) return
    const mine = ++turn.current
    stopAll()
    setNotice('')
    await loadCloud(dialogues, hostVoiceId, guestVoiceId, mine, true)
  }

  const cloudActive = engine === 'cloud' && cloudAvailable

  const play = async () => {
    if (cloudActive) {
      await cloudRef.current?.play()
      setState('playing')
    } else {
      systemRef.current?.play()
    }
  }

  const pause = () => {
    if (cloudActive) {
      cloudRef.current?.pause()
      setState('paused')
    } else {
      systemRef.current?.pause()
    }
  }

  const stop = () => {
    stopAll()
  }

  const seek = async (at: number, keepPlaying = true) => {
    if (at < 0 || at >= dialogues.length) return
    if (cloudActive) {
      const player = cloudRef.current
      if (!player) return
      await player.seekTo(at)
      setIndex(at)
      if (keepPlaying) {
        await player.play()
        setState('playing')
      }
    } else {
      systemRef.current?.seekTo(at, keepPlaying)
      setIndex(at)
    }
  }

  return {
    dialogues,
    state,
    index,
    engine,
    cloudAvailable,
    voices,
    hostVoiceId,
    guestVoiceId,
    audioLoading,
    audioProgress,
    notice,
    prepare,
    switchEngine,
    changeVoice,
    regenerateAudio,
    play,
    pause,
    stop,
    prev: () => seek(index - 1, state === 'playing'),
    next: () => seek(index + 1, state === 'playing'),
    seek,
  }
}
