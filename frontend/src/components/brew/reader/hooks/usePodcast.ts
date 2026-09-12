import type { PodcastDialogue } from '../../../../services/brewliaApi'
import type {
  ArticleCacheResponse,
  TTSEngine,
  VoiceInfo,
} from '../../../../services/speechApi'
import type { ReaderCopy } from '../types'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getDefaultLocale } from '../../../../i18n'
import * as brewliaApi from '../../../../services/brewliaApi'
import { PodcastPlayer } from '../../../../services/brewliaApi'
import {
  clearArticleVoiceCache,
  CloudPodcastPlayer,
  getArticleCacheInfo,
  getSpeechStatus,
  getTTSSettings,
  getVoiceList,
  isFemaleVoice,
  isMaleVoice,
  saveTTSSettings,
} from '../../../../services/speechApi'
import { userFacingError } from '../../../../utils/userFacingError'
import { RequestTurn } from '../../logic/requestTurn'
import { useArticleTaskScope } from './useArticleTaskScope'

export interface UsePodcastOptions {
  itemId: number
  sourceId: number
  isBrewlia: boolean
  showToastMessage: (message: string, duration?: number) => void
  t: ReaderCopy
}

/** 完整缓存：单音色覆盖全部索引，或 host 偶数 / guest 奇数。 */
function findCompleteCacheVoices(
  cache: ArticleCacheResponse,
  dialogueCount: number,
): { hostVoiceId: number; guestVoiceId: number; voiceName: string } | null {
  if (!cache.voices.length || dialogueCount <= 0) return null

  const singleVoice = cache.voices.find((v) => v.file_count >= dialogueCount)
  if (singleVoice) {
    return {
      hostVoiceId: singleVoice.voice_id,
      guestVoiceId: singleVoice.voice_id,
      voiceName: singleVoice.voice_name || singleVoice.voice_id.toString(),
    }
  }

  const hostVoice = cache.voices.find((v) => v.role === 'host')
  const guestVoice = cache.voices.find((v) => v.role === 'guest')

  if (hostVoice && guestVoice) {
    const expectedHostCount = Math.ceil(dialogueCount / 2)
    const expectedGuestCount = Math.floor(dialogueCount / 2)

    if (
      hostVoice.file_count >= expectedHostCount &&
      guestVoice.file_count >= expectedGuestCount
    ) {
      const hostName = hostVoice.voice_name || hostVoice.voice_id.toString()
      const guestName = guestVoice.voice_name || guestVoice.voice_id.toString()
      return {
        hostVoiceId: hostVoice.voice_id,
        guestVoiceId: guestVoice.voice_id,
        voiceName:
          hostName === guestName ? hostName : `${hostName} + ${guestName}`,
      }
    }
  }

  return null
}

export interface UsePodcastReturn {
  podcastDialogues: PodcastDialogue[]
  podcastLoading: boolean
  podcastError: string | null
  showPodcastPlayer: boolean
  podcastState: 'stopped' | 'playing' | 'paused'
  podcastCurrentIndex: number
  podcastLanguage: string

  ttsEngine: TTSEngine
  cloudTtsAvailable: boolean | null
  cloudTtsError: string | null
  cloudTtsLoading: boolean
  cloudTtsLoadProgress: { loaded: number; total: number }

  voiceList: VoiceInfo[]
  showVoiceSettings: boolean
  hostVoiceId: number | undefined
  guestVoiceId: number | undefined

  articleCache: ArticleCacheResponse | null
  articleCacheLoading: boolean
  clearingVoiceId: number | null

  groupedVoices: {
    ultra: VoiceInfo[]
    llm: VoiceInfo[]
    premium: VoiceInfo[]
    ultraMale: VoiceInfo[]
    ultraFemale: VoiceInfo[]
    llmMale: VoiceInfo[]
    llmFemale: VoiceInfo[]
    premiumMale: VoiceInfo[]
    premiumFemale: VoiceInfo[]
  }
  voiceNameById: Map<number, string>

  setShowPodcastPlayer: (show: boolean) => void
  setShowVoiceSettings: (show: boolean) => void
  loadPodcast: () => Promise<void>
  regeneratePodcast: () => Promise<void>
  handleTtsEngineChange: (engine: TTSEngine) => Promise<void>
  handleVoiceChange: (role: 'host' | 'guest', voiceId: number) => void
  handleOpenSettings: () => void
  handleSwitchToVoice: (voiceId: number, role: string) => Promise<void>
  handleClearVoiceCache: (voiceId: number) => Promise<void>
  reloadCloudTTS: () => Promise<void>

  handlePodcastPlay: () => Promise<void>
  handlePodcastPause: () => void
  handlePodcastStop: () => void
  handlePodcastPrev: () => Promise<void>
  handlePodcastNext: () => Promise<void>
  handlePodcastSeek: (index: number) => Promise<void>

  podcastListRef: React.RefObject<HTMLDivElement | null>
}

export function usePodcast({
  itemId,
  sourceId,
  isBrewlia,
  showToastMessage,
  t,
}: UsePodcastOptions): UsePodcastReturn {
  const captureTask = useArticleTaskScope(itemId)
  const turns = useRef(new RequestTurn())
  useEffect(() => () => turns.current.cancel(), [itemId])
  const [podcastDialogues, setPodcastDialogues] = useState<PodcastDialogue[]>(
    [],
  )
  const [podcastLoading, setPodcastLoading] = useState(false)
  const [podcastError, setPodcastError] = useState<string | null>(null)
  const [showPodcastPlayer, setShowPodcastPlayer] = useState(false)
  const [podcastState, setPodcastState] = useState<
    'stopped' | 'playing' | 'paused'
  >('stopped')
  const [podcastCurrentIndex, setPodcastCurrentIndex] = useState(0)
  const [podcastLanguage, setPodcastLanguage] = useState<string>(() =>
    getDefaultLocale(),
  )

  const [ttsEngine, setTtsEngine] = useState<TTSEngine>(
    () => getTTSSettings().engine,
  )
  const [cloudTtsAvailable, setCloudTtsAvailable] = useState<boolean | null>(
    null,
  )
  const [cloudTtsError, setCloudTtsError] = useState<string | null>(null)
  const [cloudTtsLoading, setCloudTtsLoading] = useState(false)
  const [cloudTtsLoadProgress, setCloudTtsLoadProgress] = useState({
    loaded: 0,
    total: 0,
  })

  const [voiceList, setVoiceList] = useState<VoiceInfo[]>([])
  const [showVoiceSettings, setShowVoiceSettings] = useState(false)
  const [hostVoiceId, setHostVoiceId] = useState<number | undefined>(
    () => getTTSSettings().hostVoiceId,
  )
  const [guestVoiceId, setGuestVoiceId] = useState<number | undefined>(
    () => getTTSSettings().guestVoiceId,
  )

  const [articleCache, setArticleCache] = useState<ArticleCacheResponse | null>(
    null,
  )
  const [articleCacheLoading, setArticleCacheLoading] = useState(false)
  const [clearingVoiceId, setClearingVoiceId] = useState<number | null>(null)

  const podcastPlayerRef = useRef<PodcastPlayer | null>(null)
  const cloudPodcastPlayerRef = useRef<CloudPodcastPlayer | null>(null)
  const podcastListRef = useRef<HTMLDivElement>(null)

  const voiceNameById = useMemo(() => {
    const map = new Map<number, string>()
    voiceList.forEach((v) => map.set(v.id, v.name))
    return map
  }, [voiceList])

  const groupedVoices = useMemo(() => {
    const byBucket = Object.groupBy(voiceList, (voice) =>
      voice.voice_type === 'ultra_natural'
        ? 'ultra'
        : voice.voice_type === 'llm'
          ? 'llm'
          : 'premium',
    )
    const ultra = byBucket.ultra ?? []
    const llm = byBucket.llm ?? []
    const premium = byBucket.premium ?? []

    return {
      ultra,
      llm,
      premium,
      ultraMale: ultra.filter((v) => isMaleVoice(v.gender)),
      ultraFemale: ultra.filter((v) => isFemaleVoice(v.gender)),
      llmMale: llm.filter((v) => isMaleVoice(v.gender)),
      llmFemale: llm.filter((v) => isFemaleVoice(v.gender)),
      premiumMale: premium.filter((v) => isMaleVoice(v.gender)),
      premiumFemale: premium.filter((v) => isFemaleVoice(v.gender)),
    }
  }, [voiceList])

  useEffect(() => {
    if (!isBrewlia) return

    getSpeechStatus()
      .then((status) => {
        setCloudTtsAvailable(status.available && status.tts_enabled)
        if (!status.available || !status.tts_enabled) {
          setCloudTtsError(status.error || t.brew.cloudTtsUnavailableError)
        }
      })
      .catch((err) => {
        console.error('[TTS] Failed to get speech status:', err)
        setCloudTtsAvailable(false)
        setCloudTtsError(userFacingError(err, t.brew.cannotConnectVoiceService))
      })

    getVoiceList()
      .then((response) => {
        setVoiceList(response.voices)
      })
      .catch((err) => {
        console.error('[TTS] Failed to get voice list:', err)
      })
  }, [isBrewlia, t])

  useEffect(() => {
    if (!isBrewlia) return

    const player = new PodcastPlayer()
    player.setCallbacks({
      onProgress: (index, _total) => {
        setPodcastCurrentIndex(index)
      },
      onEnd: () => {
        setPodcastState('stopped')
        setPodcastCurrentIndex(0)
      },
      onStateChange: (state) => {
        setPodcastState(state)
      },
    })
    podcastPlayerRef.current = player

    return () => {
      player.destroy()
      podcastPlayerRef.current = null
    }
  }, [isBrewlia])

  useEffect(() => {
    if (!isBrewlia) return

    const player = new CloudPodcastPlayer()
    player.setOnProgress((index, _total) => {
      setPodcastCurrentIndex(index)
    })
    player.setOnEnd(() => {
      setPodcastState('stopped')
      setPodcastCurrentIndex(0)
    })
    player.setOnLoadProgress((loaded, total) => {
      setCloudTtsLoadProgress({ loaded, total })
    })
    cloudPodcastPlayerRef.current = player

    return () => {
      player.destroy()
      cloudPodcastPlayerRef.current = null
    }
  }, [isBrewlia])

  const getActivePlayer = useCallback(() => {
    if (
      ttsEngine === 'cloud' &&
      cloudTtsAvailable &&
      cloudPodcastPlayerRef.current
    ) {
      return cloudPodcastPlayerRef.current
    }
    return podcastPlayerRef.current
  }, [ttsEngine, cloudTtsAvailable])

  const handleTtsEngineChange = useCallback(
    async (engine: TTSEngine) => {
      const isCurrent = captureTask()
      if (!isCurrent()) return

      if (engine === ttsEngine) return

      if (podcastState !== 'stopped') {
        podcastPlayerRef.current?.stop()
        cloudPodcastPlayerRef.current?.stop()
        setPodcastState('stopped')
        setPodcastCurrentIndex(0)
      }

      setTtsEngine(engine)
      saveTTSSettings({ engine })

      if (podcastDialogues.length > 0) {
        if (engine === 'cloud') {
          if (cloudTtsAvailable === null) {
            showToastMessage(t.brew.checkingCloudTts)
            try {
              const status = await getSpeechStatus()
              if (!isCurrent()) return

              if (!status.available || !status.tts_enabled) {
                setCloudTtsAvailable(false)
                setCloudTtsError(status.error || t.brew.cloudTtsUnavailable)
                showToastMessage(
                  status.error || t.brew.cloudTtsUnavailableCheck,
                  5000,
                )
                setTtsEngine('system')
                saveTTSSettings({ engine: 'system' })
                return
              }
              setCloudTtsAvailable(true)
            } catch (err) {
              if (!isCurrent()) return

              const errMsg = userFacingError(err, t.brew.cannotConnectSpeech)
              setCloudTtsAvailable(false)
              setCloudTtsError(errMsg)
              showToastMessage(`${t.brew.cloudTtsUnavailable}: ${errMsg}`, 5000)
              setTtsEngine('system')
              saveTTSSettings({ engine: 'system' })
              return
            }
          } else if (!cloudTtsAvailable) {
            showToastMessage(
              cloudTtsError || t.brew.cloudTtsUnavailableCheck,
              5000,
            )
            setTtsEngine('system')
            saveTTSSettings({ engine: 'system' })
            return
          }

          if (cloudPodcastPlayerRef.current?.hasAudio()) {
            showToastMessage(t.brew.switchedToCloudTts)
          } else {
            showToastMessage(t.brew.checkingCloudCache)
            try {
              const cache = await getArticleCacheInfo(sourceId, itemId)
              if (!isCurrent()) return

              const completeCache = findCompleteCacheVoices(
                cache,
                podcastDialogues.length,
              )

              if (completeCache) {
                setHostVoiceId(completeCache.hostVoiceId)
                setGuestVoiceId(completeCache.guestVoiceId)
                saveTTSSettings({
                  hostVoiceId: completeCache.hostVoiceId,
                  guestVoiceId: completeCache.guestVoiceId,
                })
                showToastMessage(
                  `${t.brew.switchedToCloudTts}（${t.brew.cached}: ${completeCache.voiceName}）`,
                )
              } else {
                setTtsEngine('system')
                saveTTSSettings({ engine: 'system' })
                if (podcastPlayerRef.current) {
                  podcastPlayerRef.current.load(
                    podcastDialogues,
                    podcastLanguage,
                  )
                  const voices = await PodcastPlayer.getAvailableVoices()
                  if (!isCurrent()) return

                  const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
                    voices,
                    podcastLanguage,
                  )
                  if (voiceA && voiceB) {
                    podcastPlayerRef.current.setVoices(voiceA, voiceB)
                  }
                }
                showToastMessage(
                  cache.voices.length > 0
                    ? t.brew.cloudCacheIncomplete
                    : t.brew.noCloudCache,
                )
              }
            } catch (err) {
              if (!isCurrent()) return

              console.error('[TTS] Failed to check cache:', err)
              setTtsEngine('system')
              saveTTSSettings({ engine: 'system' })
              if (podcastPlayerRef.current) {
                podcastPlayerRef.current.load(podcastDialogues, podcastLanguage)
                const voices = await PodcastPlayer.getAvailableVoices()
                if (!isCurrent()) return

                const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
                  voices,
                  podcastLanguage,
                )
                if (voiceA && voiceB) {
                  podcastPlayerRef.current.setVoices(voiceA, voiceB)
                }
              }
              showToastMessage(t.brew.checkCacheFailed)
            }
          }
        } else {
          if (podcastPlayerRef.current) {
            podcastPlayerRef.current.load(podcastDialogues, podcastLanguage)
            const voices = await PodcastPlayer.getAvailableVoices()
            if (!isCurrent()) return

            const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
              voices,
              podcastLanguage,
            )
            if (voiceA && voiceB) {
              podcastPlayerRef.current.setVoices(voiceA, voiceB)
            }
          }
          showToastMessage(t.brew.switchedToSystemTts)
        }
      } else {
        showToastMessage(
          engine === 'cloud'
            ? t.brew.switchedToCloudTts
            : t.brew.switchedToSystemTts,
        )
      }
    },
    [
      podcastState,
      ttsEngine,
      podcastDialogues,
      podcastLanguage,
      cloudTtsAvailable,
      cloudTtsError,
      sourceId,
      itemId,
      showToastMessage,
      t,
    ],
  )

  const handleVoiceChange = useCallback(
    (role: 'host' | 'guest', voiceId: number) => {
      const newVoiceId = voiceId === 0 ? undefined : voiceId

      if (role === 'host') {
        setHostVoiceId(newVoiceId)
        saveTTSSettings({ hostVoiceId: newVoiceId })
      } else {
        setGuestVoiceId(newVoiceId)
        saveTTSSettings({ guestVoiceId: newVoiceId })
      }

      showToastMessage(t.brew.voiceSettingSaved)
    },
    [showToastMessage, t],
  )

  const loadArticleCache = useCallback(async () => {
    const isCurrent = captureTask()
    if (!isCurrent()) return

    if (articleCacheLoading) return
    setArticleCacheLoading(true)
    try {
      const cache = await getArticleCacheInfo(sourceId, itemId)
      if (!isCurrent()) return

      setArticleCache(cache)
    } catch (error) {
      if (!isCurrent()) return

      console.error('[ArticleCache] Failed to load:', error)
    } finally {
      if (isCurrent()) {
        setArticleCacheLoading(false)
      }
    }
  }, [sourceId, itemId, articleCacheLoading])

  const handleOpenSettings = useCallback(() => {
    setShowVoiceSettings(!showVoiceSettings)
    if (!showVoiceSettings && ttsEngine === 'cloud') {
      loadArticleCache()
    }
  }, [showVoiceSettings, ttsEngine, loadArticleCache])

  const handleSwitchToVoice = useCallback(
    async (voiceId: number, role: string) => {
      const isCurrent = captureTask()
      if (!isCurrent()) return

      if (role === 'host') {
        setHostVoiceId(voiceId)
        saveTTSSettings({ hostVoiceId: voiceId })
      } else if (role === 'guest') {
        setGuestVoiceId(voiceId)
        saveTTSSettings({ guestVoiceId: voiceId })
      }

      setShowVoiceSettings(false)

      const voiceCache = articleCache?.voices.find(
        (v) => v.voice_id === voiceId,
      )
      const voiceName = voiceCache?.voice_name || voiceId.toString()

      if (!voiceCache || voiceCache.file_count < podcastDialogues.length) {
        showToastMessage(`${voiceName} ${t.brew.reload}`)
        return
      }

      if (podcastDialogues.length > 0 && cloudPodcastPlayerRef.current) {
        cloudPodcastPlayerRef.current.stop()
        setPodcastState('stopped')
        setPodcastCurrentIndex(0)

        setCloudTtsLoading(true)
        showToastMessage(`${voiceName}...`)

        try {
          const newHostVoiceId = role === 'host' ? voiceId : hostVoiceId
          const newGuestVoiceId = role === 'guest' ? voiceId : guestVoiceId

          const result = await cloudPodcastPlayerRef.current.load(
            podcastDialogues,
            {
              sourceId,
              articleId: itemId,
              hostVoiceId: newHostVoiceId,
              guestVoiceId: newGuestVoiceId,
            },
          )
          if (!isCurrent()) return

          if (result) {
            if (result.cacheHits === result.total) {
              showToastMessage(`${voiceName}（${t.brew.cached}）`)
            } else {
              showToastMessage(
                `${voiceName} (${result.cacheHits}/${result.generated})`,
              )
            }
          }
        } catch (err) {
          if (!isCurrent()) return

          console.error('Failed to switch voice:', err)
          showToastMessage(t.brew.switchFailed)
        } finally {
          if (isCurrent()) {
            setCloudTtsLoading(false)
          }
        }
      } else {
        showToastMessage(`${voiceName}`)
      }
    },
    [
      podcastDialogues,
      sourceId,
      itemId,
      hostVoiceId,
      guestVoiceId,
      articleCache,
      showToastMessage,
      t,
    ],
  )

  const handleClearVoiceCache = useCallback(
    async (voiceId: number) => {
      const isCurrent = captureTask()
      if (!isCurrent()) return

      if (clearingVoiceId !== null) return
      setClearingVoiceId(voiceId)
      try {
        const result = await clearArticleVoiceCache(sourceId, itemId, voiceId)
        if (!isCurrent()) return

        if (result.success) {
          const voiceName =
            articleCache?.voices.find((v) => v.voice_id === voiceId)
              ?.voice_name || voiceId.toString()
          showToastMessage(`${voiceName}`)
          await loadArticleCache()
        }
      } catch (error) {
        if (!isCurrent()) return

        console.error('[ArticleCache] Failed to clear voice cache:', error)
        showToastMessage(t.brew.clearFailed)
      } finally {
        if (isCurrent()) {
          setClearingVoiceId(null)
        }
      }
    },
    [
      sourceId,
      itemId,
      clearingVoiceId,
      articleCache,
      loadArticleCache,
      showToastMessage,
      t,
    ],
  )

  const loadPodcast = useCallback(async () => {
    const isCurrent = captureTask()
    if (!isCurrent()) return

    if (!isBrewlia || podcastLoading || cloudTtsLoading) return

    const signal = turns.current.begin()
    setPodcastLoading(true)
    setPodcastError(null)

    try {
      const response = await brewliaApi.getPodcastScript(itemId, signal)
      if (!isCurrent() || signal.aborted) return

      if (response.success) {
        setPodcastDialogues(response.dialogues)
        setPodcastLanguage(response.language || getDefaultLocale())
        setShowPodcastPlayer(true)

        if (cloudTtsAvailable) {
          try {
            const cache = await getArticleCacheInfo(sourceId, itemId)
            if (!isCurrent()) return

            const completeCache = findCompleteCacheVoices(
              cache,
              response.dialogues.length,
            )

            if (completeCache) {
              setTtsEngine('cloud')
              saveTTSSettings({ engine: 'cloud' })

              setCloudTtsLoading(true)
              setCloudTtsLoadProgress({
                loaded: 0,
                total: response.dialogues.length,
              })
              showToastMessage(t.brew.loadingCloudCache)

              setHostVoiceId(completeCache.hostVoiceId)
              setGuestVoiceId(completeCache.guestVoiceId)
              saveTTSSettings({
                hostVoiceId: completeCache.hostVoiceId,
                guestVoiceId: completeCache.guestVoiceId,
              })

              const result = await cloudPodcastPlayerRef.current?.load(
                response.dialogues,
                {
                  sourceId,
                  articleId: itemId,
                  hostVoiceId: completeCache.hostVoiceId,
                  guestVoiceId: completeCache.guestVoiceId,
                },
              )
              if (!isCurrent()) return

              if (result) {
                showToastMessage(`${t.brew.cached}: ${completeCache.voiceName}`)
              }
              setCloudTtsLoading(false)
            } else {
              setTtsEngine('system')
              saveTTSSettings({ engine: 'system' })

              if (podcastPlayerRef.current) {
                podcastPlayerRef.current.load(
                  response.dialogues,
                  response.language,
                )
                const voices = await PodcastPlayer.getAvailableVoices()
                if (!isCurrent()) return

                const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
                  voices,
                  response.language || getDefaultLocale(),
                )
                if (voiceA && voiceB) {
                  podcastPlayerRef.current.setVoices(voiceA, voiceB)
                }
              }
              showToastMessage(
                cache.voices.length > 0
                  ? t.brew.cloudCacheIncomplete
                  : `${response.dialogues.length}`,
              )
            }
          } catch (cacheErr) {
            if (!isCurrent()) return

            console.error(
              '[TTS] Failed to check cache in loadPodcast:',
              cacheErr,
            )
            setTtsEngine('system')
            saveTTSSettings({ engine: 'system' })

            if (podcastPlayerRef.current) {
              podcastPlayerRef.current.load(
                response.dialogues,
                response.language,
              )
              const voices = await PodcastPlayer.getAvailableVoices()
              if (!isCurrent()) return

              const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
                voices,
                response.language || getDefaultLocale(),
              )
              if (voiceA && voiceB) {
                podcastPlayerRef.current.setVoices(voiceA, voiceB)
              }
            }
            showToastMessage(`${response.dialogues.length}`)
          }
        } else {
          setTtsEngine('system')
          if (podcastPlayerRef.current) {
            podcastPlayerRef.current.load(response.dialogues, response.language)
            const voices = await PodcastPlayer.getAvailableVoices()
            if (!isCurrent()) return

            const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
              voices,
              response.language || getDefaultLocale(),
            )
            if (voiceA && voiceB) {
              podcastPlayerRef.current.setVoices(voiceA, voiceB)
            }
          }
          showToastMessage(`${response.dialogues.length}`, 3000)
        }
      } else {
        setPodcastError(response.error || t.brew.generateFailed)
      }
    } catch (err) {
      if (!isCurrent() || signal.aborted) return

      console.error('Failed to load podcast:', err)
      setPodcastError(userFacingError(err, t.brew.generatePodcastFailed))
    } finally {
      if (isCurrent() && !signal.aborted) {
        setPodcastLoading(false)
      }
    }
  }, [
    isBrewlia,
    podcastLoading,
    cloudTtsLoading,
    itemId,
    sourceId,
    cloudTtsAvailable,
    showToastMessage,
    t,
  ])

  const regeneratePodcast = useCallback(async () => {
    const isCurrent = captureTask()
    if (!isCurrent()) return

    if (!isBrewlia || podcastLoading || cloudTtsLoading) return

    const signal = turns.current.begin()
    podcastPlayerRef.current?.stop()
    cloudPodcastPlayerRef.current?.stop()
    setPodcastState('stopped')
    setPodcastCurrentIndex(0)

    setPodcastLoading(true)
    setPodcastError(null)
    showToastMessage(t.brew.regeneratingScript)

    try {
      const response = await brewliaApi.regeneratePodcastScript(itemId, signal)
      if (!isCurrent() || signal.aborted) return

      if (response.success) {
        setPodcastDialogues(response.dialogues)
        setPodcastLanguage(response.language || getDefaultLocale())
        setShowPodcastPlayer(true)

        setTtsEngine('system')
        saveTTSSettings({ engine: 'system' })

        if (podcastPlayerRef.current) {
          podcastPlayerRef.current.load(response.dialogues, response.language)
          const voices = await PodcastPlayer.getAvailableVoices()
          if (!isCurrent()) return

          const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
            voices,
            response.language || getDefaultLocale(),
          )
          if (voiceA && voiceB) {
            podcastPlayerRef.current.setVoices(voiceA, voiceB)
          }
        }
        showToastMessage(t.brew.switchedToSystemTts, 3000)
      } else {
        setPodcastError(response.error || t.brew.regenerateFailed)
        showToastMessage(response.error || t.brew.regenerateFailed, 3000)
      }
    } catch (err) {
      if (!isCurrent() || signal.aborted) return

      console.error('Failed to regenerate podcast:', err)
      const errMsg = userFacingError(err, t.brew.generatePodcastFailed)
      setPodcastError(errMsg)
      showToastMessage(errMsg, 3000)
    } finally {
      if (isCurrent() && !signal.aborted) {
        setPodcastLoading(false)
      }
    }
  }, [isBrewlia, podcastLoading, cloudTtsLoading, itemId, showToastMessage, t])

  const reloadCloudTTS = useCallback(async () => {
    const isCurrent = captureTask()
    if (!isCurrent()) return

    if (!podcastDialogues.length || !cloudTtsAvailable || cloudTtsLoading)
      return

    cloudPodcastPlayerRef.current?.stop()
    setPodcastState('stopped')
    setPodcastCurrentIndex(0)

    setCloudTtsLoading(true)
    setCloudTtsLoadProgress({ loaded: 0, total: podcastDialogues.length })
    showToastMessage(t.brew.regeneratingCloudVoice)

    try {
      const result = await cloudPodcastPlayerRef.current?.load(
        podcastDialogues,
        {
          sourceId,
          articleId: itemId,
          hostVoiceId,
          guestVoiceId,
          forceRegenerate: true,
        },
      )
      if (!isCurrent()) return

      if (result) {
        if (result.generated > 0) {
          showToastMessage(`${result.generated}`)
        } else {
          showToastMessage(t.brew.cached)
        }
      }
    } catch (err) {
      if (!isCurrent()) return

      const errMsg = userFacingError(err, t.brew.loadFailed)
      console.error('Failed to reload cloud TTS:', errMsg, err)
      showToastMessage(`${t.brew.cloudTtsUnavailable}: ${errMsg}`, 3000)
    } finally {
      if (isCurrent()) {
        setCloudTtsLoading(false)
      }
    }
  }, [
    podcastDialogues,
    cloudTtsAvailable,
    cloudTtsLoading,
    sourceId,
    itemId,
    hostVoiceId,
    guestVoiceId,
    showToastMessage,
    t,
  ])

  const handlePodcastPlay = useCallback(async () => {
    const isCurrent = captureTask()
    if (!isCurrent()) return

    const player = getActivePlayer()
    if (player && 'play' in player && typeof player.play === 'function') {
      await player.play()
      if (!isCurrent()) return

      setPodcastState('playing')
    }
  }, [getActivePlayer])

  const handlePodcastPause = useCallback(() => {
    const player = getActivePlayer()
    if (player) {
      player.pause()
      setPodcastState('paused')
    }
  }, [getActivePlayer])

  const handlePodcastStop = useCallback(() => {
    const player = getActivePlayer()
    if (player) {
      player.stop()
      setPodcastState('stopped')
      setPodcastCurrentIndex(0)
    }
  }, [getActivePlayer])

  const handlePodcastPrev = useCallback(async () => {
    const isCurrent = captureTask()
    if (!isCurrent()) return

    if (podcastCurrentIndex <= 0) return
    const newIndex = podcastCurrentIndex - 1
    const player = getActivePlayer()
    if (player) {
      if (ttsEngine === 'cloud') {
        await player.seekTo(newIndex)
        if (!isCurrent()) return

        if (podcastState === 'playing') {
          await (player as CloudPodcastPlayer).play()
        }
      } else {
        ;(player as PodcastPlayer).seekTo(newIndex, podcastState === 'playing')
      }
    }
  }, [podcastCurrentIndex, podcastState, getActivePlayer, ttsEngine])

  const handlePodcastNext = useCallback(async () => {
    const isCurrent = captureTask()
    if (!isCurrent()) return

    if (podcastCurrentIndex >= podcastDialogues.length - 1) return
    const newIndex = podcastCurrentIndex + 1
    const player = getActivePlayer()
    if (player) {
      if (ttsEngine === 'cloud') {
        await player.seekTo(newIndex)
        if (!isCurrent()) return

        if (podcastState === 'playing') {
          await (player as CloudPodcastPlayer).play()
        }
      } else {
        ;(player as PodcastPlayer).seekTo(newIndex, podcastState === 'playing')
      }
    }
  }, [
    podcastCurrentIndex,
    podcastDialogues.length,
    podcastState,
    getActivePlayer,
    ttsEngine,
  ])

  const handlePodcastSeek = useCallback(
    async (index: number) => {
      const isCurrent = captureTask()
      if (!isCurrent()) return

      const player = getActivePlayer()
      if (player) {
        if (ttsEngine === 'cloud') {
          await player.seekTo(index)
          if (!isCurrent()) return

          await (player as CloudPodcastPlayer).play()
          if (!isCurrent()) return

          setPodcastState('playing')
        } else {
          ;(player as PodcastPlayer).seekTo(index, true)
        }
      }
    },
    [getActivePlayer, ttsEngine],
  )

  useEffect(() => {
    if (!showPodcastPlayer || podcastDialogues.length === 0) return

    const container = podcastListRef.current
    if (!container) return

    const timer = setTimeout(() => {
      const currentElement = container.querySelector(
        `[data-podcast-index="${podcastCurrentIndex}"]`,
      ) as HTMLElement
      if (currentElement) {
        const containerRect = container.getBoundingClientRect()
        const elementRect = currentElement.getBoundingClientRect()

        const isAbove = elementRect.top < containerRect.top
        const isBelow = elementRect.bottom > containerRect.bottom

        if (isAbove || isBelow) {
          const scrollTop =
            container.scrollTop +
            elementRect.top -
            containerRect.top -
            containerRect.height / 2 +
            elementRect.height / 2
          container.scrollTo({
            top: scrollTop,
            behavior: 'smooth',
          })
        }
      }
    }, 50)

    return () => clearTimeout(timer)
  }, [podcastCurrentIndex, showPodcastPlayer, podcastDialogues.length])

  return {
    podcastDialogues,
    podcastLoading,
    podcastError,
    showPodcastPlayer,
    podcastState,
    podcastCurrentIndex,
    podcastLanguage,

    ttsEngine,
    cloudTtsAvailable,
    cloudTtsError,
    cloudTtsLoading,
    cloudTtsLoadProgress,

    voiceList,
    showVoiceSettings,
    hostVoiceId,
    guestVoiceId,

    articleCache,
    articleCacheLoading,
    clearingVoiceId,

    groupedVoices,
    voiceNameById,

    setShowPodcastPlayer,
    setShowVoiceSettings,
    loadPodcast,
    regeneratePodcast,
    handleTtsEngineChange,
    handleVoiceChange,
    handleOpenSettings,
    handleSwitchToVoice,
    handleClearVoiceCache,
    reloadCloudTTS,

    handlePodcastPlay,
    handlePodcastPause,
    handlePodcastStop,
    handlePodcastPrev,
    handlePodcastNext,
    handlePodcastSeek,

    podcastListRef,
  }
}
