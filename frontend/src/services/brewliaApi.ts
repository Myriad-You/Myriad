import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { getDefaultLocale } from '../i18n/locales'
import { withAiTimeoutSignal } from '../utils/aiRequestTimeout.mjs'
import { brewSubject } from '../utils/brewSubject'
import { clearCSRFToken, getCSRFToken } from '../utils/csrf'
import { httpStatusMessage, isUselessErrorText } from '../utils/userFacingError'
import { parseApiErrorBody } from './api'

const API_BASE = `${API_URL}/api/brewlia`

export type AnnotationType =
  'term' | 'reference' | 'implicit' | 'context' | 'abbreviation'

export const ANNOTATION_TYPE_CONFIG: Record<
  AnnotationType,
  {
    color: string
    bgColor: string
    icon: string
  }
> = {
  reference: {
    color: 'text-blue-600 dark:text-blue-400',
    bgColor: 'bg-blue-100 dark:bg-blue-900/30',
    icon: 'T',
  },
  implicit: {
    color: 'text-purple-600 dark:text-purple-400',
    bgColor: 'bg-purple-100 dark:bg-purple-900/30',
    icon: 'I',
  },
  term: {
    color: 'text-orange-600 dark:text-orange-400',
    bgColor: 'bg-orange-100 dark:bg-orange-900/30',
    icon: 'C',
  },
  context: {
    color: 'text-green-600 dark:text-green-400',
    bgColor: 'bg-green-100 dark:bg-green-900/30',
    icon: 'B',
  },
  abbreviation: {
    color: 'text-pink-600 dark:text-pink-400',
    bgColor: 'bg-pink-100 dark:bg-pink-900/30',
    icon: 'W',
  },
}

export function annotationTypeLabel(type: AnnotationType): string {
  const t = currentCopy().brew
  switch (type) {
    case 'reference':
      return t.annotationReference
    case 'implicit':
      return t.annotationImplicit
    case 'term':
      return t.annotationTerm
    case 'context':
      return t.annotationContext
    case 'abbreviation':
      return t.annotationAbbreviation
  }
}

export interface AnnotationItem {
  /** 持久化后才有 */
  id?: number
  type: AnnotationType
  term: string
  explanation: string
  position?: number
  context_hint?: string
}

export interface AnnotationsResponse {
  success: boolean
  annotations: AnnotationItem[]
  from_cache: boolean
  detected_language?: string
  error?: string
}

async function request<T>(
  endpoint: string,
  options: RequestInit = {},
  retryOnCSRFError: boolean = true,
): Promise<T> {
  const subject = brewSubject.capture()
  options = {
    ...options,
    signal: options.signal
      ? AbortSignal.any([options.signal, subject.signal])
      : subject.signal,
  }
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }

  const method = options.method?.toUpperCase() || 'GET'
  const needsCSRF = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method)

  if (needsCSRF) {
    const csrfToken = await getCSRFToken()
    if (!csrfToken) {
      throw new Error(currentCopy().userModal.pleaseLogin)
    }
    headers['X-CSRF-Token'] = csrfToken
  }

  const url = `${API_BASE}${endpoint}`
  brewSubject.assert(subject)
  const response = await fetch(
    url,
    withAiTimeoutSignal(url, {
      ...options,
      headers,
      credentials: 'include',
    }),
  )

  const data = await response.json()
  brewSubject.assert(subject)

  if (!response.ok) {
    if (response.status === 403 && retryOnCSRFError && needsCSRF) {
      const errorMsg = data.error || ''
      if (errorMsg.includes('CSRF') || errorMsg.includes('csrf')) {
        console.warn(
          '[BrewliaAPI] CSRF rejection — force-refreshing token and retrying once',
        )
        clearCSRFToken()
        const fresh = await getCSRFToken(true)
        brewSubject.assert(subject)
        if (!fresh) {
          throw new Error(currentCopy().errors.csrfUnavailable)
        }
        return request<T>(endpoint, options, false)
      }
    }
    const parsed = parseApiErrorBody(data, response.status)
    throw new Error(
      isUselessErrorText(parsed.message)
        ? httpStatusMessage(response.status)
        : parsed.message,
    )
  }

  return data
}

export async function getAnnotations(
  itemId: number,
  signal?: AbortSignal,
): Promise<AnnotationsResponse> {
  return request<AnnotationsResponse>(`/items/${itemId}/annotations`, {
    signal,
  })
}

export async function regenerateAnnotations(
  itemId: number,
  signal?: AbortSignal,
): Promise<AnnotationsResponse> {
  return request<AnnotationsResponse>(
    `/items/${itemId}/annotations/regenerate`,
    {
      method: 'POST',
      signal,
    },
  )
}

export interface PodcastDialogue {
  speaker: 'host_a' | 'host_b'
  text: string
}

export interface PodcastResponse {
  success: boolean
  title: string
  dialogues: PodcastDialogue[]
  language?: string
  /** 秒 */
  estimated_duration: number
  error?: string
}

export async function getPodcastScript(
  itemId: number,
  signal?: AbortSignal,
): Promise<PodcastResponse> {
  return request<PodcastResponse>(`/items/${itemId}/podcast`, { signal })
}

export async function regeneratePodcastScript(
  itemId: number,
  signal?: AbortSignal,
): Promise<PodcastResponse> {
  return request<PodcastResponse>(`/items/${itemId}/podcast/regenerate`, {
    method: 'POST',
    signal,
  })
}

export interface PodcastPlayerConfig {
  voiceA?: SpeechSynthesisVoice
  voiceB?: SpeechSynthesisVoice
  rate?: number
  pitch?: number
  /** ms */
  dialogueGap?: number
}

export class PodcastPlayer {
  private dialogues: PodcastDialogue[] = []
  private currentIndex = 0
  private isPlaying = false
  private isPaused = false
  private isSeeking = false // cancel 触发的 onend 不得改索引
  private synth: SpeechSynthesis
  private config: Required<PodcastPlayerConfig>
  private onProgress?: (index: number, total: number) => void
  private onEnd?: () => void
  private onStateChange?: (state: 'playing' | 'paused' | 'stopped') => void
  private language: string = getDefaultLocale()
  private utteranceId = 0

  constructor(config?: PodcastPlayerConfig) {
    this.synth = window.speechSynthesis
    this.config = {
      voiceA: config?.voiceA ?? (null as unknown as SpeechSynthesisVoice),
      voiceB: config?.voiceB ?? (null as unknown as SpeechSynthesisVoice),
      rate: config?.rate ?? 1.0,
      pitch: config?.pitch ?? 1.0,
      dialogueGap: config?.dialogueGap ?? 500,
    }
  }

  static getAvailableVoices(): Promise<SpeechSynthesisVoice[]> {
    return new Promise((resolve) => {
      const voices = window.speechSynthesis.getVoices()
      if (voices.length > 0) {
        resolve(voices)
      } else {
        window.speechSynthesis.onvoiceschanged = () => {
          resolve(window.speechSynthesis.getVoices())
        }
      }
    })
  }

  static filterVoicesByLanguage(
    voices: SpeechSynthesisVoice[],
    lang: string,
  ): SpeechSynthesisVoice[] {
    const requested = lang.trim().toLowerCase()
    const langPrefix = requested.split('-')[0] ?? ''
    const wantsTraditional =
      requested === 'zh-tw' ||
      requested.startsWith('zh-hk') ||
      requested.startsWith('zh-mo') ||
      requested.includes('hant')
    if (wantsTraditional) {
      const traditional = voices.filter((voice) => {
        const voiceLang = voice.lang.toLowerCase()
        return (
          voiceLang.startsWith('zh-tw') ||
          voiceLang.startsWith('zh-hk') ||
          voiceLang.startsWith('zh-mo') ||
          voiceLang.includes('hant')
        )
      })
      if (traditional.length > 0) return traditional
    }
    return voices.filter((voice) =>
      voice.lang.toLowerCase().startsWith(langPrefix),
    )
  }

  static selectVoicePair(
    voices: SpeechSynthesisVoice[],
    lang: string,
  ): {
    voiceA: SpeechSynthesisVoice | null
    voiceB: SpeechSynthesisVoice | null
  } {
    const filtered = this.filterVoicesByLanguage(voices, lang)

    console.debug(
      '[PodcastPlayer] Available voices for language',
      lang,
      ':',
      filtered.map((v) => ({ name: v.name, lang: v.lang })),
    )

    if (filtered.length === 0) {
      console.debug(
        '[PodcastPlayer] No voices found for language, using fallback',
      )
      return {
        voiceA: voices[0] || null,
        voiceB: voices[1] || voices[0] || null,
      }
    }

    // 用名称推断性别。Windows 中文：Huihui/Yaoyao 女，Kangkang 男。
    const maleKeywords = [
      'male',
      'man',
      '男',
      'david',
      'mark',
      'james',
      'kangkang',
      'yunxi',
      'yunyang',
    ]
    const femaleKeywords = [
      'female',
      'woman',
      '女',
      'samantha',
      'victoria',
      'huihui',
      'yaoyao',
      'xiaoxiao',
      'xiaoyi',
    ]

    let maleVoice = filtered.find((v) =>
      maleKeywords.some((k) => v.name.toLowerCase().includes(k)),
    )
    let femaleVoice = filtered.find((v) =>
      femaleKeywords.some((k) => v.name.toLowerCase().includes(k)),
    )

    if (!maleVoice) maleVoice = filtered[0]
    if (!femaleVoice)
      femaleVoice = filtered.find((v) => v !== maleVoice) ?? filtered[0]

    const voicesAreSame =
      maleVoice === femaleVoice || maleVoice?.name === femaleVoice?.name
    console.debug('[PodcastPlayer] Selected voices:', {
      voiceA: maleVoice?.name,
      voiceB: femaleVoice?.name,
      sameVoice: voicesAreSame,
    })

    return { voiceA: maleVoice, voiceB: femaleVoice }
  }

  load(dialogues: PodcastDialogue[], language?: string) {
    this.dialogues = dialogues
    this.currentIndex = 0
    this.isPlaying = false
    this.isPaused = false
    if (language) {
      this.language = language
    }
  }

  setVoices(voiceA: SpeechSynthesisVoice, voiceB: SpeechSynthesisVoice) {
    this.config.voiceA = voiceA
    this.config.voiceB = voiceB
  }

  setCallbacks(callbacks: {
    onProgress?: (index: number, total: number) => void
    onEnd?: () => void
    onStateChange?: (state: 'playing' | 'paused' | 'stopped') => void
  }) {
    this.onProgress = callbacks.onProgress
    this.onEnd = callbacks.onEnd
    this.onStateChange = callbacks.onStateChange
  }

  play() {
    if (this.dialogues.length === 0) return

    if (this.isPaused) {
      this.synth.resume()
      this.isPaused = false
      this.isPlaying = true
      this.onStateChange?.('playing')
      return
    }

    this.isPlaying = true
    this.isPaused = false
    this.onStateChange?.('playing')
    this.onProgress?.(this.currentIndex, this.dialogues.length)
    this.speakNext()
  }

  pause() {
    if (!this.isPlaying) return
    this.synth.pause()
    this.isPaused = true
    this.isPlaying = false
    this.onStateChange?.('paused')
  }

  stop() {
    this.isSeeking = true
    this.synth.cancel()
    this.isSeeking = false
    this.isPlaying = false
    this.isPaused = false
    this.currentIndex = 0
    this.utteranceId++
    this.onStateChange?.('stopped')
    this.onProgress?.(0, this.dialogues.length)
  }

  seekTo(index: number, autoPlay: boolean = true) {
    if (index < 0 || index >= this.dialogues.length) return

    this.isSeeking = true
    this.synth.cancel()
    this.utteranceId++
    this.isSeeking = false

    this.currentIndex = index
    this.isPaused = false

    this.onProgress?.(index, this.dialogues.length)

    if (autoPlay) {
      this.isPlaying = true
      this.onStateChange?.('playing')
      this.speakNext()
    } else if (!this.isPlaying) {
      this.onStateChange?.('stopped')
    }
  }

  getState() {
    return {
      isPlaying: this.isPlaying,
      isPaused: this.isPaused,
      currentIndex: this.currentIndex,
      total: this.dialogues.length,
    }
  }

  private speakNext() {
    if (!this.isPlaying || this.currentIndex >= this.dialogues.length) {
      this.isPlaying = false
      this.onEnd?.()
      this.onStateChange?.('stopped')
      return
    }

    const dialogue = this.dialogues[this.currentIndex]
    const utterance = new SpeechSynthesisUtterance(dialogue.text)

    const isHostA = dialogue.speaker === 'host_a'
    const voice = isHostA ? this.config.voiceA : this.config.voiceB
    if (voice) {
      utterance.voice = voice
    }

    utterance.lang = this.language

    const voicesAreSame =
      this.config.voiceA === this.config.voiceB ||
      this.config.voiceA?.name === this.config.voiceB?.name

    if (voicesAreSame) {
      utterance.rate = isHostA
        ? this.config.rate * 1.05
        : this.config.rate * 0.95
      utterance.pitch = isHostA
        ? this.config.pitch * 0.85
        : this.config.pitch * 1.2
    } else {
      utterance.rate = this.config.rate
      utterance.pitch = isHostA ? this.config.pitch : this.config.pitch * 1.1
    }

    const myUtteranceId = this.utteranceId
    const myIndex = this.currentIndex

    utterance.onend = () => {
      if (this.isSeeking || myUtteranceId !== this.utteranceId) {
        return
      }

      this.currentIndex = myIndex + 1

      setTimeout(() => {
        if (this.isSeeking || myUtteranceId !== this.utteranceId) {
          return
        }
        if (this.isPlaying && !this.isPaused) {
          if (this.currentIndex < this.dialogues.length) {
            this.onProgress?.(this.currentIndex, this.dialogues.length)
          }
          this.speakNext()
        }
      }, this.config.dialogueGap)
    }

    utterance.onerror = (e) => {
      if (this.isSeeking || myUtteranceId !== this.utteranceId) {
        return
      }
      console.error('[PodcastPlayer] Speech error:', e)
      this.currentIndex = myIndex + 1
      if (this.isPlaying) {
        this.onProgress?.(this.currentIndex, this.dialogues.length)
        this.speakNext()
      }
    }

    this.synth.speak(utterance)
  }

  destroy() {
    this.stop()
    this.dialogues = []
  }
}

export interface StyleTagsResponse {
  success: boolean
  tags: string[]
  from_cache: boolean
  error?: string
}

/** 最近 10 篇标题 + 前 100 字。 */
export async function generateStyleTags(
  sourceId: number,
  signal?: AbortSignal,
): Promise<StyleTagsResponse> {
  return request<StyleTagsResponse>(`/sources/${sourceId}/style-tags`, {
    method: 'POST',
    signal,
  })
}
