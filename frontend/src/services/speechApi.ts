import type { VoiceRunNotice } from '../features/merope/speech/realtimeChat'
import { API_URL } from '../config'
import {
  parseVoiceRunNotice,

} from '../features/merope/speech/realtimeChat'
import { currentCopy } from '../i18n/localeCopy'
import { withAiTimeoutSignal } from '../utils/aiRequestTimeout.mjs'
import { clearCSRFToken, getCSRFToken } from '../utils/csrf'
import { notifyHttpRateLimit } from '../utils/httpRateLimitToast'
import { userFacingError } from '../utils/userFacingError'
import { ApiError, parseApiErrorBody } from './api'

function speechHttpError(
  status: number,
  raw: string,
  fallback: string,
): ApiError {
  let parsed: unknown
  try {
    parsed = raw.trim() ? JSON.parse(raw) : undefined
  } catch {
    parsed = undefined
  }
  const body = parseApiErrorBody(parsed, status)
  const message =
    body.message !== `API Error: ${status}`
      ? body.message
      : raw.trim() || fallback
  return new ApiError(message, status, body.code, body.details, body.hint)
}

const API_BASE = `${API_URL}/api/speech`

export type TTSEngine = 'system' | 'cloud'

export interface VoiceInfo {
  id: number
  name: string
  gender: string
  language: string
  description: string
  voice_type: 'ultra_natural' | 'llm' | 'premium'
  emotion_support: boolean
}

/** Accepts current machine values and leftover Chinese labels. */
export function isMaleVoice(gender: string): boolean {
  return (
    gender === 'male' ||
    gender === 'boy' ||
    gender === '男' ||
    gender === '男童'
  )
}

export function isFemaleVoice(gender: string): boolean {
  return (
    gender === 'female' ||
    gender === 'girl' ||
    gender === '女' ||
    gender === '女童'
  )
}

export function localizedVoiceDescription(
  catalog: object,
  voice: Pick<VoiceInfo, 'id' | 'description'>,
): string {
  const key = `voiceDesc.${voice.id}`
  if (!Object.hasOwn(catalog, key)) {
    return voice.description
  }
  const value = Reflect.get(catalog, key)
  return typeof value === 'string' && value.length > 0 ? value : voice.description
}

export interface TTSRequest {
  text: string
  voice_type?: number
  /** [-2, 6] */
  speed?: number
  /** [-10, 10] */
  volume?: number
  codec?: string
  /** 8000 | 16000 | 24000 */
  sample_rate?: number
  emotion?: string
  /** Skip cache. */
  force_regenerate?: boolean
}

export interface TTSResponse {
  success: boolean
  audio?: string
  session_id?: string
  cached?: boolean
  error?: string
}

export interface BatchTTSDialogue {
  index: number
  speaker: string
  text: string
  voice_type?: number
  /** [-2, 6] */
  speed?: number
}

export interface BatchTTSRequest {
  source_id: number
  article_id: number
  dialogues: BatchTTSDialogue[]
  codec?: string
  /** 8000 | 16000 | 24000 */
  sample_rate?: number
  /** Skip cache fallback. */
  force_regenerate?: boolean
}

export interface BatchTTSAudioItem {
  index: number
  speaker: string
  audio: string
  cached: boolean
}

export interface BatchTTSError {
  index: number
  error: string
}

export interface BatchTTSResponse {
  success: boolean
  audios?: BatchTTSAudioItem[]
  cache_hits: number
  generated: number
  errors?: BatchTTSError[]
  error?: string
}

export interface SpeechStatus {
  available: boolean
  tts_enabled: boolean
  asr_enabled: boolean
  convo_enabled?: boolean
  /** Missing/false: do not speak. */
  persona_speech_enabled?: boolean
  error?: string
}

export interface ConvoSession {
  success: boolean
  app_id: string
  channel: string
  uid: number
  agent_uid: number
  token: string
  agent_id: string
  session_id: string
  error?: string
}

async function request<T>(
  endpoint: string,
  options: RequestInit = {},
  retryOnCSRFError: boolean = true,
): Promise<T> {
  options.signal?.throwIfAborted()
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }

  if (options.method && ['POST', 'PUT', 'DELETE'].includes(options.method)) {
    const csrfToken = await getCSRFToken()
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken
    }
  }

  const url = `${API_BASE}${endpoint}`
  options.signal?.throwIfAborted()
  console.log(`[SpeechAPI] ${options.method || 'GET'} ${url}`)

  const response = await fetch(
    url,
    withAiTimeoutSignal(url, {
      ...options,
      headers,
      credentials: 'include',
    }),
  )

  console.log(`[SpeechAPI] Response status: ${response.status}`)

  notifyHttpRateLimit(response)

  // CSRF: forceRefresh; inflight must not reuse a stale token.
  if (response.status === 403 && retryOnCSRFError) {
    const text = await response.text()
    console.log(`[SpeechAPI] 403 response:`, text)
    if (text.includes('CSRF') || text.includes('csrf')) {
      clearCSRFToken()
      const fresh = await getCSRFToken(true)
      if (!fresh) {
        throw speechHttpError(
          response.status,
          text,
          'CSRF token refresh failed',
        )
      }
      return request<T>(endpoint, options, false)
    }
    throw speechHttpError(
      response.status,
      text,
      currentCopy().errors.requestRejected,
    )
  }

  if (!response.ok) {
    const error = await response.text()
    console.error(`[SpeechAPI] Error response:`, error)
    throw speechHttpError(
      response.status,
      error,
      `${currentCopy().errors.requestFailed} (HTTP ${response.status})`,
    )
  }

  return response.json()
}

/** Tapp sandbox: send Runtime Grant. */
export type SpeechAttributionHeaders = Record<string, string>

let speechStatusCache: SpeechStatus | null = null
let speechStatusInflight: Promise<SpeechStatus> | null = null

/** Invalidate /status cache after settings save. */
export function invalidateSpeechStatusCache(): void {
  speechStatusCache = null
  speechStatusInflight = null
}

/** Tapp attribution skips the host /status cache. */
export async function getSpeechStatus(
  attributionHeaders?: SpeechAttributionHeaders,
): Promise<SpeechStatus> {
  if (attributionHeaders) {
    return request<SpeechStatus>('/status', { headers: attributionHeaders })
  }
  if (speechStatusCache) return speechStatusCache
  if (!speechStatusInflight) {
    speechStatusInflight = request<SpeechStatus>('/status')
      .then((status) => {
        speechStatusCache = status
        speechStatusInflight = null
        return status
      })
      .catch((error: unknown) => {
        speechStatusInflight = null
        throw error
      })
  }
  return speechStatusInflight
}

export async function getVoiceList(
  attributionHeaders?: SpeechAttributionHeaders,
): Promise<{ voices: VoiceInfo[] }> {
  return request<{ voices: VoiceInfo[] }>('/voices', {
    headers: attributionHeaders,
  })
}

export interface ASRRequest {
  audio_data?: string
  url?: string
  format?: string
  engine?: string
  word_info?: number
  filter_dirty?: number
  hotword_list?: string
}

export interface ASRWord {
  word: string
  start_time: number
  end_time: number
}

export interface ASRResponse {
  success: boolean
  text?: string
  /** ms */
  duration?: number
  words?: ASRWord[]
  error?: string
}

export async function speechToText(
  req: ASRRequest,
  attributionHeaders?: SpeechAttributionHeaders,
  signal?: AbortSignal,
): Promise<ASRResponse> {
  return request<ASRResponse>('/asr', {
    method: 'POST',
    body: JSON.stringify(req),
    headers: attributionHeaders,
    signal,
  })
}

export async function startConvoSession(
  language?: string,
  sessionId?: string | null,
): Promise<ConvoSession> {
  return request<ConvoSession>('/convo/start', {
    method: 'POST',
    body: JSON.stringify({ language, session_id: sessionId }),
  })
}

/** Cookie notices: run IDs only; never the cloud callback key. */
export function subscribeConvoRuns(
  agentId: string,
  onRun: (notice: VoiceRunNotice) => void,
  onClosed: () => void,
): () => void {
  const events = new EventSource(
    `${API_BASE}/convo/events?agent_id=${encodeURIComponent(agentId)}`,
    { withCredentials: true },
  )
  let after = 0
  let closed = false
  const close = () => {
    if (closed) return
    closed = true
    events.close()
    onClosed()
  }
  events.addEventListener('closed', close)
  events.onerror = () => {
    if (events.readyState === EventSource.CLOSED) close()
  }
  events.onmessage = (event) => {
    if (closed) return
    let value: unknown
    try {
      value = JSON.parse(event.data)
    } catch {
      return
    }
    const notice = parseVoiceRunNotice(value)
    if (!notice || notice.sequence <= after) return
    after = notice.sequence
    onRun(notice)
  }
  return () => {
    closed = true
    events.close()
  }
}

export async function stopConvoSession(
  agentId: string,
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>('/convo/stop', {
    method: 'POST',
    body: JSON.stringify({ agent_id: agentId }),
  })
}

export async function interruptConvoSession(
  agentId: string,
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>('/convo/interrupt', {
    method: 'POST',
    body: JSON.stringify({ agent_id: agentId }),
  })
}

export function audioToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onloadend = () => {
      const base64 = reader.result as string
      const base64Data = base64.split(',')[1]
      resolve(base64Data)
    }
    reader.onerror = reject
    reader.readAsDataURL(blob)
  })
}

export async function textToSpeech(
  req: TTSRequest,
  attributionHeaders?: SpeechAttributionHeaders,
  signal?: AbortSignal,
): Promise<TTSResponse> {
  return request<TTSResponse>('/tts', {
    method: 'POST',
    body: JSON.stringify(req),
    headers: attributionHeaders,
    signal,
  })
}

export async function batchTextToSpeech(
  req: BatchTTSRequest,
  signal?: AbortSignal,
): Promise<BatchTTSResponse> {
  return request<BatchTTSResponse>('/tts/batch', {
    signal,
    method: 'POST',
    body: JSON.stringify(req),
  })
}

export function base64ToAudioUrl(
  base64: string,
  mimeType: string = 'audio/mp3',
): string {
  const byteCharacters = atob(base64)
  const byteArray = new Uint8Array(byteCharacters.length)
  for (let i = 0; i < byteCharacters.length; i++) {
    byteArray[i] = byteCharacters.charCodeAt(i)
  }
  const sniffed =
    byteArray.length >= 12 &&
    byteArray[0] === 0x52 &&
    byteArray[1] === 0x49 &&
    byteArray[2] === 0x46 &&
    byteArray[3] === 0x46
      ? 'audio/wav'
      : mimeType
  const blob = new Blob([byteArray], { type: sniffed })
  return URL.createObjectURL(blob)
}

export class CloudPodcastPlayer {
  private loadController: AbortController | null = null
  private destroyed = false
  private dialogues: Array<{ speaker: string; text: string }> = []
  private audioElements: Map<number, HTMLAudioElement> = new Map()
  private audioUrls: string[] = []
  private currentIndex = 0
  private isPlaying = false
  private isPaused = false
  private isLoading = false
  private dialogueGapTimer: ReturnType<typeof setTimeout> | null = null
  private config = {
    dialogueGap: 300, // ms
  }

  private onProgress?: (current: number, total: number) => void
  private onEnd?: () => void
  private onLoadProgress?: (loaded: number, total: number) => void

  constructor() {}

  setConfig(config: Partial<typeof this.config>) {
    this.config = { ...this.config, ...config }
  }

  setOnProgress(callback: (current: number, total: number) => void) {
    this.onProgress = callback
  }

  setOnEnd(callback: () => void) {
    this.onEnd = callback
  }

  setOnLoadProgress(callback: (loaded: number, total: number) => void) {
    this.onLoadProgress = callback
  }

  async load(
    dialogues: Array<{ speaker: string; text: string }>,
    options: {
      sourceId: number
      articleId: number
      hostVoiceId?: number
      guestVoiceId?: number
      forceRegenerate?: boolean
    },
  ): Promise<{
    success: boolean
    cacheHits: number
    generated: number
    total: number
  }> {
    if (this.destroyed) throw new DOMException('Player destroyed', 'AbortError')
    this.loadController?.abort()
    const controller = new AbortController()
    this.loadController = controller
    this.stop()
    this.cleanup()
    this.dialogues = dialogues
    this.currentIndex = 0
    this.isLoading = true

    try {
      const batchReq: BatchTTSRequest = {
        source_id: options.sourceId,
        article_id: options.articleId,
        dialogues: dialogues.map((d, i) => {
          const isHost = d.speaker === 'host_a' || d.speaker === 'host'
          const voiceId = isHost ? options?.hostVoiceId : options?.guestVoiceId
          return {
            index: i,
            speaker: isHost ? 'host' : 'guest',
            text: d.text,
            voice_type: voiceId,
          }
        }),
        codec: 'mp3',
        sample_rate: 16000,
        force_regenerate: options.forceRegenerate,
      }

      console.log(
        '[CloudPodcastPlayer] Loading TTS for',
        dialogues.length,
        'dialogues',
      )
      console.log('[CloudPodcastPlayer] Options:', {
        sourceId: options.sourceId,
        articleId: options.articleId,
        hostVoiceId: options?.hostVoiceId,
        guestVoiceId: options?.guestVoiceId,
        forceRegenerate: options.forceRegenerate,
      })

      const response = await batchTextToSpeech(batchReq, controller.signal)
      controller.signal.throwIfAborted()

      console.log('[CloudPodcastPlayer] Response:', {
        success: response.success,
        audios_count: response.audios?.length || 0,
        cache_hits: response.cache_hits,
        generated: response.generated,
        errors: response.errors,
        error: response.error,
      })

      if (!response.audios || response.audios.length === 0) {
        if (response.errors && response.errors.length > 0) {
          const firstError = response.errors[0]
          throw new Error(
            userFacingError(
              firstError.error === 'empty_dialogue_text'
                ? currentCopy().errors.emptyDialogueText
                : firstError.error,
              currentCopy().brew.generateFailed,
            ),
          )
        } else if (response.error) {
          throw new Error(
            userFacingError(
              response.error,
              currentCopy().brew.generatePodcastFailed,
            ),
          )
        } else {
          throw new Error(currentCopy().brew.generatePodcastFailed)
        }
      }

      if (response.errors && response.errors.length > 0) {
        console.warn(
          '[CloudPodcastPlayer] Some dialogues failed:',
          response.errors,
        )
      }

      console.log('[CloudPodcastPlayer] TTS loaded:', {
        cache_hits: response.cache_hits,
        generated: response.generated,
        errors: response.errors?.length || 0,
      })

      // Preload current+next only.
      if (response.audios) {
        for (const item of response.audios) {
          const url = base64ToAudioUrl(item.audio, 'audio/mp3')
          this.audioUrls.push(url)

          const audio = new Audio()
          audio.preload = item.index <= 1 ? 'auto' : 'none'
          audio.src = url
          if (item.index <= 1) {
            audio.load()
          }
          this.audioElements.set(item.index, audio)

          this.onLoadProgress?.(this.audioElements.size, dialogues.length)
        }
      }

      this.isLoading = false
      return {
        success: this.audioElements.size > 0,
        cacheHits: response.cache_hits || 0,
        generated: response.generated || 0,
        total: dialogues.length,
      }
    } catch (error) {
      if (controller.signal.aborted) throw error
      console.error('[CloudPodcastPlayer] Load failed:', error)
      this.isLoading = false
      this.cleanup()
      throw error
    }
  }

  async play(): Promise<void> {
    if (this.destroyed || this.dialogues.length === 0) return
    if (this.isLoading) return

    if (this.isPaused) {
      this.isPaused = false
      const audio = this.audioElements.get(this.currentIndex)
      if (audio) {
        await audio.play()
      }
      this.isPlaying = true
      return
    }

    this.isPlaying = true
    this.isPaused = false
    this.playNext()
  }

  pause() {
    if (!this.isPlaying) return
    this.isPaused = true

    const audio = this.audioElements.get(this.currentIndex)
    if (audio) {
      audio.pause()
    }
  }

  async resume() {
    if (!this.isPaused) return
    this.isPaused = false

    const audio = this.audioElements.get(this.currentIndex)
    if (audio) {
      await audio.play()
    }
  }

  stop() {
    this.isPlaying = false
    this.isPaused = false
    if (this.dialogueGapTimer !== null) {
      clearTimeout(this.dialogueGapTimer)
      this.dialogueGapTimer = null
    }

    for (const audio of this.audioElements.values()) {
      audio.pause()
      audio.currentTime = 0
    }
  }

  async seekTo(index: number) {
    if (index < 0 || index >= this.dialogues.length) return

    const currentAudio = this.audioElements.get(this.currentIndex)
    if (currentAudio) {
      currentAudio.pause()
      currentAudio.currentTime = 0
    }

    this.currentIndex = index
    this.onProgress?.(this.currentIndex, this.dialogues.length)

    if (this.isPlaying && !this.isPaused) {
      this.playNext()
    }
  }

  getState() {
    return {
      isPlaying: this.isPlaying,
      isPaused: this.isPaused,
      isLoading: this.isLoading,
      currentIndex: this.currentIndex,
      total: this.dialogues.length,
    }
  }

  getIsLoading(): boolean {
    return this.isLoading
  }

  hasAudio(): boolean {
    return this.audioElements.size > 0
  }

  /** Preload current+next only. */
  private warmPlaybackWindow(index: number) {
    for (const offset of [0, 1]) {
      const audio = this.audioElements.get(index + offset)
      if (!audio) continue
      if (audio.preload !== 'auto') {
        audio.preload = 'auto'
      }
      if (audio.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) {
        audio.load()
      }
    }
  }

  private async playNext() {
    if (!this.isPlaying || this.isPaused) return

    if (this.currentIndex >= this.dialogues.length) {
      this.isPlaying = false
      this.onEnd?.()
      return
    }

    this.warmPlaybackWindow(this.currentIndex)

    const audio = this.audioElements.get(this.currentIndex)
    if (!audio) {
      console.warn('[CloudPodcastPlayer] No audio for index', this.currentIndex)
      this.currentIndex++
      this.onProgress?.(this.currentIndex, this.dialogues.length)
      this.playNext()
      return
    }

    audio.currentTime = 0

    const currentIdx = this.currentIndex

    audio.onended = () => {
      if (this.currentIndex !== currentIdx) return

      this.currentIndex++

      if (this.dialogueGapTimer !== null) {
        clearTimeout(this.dialogueGapTimer)
      }
      this.dialogueGapTimer = setTimeout(() => {
        this.dialogueGapTimer = null
        if (!this.isPlaying || this.isPaused) return
        this.onProgress?.(this.currentIndex, this.dialogues.length)
        this.playNext()
      }, this.config.dialogueGap)
    }

    audio.onerror = (e) => {
      console.error('[CloudPodcastPlayer] Audio error:', e)
      this.currentIndex++
      this.onProgress?.(this.currentIndex, this.dialogues.length)
      this.playNext()
    }

    try {
      await audio.play()
    } catch (e) {
      console.error('[CloudPodcastPlayer] Play failed:', e)
      this.currentIndex++
      this.onProgress?.(this.currentIndex, this.dialogues.length)
      this.playNext()
    }
  }

  private cleanup() {
    for (const url of this.audioUrls) {
      URL.revokeObjectURL(url)
    }
    this.audioUrls = []

    for (const audio of this.audioElements.values()) {
      audio.pause()
      audio.onended = null
      audio.onerror = null
      audio.src = ''
    }
    this.audioElements.clear()
  }

  destroy() {
    this.destroyed = true
    this.loadController?.abort()
    this.loadController = null
    this.onProgress = undefined
    this.onEnd = undefined
    this.onLoadProgress = undefined
    this.isLoading = false
    this.stop()
    this.cleanup()
    this.dialogues = []
  }
}

export interface TTSSettings {
  engine: TTSEngine
  hostVoiceId?: number
  guestVoiceId?: number
}

export function getTTSSettings(): TTSSettings {
  try {
    const stored = localStorage.getItem('brewlia_tts_settings')
    if (stored) {
      return JSON.parse(stored)
    }
  } catch (e) {
    console.warn('[TTS] Failed to load settings:', e)
  }
  return { engine: 'system' }
}

export function saveTTSSettings(settings: Partial<TTSSettings>) {
  try {
    const current = getTTSSettings()
    const merged = { ...current, ...settings }
    localStorage.setItem('brewlia_tts_settings', JSON.stringify(merged))
  } catch (e) {
    console.warn('[TTS] Failed to save settings:', e)
    void import('../utils/toastManager').then(({ showError }) => {
      showError(userFacingError(e, currentCopy().errors.ttsSettingsSaveFailed))
    })
  }
}

export interface ClearCacheResponse {
  success: boolean
  deleted_files: number
  deleted_dirs: number
  freed_size: number
  freed_size_formatted: string
  error?: string
}

export async function clearCache(): Promise<ClearCacheResponse> {
  return request<ClearCacheResponse>('/cache/clear', {
    method: 'POST',
  })
}

export interface VoiceCacheInfo {
  voice_id: number
  voice_name: string | null
  role: string
  file_count: number
  total_size: number
  size_formatted: string
  indices: number[]
}

export interface ArticleCacheResponse {
  source_id: number
  article_id: number
  voices: VoiceCacheInfo[]
  total_files: number
  total_size: number
  total_size_formatted: string
}

export async function getArticleCacheInfo(
  sourceId: number,
  articleId: number,
): Promise<ArticleCacheResponse> {
  return request<ArticleCacheResponse>(
    `/cache/article?source_id=${sourceId}&article_id=${articleId}`,
  )
}

export async function clearArticleVoiceCache(
  sourceId: number,
  articleId: number,
  voiceId: number,
): Promise<ClearCacheResponse> {
  return request<ClearCacheResponse>(
    `/cache/article/voice?source_id=${sourceId}&article_id=${articleId}&voice_id=${voiceId}`,
    { method: 'DELETE' },
  )
}
