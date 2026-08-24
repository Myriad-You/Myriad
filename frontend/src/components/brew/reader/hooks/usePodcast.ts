/**
 * AI 播客 Hook
 * 管理 Brewlia AI 播客的加载、播放控制、TTS 引擎切换等功能
 */

import type { PodcastDialogue } from '../../../../services/brewliaApi'
import type {
  ArticleCacheResponse,
  TTSEngine,
  VoiceInfo,
} from '../../../../services/speechApi'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { userFacingError } from '../../../../utils/userFacingError'
import * as brewliaApi from '../../../../services/brewliaApi'
import { PodcastPlayer } from '../../../../services/brewliaApi'
import {
  clearArticleVoiceCache,
  CloudPodcastPlayer,
  getArticleCacheInfo,
  getSpeechStatus,
  getTTSSettings,
  getVoiceList,
  saveTTSSettings,
} from '../../../../services/speechApi'

export interface UsePodcastOptions {
  itemId: number
  sourceId: number
  isBrewlia: boolean
  showToastMessage: (message: string, duration?: number) => void
  t: Record<string, any>
}

/**
 * 检测完整的云端缓存
 * 支持两种场景:
 * 1. 单音色缓存: 一个音色包含所有对话索引
 * 2. 双音色缓存: host音色包含偶数索引, guest音色包含奇数索引
 */
function findCompleteCacheVoices(
  cache: ArticleCacheResponse,
  dialogueCount: number,
): { hostVoiceId: number; guestVoiceId: number; voiceName: string } | null {
  if (!cache.voices.length || dialogueCount <= 0) return null

  // 场景1: 单音色包含所有对话
  const singleVoice = cache.voices.find((v) => v.file_count >= dialogueCount)
  if (singleVoice) {
    return {
      hostVoiceId: singleVoice.voice_id,
      guestVoiceId: singleVoice.voice_id,
      voiceName: singleVoice.voice_name || singleVoice.voice_id.toString(),
    }
  }

  // 场景2: 双音色 (host = 偶数索引, guest = 奇数索引)
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
  // 播客状态
  podcastDialogues: PodcastDialogue[]
  podcastLoading: boolean
  podcastError: string | null
  showPodcastPlayer: boolean
  podcastState: 'stopped' | 'playing' | 'paused'
  podcastCurrentIndex: number
  podcastLanguage: string

  // TTS 引擎状态
  ttsEngine: TTSEngine
  cloudTtsAvailable: boolean | null
  cloudTtsError: string | null
  cloudTtsLoading: boolean
  cloudTtsLoadProgress: { loaded: number; total: number }

  // TTS 音色状态
  voiceList: VoiceInfo[]
  showVoiceSettings: boolean
  hostVoiceId: number | undefined
  guestVoiceId: number | undefined

  // 文章缓存状态
  articleCache: ArticleCacheResponse | null
  articleCacheLoading: boolean
  clearingVoiceId: number | null

  // 分组音色（优化渲染性能）
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

  // 操作
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

  // 播放控制
  handlePodcastPlay: () => Promise<void>
  handlePodcastPause: () => void
  handlePodcastStop: () => void
  handlePodcastPrev: () => Promise<void>
  handlePodcastNext: () => Promise<void>
  handlePodcastSeek: (index: number) => Promise<void>

  // Refs
  podcastListRef: React.RefObject<HTMLDivElement | null>
}

export function usePodcast({
  itemId,
  sourceId,
  isBrewlia,
  showToastMessage,
  t,
}: UsePodcastOptions): UsePodcastReturn {
  // 播客状态
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
  const [podcastLanguage, setPodcastLanguage] = useState<string>('zh-CN')

  // TTS 引擎状态
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

  // TTS 音色状态
  const [voiceList, setVoiceList] = useState<VoiceInfo[]>([])
  const [showVoiceSettings, setShowVoiceSettings] = useState(false)
  const [hostVoiceId, setHostVoiceId] = useState<number | undefined>(
    () => getTTSSettings().hostVoiceId,
  )
  const [guestVoiceId, setGuestVoiceId] = useState<number | undefined>(
    () => getTTSSettings().guestVoiceId,
  )

  // 文章缓存状态
  const [articleCache, setArticleCache] = useState<ArticleCacheResponse | null>(
    null,
  )
  const [articleCacheLoading, setArticleCacheLoading] = useState(false)
  const [clearingVoiceId, setClearingVoiceId] = useState<number | null>(null)

  // Refs
  const podcastPlayerRef = useRef<PodcastPlayer | null>(null)
  const cloudPodcastPlayerRef = useRef<CloudPodcastPlayer | null>(null)
  const podcastListRef = useRef<HTMLDivElement>(null)

  // 音色 ID -> 名称映射
  const voiceNameById = useMemo(() => {
    const map = new Map<number, string>()
    voiceList.forEach((v) => map.set(v.id, v.name))
    return map
  }, [voiceList])

  // 分组音色列表
  const groupedVoices = useMemo(() => {
    const ultra: VoiceInfo[] = []
    const llm: VoiceInfo[] = []
    const premium: VoiceInfo[] = []

    voiceList.forEach((voice) => {
      if (voice.voice_type === 'ultra_natural') {
        ultra.push(voice)
      } else if (voice.voice_type === 'llm') {
        llm.push(voice)
      } else {
        premium.push(voice)
      }
    })

    const isMale = (v: VoiceInfo) => v.gender === '男' || v.gender === '男童'
    const isFemale = (v: VoiceInfo) => v.gender === '女' || v.gender === '女童'

    return {
      ultra,
      llm,
      premium,
      ultraMale: ultra.filter(isMale),
      ultraFemale: ultra.filter(isFemale),
      llmMale: llm.filter(isMale),
      llmFemale: llm.filter(isFemale),
      premiumMale: premium.filter((v) => v.gender === '男'),
      premiumFemale: premium.filter((v) => v.gender === '女'),
    }
  }, [voiceList])

  // 检测云端 TTS 是否可用 & 获取音色列表
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
        setCloudTtsError(
          userFacingError(err, t.brew.cannotConnectVoiceService),
        )
      })

    getVoiceList()
      .then((response) => {
        setVoiceList(response.voices)
      })
      .catch((err) => {
        console.error('[TTS] Failed to get voice list:', err)
      })
  }, [isBrewlia, t])

  // 初始化系统播客播放器
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

  // 初始化云端播客播放器
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

  // 获取当前活动的播放器
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

  // 切换 TTS 引擎
  const handleTtsEngineChange = useCallback(
    async (engine: TTSEngine) => {
      if (engine === ttsEngine) return

      // 停止当前播放
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
          // 检查云端TTS状态
          if (cloudTtsAvailable === null) {
            showToastMessage(t.brew.checkingCloudTts)
            try {
              const status = await getSpeechStatus()
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

          // 检查云端缓存
          if (cloudPodcastPlayerRef.current?.hasAudio()) {
            showToastMessage(t.brew.switchedToCloudTts)
          } else {
            showToastMessage(t.brew.checkingCloudCache)
            try {
              const cache = await getArticleCacheInfo(sourceId, itemId)
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
                // 无完整缓存，回退
                setTtsEngine('system')
                saveTTSSettings({ engine: 'system' })
                if (podcastPlayerRef.current) {
                  podcastPlayerRef.current.load(
                    podcastDialogues,
                    podcastLanguage,
                  )
                  const voices = await PodcastPlayer.getAvailableVoices()
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
              console.error('[TTS] Failed to check cache:', err)
              setTtsEngine('system')
              saveTTSSettings({ engine: 'system' })
              if (podcastPlayerRef.current) {
                podcastPlayerRef.current.load(podcastDialogues, podcastLanguage)
                const voices = await PodcastPlayer.getAvailableVoices()
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
          // 切换到系统 TTS
          if (podcastPlayerRef.current) {
            podcastPlayerRef.current.load(podcastDialogues, podcastLanguage)
            const voices = await PodcastPlayer.getAvailableVoices()
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

  // 音色选择处理
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

  // 加载文章缓存信息
  const loadArticleCache = useCallback(async () => {
    if (articleCacheLoading) return
    setArticleCacheLoading(true)
    try {
      const cache = await getArticleCacheInfo(sourceId, itemId)
      setArticleCache(cache)
    } catch (error) {
      console.error('[ArticleCache] Failed to load:', error)
    } finally {
      setArticleCacheLoading(false)
    }
  }, [sourceId, itemId, articleCacheLoading])

  // 打开设置面板
  const handleOpenSettings = useCallback(() => {
    setShowVoiceSettings(!showVoiceSettings)
    if (!showVoiceSettings && ttsEngine === 'cloud') {
      loadArticleCache()
    }
  }, [showVoiceSettings, ttsEngine, loadArticleCache])

  // 切换到已缓存的音色
  const handleSwitchToVoice = useCallback(
    async (voiceId: number, role: string) => {
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

      // 缓存完整，加载
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
          console.error('Failed to switch voice:', err)
          showToastMessage(t.brew.switchFailed)
        } finally {
          setCloudTtsLoading(false)
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

  // 清除特定音色缓存
  const handleClearVoiceCache = useCallback(
    async (voiceId: number) => {
      if (clearingVoiceId !== null) return
      setClearingVoiceId(voiceId)
      try {
        const result = await clearArticleVoiceCache(sourceId, itemId, voiceId)
        if (result.success) {
          const voiceName =
            articleCache?.voices.find((v) => v.voice_id === voiceId)
              ?.voice_name || voiceId.toString()
          showToastMessage(`${voiceName}`)
          await loadArticleCache()
        }
      } catch (error) {
        console.error('[ArticleCache] Failed to clear voice cache:', error)
        showToastMessage(t.brew.clearFailed)
      } finally {
        setClearingVoiceId(null)
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

  // 加载播客脚本
  const loadPodcast = useCallback(async () => {
    if (!isBrewlia || podcastLoading || cloudTtsLoading) return

    setPodcastLoading(true)
    setPodcastError(null)

    try {
      const response = await brewliaApi.getPodcastScript(itemId)

      if (response.success) {
        setPodcastDialogues(response.dialogues)
        setPodcastLanguage(response.language || 'zh-CN')
        setShowPodcastPlayer(true)

        // 优先检查云端缓存
        if (cloudTtsAvailable) {
          try {
            const cache = await getArticleCacheInfo(sourceId, itemId)
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
              if (result) {
                showToastMessage(`${t.brew.cached}: ${completeCache.voiceName}`)
              }
              setCloudTtsLoading(false)
            } else {
              // 无缓存，使用系统 TTS
              setTtsEngine('system')
              saveTTSSettings({ engine: 'system' })

              if (podcastPlayerRef.current) {
                podcastPlayerRef.current.load(
                  response.dialogues,
                  response.language,
                )
                const voices = await PodcastPlayer.getAvailableVoices()
                const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
                  voices,
                  response.language || 'zh-CN',
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
              const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
                voices,
                response.language || 'zh-CN',
              )
              if (voiceA && voiceB) {
                podcastPlayerRef.current.setVoices(voiceA, voiceB)
              }
            }
            showToastMessage(`${response.dialogues.length}`)
          }
        } else {
          // 云端 TTS 不可用
          setTtsEngine('system')
          if (podcastPlayerRef.current) {
            podcastPlayerRef.current.load(response.dialogues, response.language)
            const voices = await PodcastPlayer.getAvailableVoices()
            const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
              voices,
              response.language || 'zh-CN',
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
      console.error('Failed to load podcast:', err)
      setPodcastError(userFacingError(err, t.brew.generatePodcastFailed))
    } finally {
      setPodcastLoading(false)
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

  // 强制重新生成播客稿
  const regeneratePodcast = useCallback(async () => {
    if (!isBrewlia || podcastLoading || cloudTtsLoading) return

    podcastPlayerRef.current?.stop()
    cloudPodcastPlayerRef.current?.stop()
    setPodcastState('stopped')
    setPodcastCurrentIndex(0)

    setPodcastLoading(true)
    setPodcastError(null)
    showToastMessage(t.brew.regeneratingScript)

    try {
      const response = await brewliaApi.regeneratePodcastScript(itemId)

      if (response.success) {
        setPodcastDialogues(response.dialogues)
        setPodcastLanguage(response.language || 'zh-CN')
        setShowPodcastPlayer(true)

        setTtsEngine('system')
        saveTTSSettings({ engine: 'system' })

        if (podcastPlayerRef.current) {
          podcastPlayerRef.current.load(response.dialogues, response.language)
          const voices = await PodcastPlayer.getAvailableVoices()
          const { voiceA, voiceB } = PodcastPlayer.selectVoicePair(
            voices,
            response.language || 'zh-CN',
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
      console.error('Failed to regenerate podcast:', err)
      const errMsg = userFacingError(err, t.brew.generatePodcastFailed)
      setPodcastError(errMsg)
      showToastMessage(errMsg, 3000)
    } finally {
      setPodcastLoading(false)
    }
  }, [isBrewlia, podcastLoading, cloudTtsLoading, itemId, showToastMessage, t])

  // 重新加载云端 TTS
  const reloadCloudTTS = useCallback(async () => {
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
      if (result) {
        if (result.generated > 0) {
          showToastMessage(`${result.generated}`)
        } else {
          showToastMessage(t.brew.cached)
        }
      }
    } catch (err) {
      const errMsg = userFacingError(err, t.brew.loadFailed)
      console.error('Failed to reload cloud TTS:', errMsg, err)
      showToastMessage(`${t.brew.cloudTtsUnavailable}: ${errMsg}`, 3000)
    } finally {
      setCloudTtsLoading(false)
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

  // 播放控制
  const handlePodcastPlay = useCallback(async () => {
    const player = getActivePlayer()
    if (player && 'play' in player && typeof player.play === 'function') {
      await player.play()
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
    if (podcastCurrentIndex <= 0) return
    const newIndex = podcastCurrentIndex - 1
    const player = getActivePlayer()
    if (player) {
      if (ttsEngine === 'cloud') {
        await player.seekTo(newIndex)
        if (podcastState === 'playing') {
          await (player as CloudPodcastPlayer).play()
        }
      } else {
        ;(player as PodcastPlayer).seekTo(newIndex, podcastState === 'playing')
      }
    }
  }, [podcastCurrentIndex, podcastState, getActivePlayer, ttsEngine])

  const handlePodcastNext = useCallback(async () => {
    if (podcastCurrentIndex >= podcastDialogues.length - 1) return
    const newIndex = podcastCurrentIndex + 1
    const player = getActivePlayer()
    if (player) {
      if (ttsEngine === 'cloud') {
        await player.seekTo(newIndex)
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
      const player = getActivePlayer()
      if (player) {
        if (ttsEngine === 'cloud') {
          await player.seekTo(index)
          await (player as CloudPodcastPlayer).play()
          setPodcastState('playing')
        } else {
          ;(player as PodcastPlayer).seekTo(index, true)
        }
      }
    },
    [getActivePlayer, ttsEngine],
  )

  // 自动滚动到当前播放的对话
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
    // 播客状态
    podcastDialogues,
    podcastLoading,
    podcastError,
    showPodcastPlayer,
    podcastState,
    podcastCurrentIndex,
    podcastLanguage,

    // TTS 引擎状态
    ttsEngine,
    cloudTtsAvailable,
    cloudTtsError,
    cloudTtsLoading,
    cloudTtsLoadProgress,

    // TTS 音色状态
    voiceList,
    showVoiceSettings,
    hostVoiceId,
    guestVoiceId,

    // 文章缓存状态
    articleCache,
    articleCacheLoading,
    clearingVoiceId,

    // 分组音色
    groupedVoices,
    voiceNameById,

    // 操作
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

    // 播放控制
    handlePodcastPlay,
    handlePodcastPause,
    handlePodcastStop,
    handlePodcastPrev,
    handlePodcastNext,
    handlePodcastSeek,

    // Refs
    podcastListRef,
  }
}
