/**
 * 语音服务 API 客户端
 *
 * 提供腾讯云 TTS/ASR 服务的前端接口
 */

import { clearCSRFToken, getCSRFToken } from '../utils/csrf'

const API_BASE = '/api/speech'

/**
 * TTS 引擎类型
 */
export type TTSEngine = 'system' | 'cloud'

/**
 * 语音信息
 */
export interface VoiceInfo {
  id: number
  name: string
  gender: string
  language: string
  description: string
  /** 音色类型: ultra_natural (超自然大模型), llm (大模型), premium (精品) */
  voice_type: 'ultra_natural' | 'llm' | 'premium'
  /** 是否支持情感控制 */
  emotion_support: boolean
}

/**
 * 音色列表响应
 */
export interface VoiceListResponse {
  voices: VoiceInfo[]
}

/**
 * TTS 请求参数
 */
export interface TTSRequest {
  /** 要转换的文本 */
  text: string
  /** 音色ID */
  voice_type?: number
  /** 语速 [-2, 6]，默认0 */
  speed?: number
  /** 音量 [-10, 10]，默认0 */
  volume?: number
  /** 返回格式: wav, mp3, pcm，默认mp3 */
  codec?: string
  /** 采样率: 8000, 16000, 24000，默认16000 */
  sample_rate?: number
  /** 情感类别（仅多情感音色支持） */
  emotion?: string
}

/**
 * TTS 响应
 */
export interface TTSResponse {
  success: boolean
  /** Base64编码的音频数据 */
  audio?: string
  /** 会话ID */
  session_id?: string
  /** 是否来自缓存 */
  cached?: boolean
  /** 错误信息 */
  error?: string
}

/**
 * 批量 TTS 对话项
 */
export interface BatchTTSDialogue {
  /** 对话索引 */
  index: number
  /** 说话者: "host" 或 "guest" */
  speaker: string
  /** 对话文本 */
  text: string
  /** 音色ID（可选） */
  voice_type?: number
  /** 语速 [-2, 6]，默认0 */
  speed?: number
}

/**
 * 批量 TTS 请求
 */
export interface BatchTTSRequest {
  /** 订阅源 ID */
  source_id: number
  /** 文章 ID */
  article_id: number
  /** 对话列表 */
  dialogues: BatchTTSDialogue[]
  /** 返回格式: wav, mp3, pcm，默认mp3 */
  codec?: string
  /** 采样率: 8000, 16000, 24000，默认16000 */
  sample_rate?: number
  /** 强制重新生成（跳过任意音色缓存回退） */
  force_regenerate?: boolean
}

/**
 * 批量 TTS 音频项
 */
export interface BatchTTSAudioItem {
  /** 对话索引 */
  index: number
  /** 说话者 */
  speaker: string
  /** Base64编码的音频数据 */
  audio: string
  /** 是否来自缓存 */
  cached: boolean
}

/**
 * 批量 TTS 错误项
 */
export interface BatchTTSError {
  /** 对话索引 */
  index: number
  /** 错误信息 */
  error: string
}

/**
 * 批量 TTS 响应
 */
export interface BatchTTSResponse {
  success: boolean
  /** 音频列表 */
  audios?: BatchTTSAudioItem[]
  /** 缓存命中数 */
  cache_hits: number
  /** 新生成数 */
  generated: number
  /** 失败列表 */
  errors?: BatchTTSError[]
  /** 总体错误信息 */
  error?: string
}

/**
 * 语音服务状态
 */
export interface SpeechStatus {
  available: boolean
  tts_enabled: boolean
  asr_enabled: boolean
  error?: string
}

/**
 * 通用 API 请求
 */
async function request<T>(
  endpoint: string,
  options: RequestInit = {},
  retryOnCSRFError: boolean = true,
): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }

  // POST/PUT/DELETE 请求需要 CSRF token
  if (options.method && ['POST', 'PUT', 'DELETE'].includes(options.method)) {
    const csrfToken = await getCSRFToken()
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken
    }
  }

  const url = `${API_BASE}${endpoint}`
  console.log(`[SpeechAPI] ${options.method || 'GET'} ${url}`)

  const response = await fetch(url, {
    ...options,
    headers,
    credentials: 'include',
  })

  console.log(`[SpeechAPI] Response status: ${response.status}`)

  // 处理 CSRF 错误
  if (response.status === 403 && retryOnCSRFError) {
    const text = await response.text()
    console.log(`[SpeechAPI] 403 response:`, text)
    if (text.includes('CSRF') || text.includes('csrf')) {
      clearCSRFToken()
      return request<T>(endpoint, options, false)
    }
    throw new Error(text || '请求被拒绝')
  }

  if (!response.ok) {
    const error = await response.text()
    console.error(`[SpeechAPI] Error response:`, error)
    throw new Error(error || `请求失败: ${response.status}`)
  }

  return response.json()
}

/**
 * 获取语音服务状态
 */
export async function getSpeechStatus(): Promise<SpeechStatus> {
  return request<SpeechStatus>('/status')
}

/**
 * 获取可用音色列表
 */
export async function getVoiceList(): Promise<{ voices: VoiceInfo[] }> {
  return request<{ voices: VoiceInfo[] }>('/voices')
}

/**
 * 单条文本转语音
 */
export async function textToSpeech(req: TTSRequest): Promise<TTSResponse> {
  return request<TTSResponse>('/tts', {
    method: 'POST',
    body: JSON.stringify(req),
  })
}

/**
 * 批量文本转语音（用于播客）
 */
export async function batchTextToSpeech(req: BatchTTSRequest): Promise<BatchTTSResponse> {
  return request<BatchTTSResponse>('/tts/batch', {
    method: 'POST',
    body: JSON.stringify(req),
  })
}

/**
 * 将 Base64 音频数据转换为 Blob URL
 */
export function base64ToAudioUrl(base64: string, mimeType: string = 'audio/mp3'): string {
  const byteCharacters = atob(base64)
  const byteNumbers = Array.from({ length: byteCharacters.length })
  for (let i = 0; i < byteCharacters.length; i++) {
    byteNumbers[i] = byteCharacters.charCodeAt(i)
  }
  const byteArray = new Uint8Array(byteNumbers)
  const blob = new Blob([byteArray], { type: mimeType })
  return URL.createObjectURL(blob)
}

/**
 * 云端播客播放器
 *
 * 使用腾讯云 TTS 服务生成播客音频并播放
 */
export class CloudPodcastPlayer {
  private dialogues: Array<{ speaker: string, text: string }> = []
  private audioElements: Map<number, HTMLAudioElement> = new Map()
  private audioUrls: string[] = []
  private currentIndex = 0
  private isPlaying = false
  private isPaused = false
  private isLoading = false
  private config = {
    dialogueGap: 300, // 对话间隔 ms
  }

  // 回调
  private onProgress?: (current: number, total: number) => void
  private onEnd?: () => void
  private onLoadProgress?: (loaded: number, total: number) => void

  constructor() {}

  /**
   * 设置播放配置
   */
  setConfig(config: Partial<typeof this.config>) {
    this.config = { ...this.config, ...config }
  }

  /**
   * 设置进度回调
   */
  setOnProgress(callback: (current: number, total: number) => void) {
    this.onProgress = callback
  }

  /**
   * 设置结束回调
   */
  setOnEnd(callback: () => void) {
    this.onEnd = callback
  }

  /**
   * 设置加载进度回调
   */
  setOnLoadProgress(callback: (loaded: number, total: number) => void) {
    this.onLoadProgress = callback
  }

  /**
   * 加载播客对话
   * @param dialogues 对话列表
   * @param options 可选参数：sourceId (订阅源ID), articleId (文章ID), hostVoiceId (主播音色), guestVoiceId (嘉宾音色), forceRegenerate (强制重新生成)
   * @returns 加载结果，包含成功状态、缓存命中数、新生成数、总数
   */
  async load(
    dialogues: Array<{ speaker: string, text: string }>,
    options: { sourceId: number, articleId: number, hostVoiceId?: number, guestVoiceId?: number, forceRegenerate?: boolean },
  ): Promise<{ success: boolean, cacheHits: number, generated: number, total: number }> {
    this.stop()
    this.cleanup()
    this.dialogues = dialogues
    this.currentIndex = 0
    this.isLoading = true

    try {
      // 构建批量请求
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
            voice_type: voiceId, // 自定义音色ID（可选）
          }
        }),
        codec: 'mp3',
        sample_rate: 16000,
        force_regenerate: options.forceRegenerate,
      }

      console.log('[CloudPodcastPlayer] Loading TTS for', dialogues.length, 'dialogues')
      console.log('[CloudPodcastPlayer] Options:', { sourceId: options.sourceId, articleId: options.articleId, hostVoiceId: options?.hostVoiceId, guestVoiceId: options?.guestVoiceId, forceRegenerate: options.forceRegenerate })

      const response = await batchTextToSpeech(batchReq)

      console.log('[CloudPodcastPlayer] Response:', {
        success: response.success,
        audios_count: response.audios?.length || 0,
        cache_hits: response.cache_hits,
        generated: response.generated,
        errors: response.errors,
        error: response.error,
      })

      // 检查是否有任何音频返回
      if (!response.audios || response.audios.length === 0) {
        // 没有音频，检查具体错误
        if (response.errors && response.errors.length > 0) {
          // 显示第一个错误
          const firstError = response.errors[0]
          throw new Error(`TTS失败: ${firstError.error}`)
        }
        else if (response.error) {
          throw new Error(response.error)
        }
        else {
          throw new Error('批量TTS请求失败：未返回任何音频')
        }
      }

      // 有音频返回，即使部分失败也继续
      if (response.errors && response.errors.length > 0) {
        console.warn('[CloudPodcastPlayer] Some dialogues failed:', response.errors)
      }

      console.log('[CloudPodcastPlayer] TTS loaded:', {
        cache_hits: response.cache_hits,
        generated: response.generated,
        errors: response.errors?.length || 0,
      })

      // 创建音频元素
      if (response.audios) {
        for (const item of response.audios) {
          const url = base64ToAudioUrl(item.audio, 'audio/mp3')
          this.audioUrls.push(url)

          const audio = new Audio(url)
          audio.preload = 'auto'
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
    }
    catch (error) {
      console.error('[CloudPodcastPlayer] Load failed:', error)
      this.isLoading = false
      throw error
    }
  }

  /**
   * 开始播放
   */
  async play(): Promise<void> {
    if (this.dialogues.length === 0)
      return
    if (this.isLoading)
      return

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

  /**
   * 暂停播放
   */
  pause() {
    if (!this.isPlaying)
      return
    this.isPaused = true

    const audio = this.audioElements.get(this.currentIndex)
    if (audio) {
      audio.pause()
    }
  }

  /**
   * 恢复播放
   */
  async resume() {
    if (!this.isPaused)
      return
    this.isPaused = false

    const audio = this.audioElements.get(this.currentIndex)
    if (audio) {
      await audio.play()
    }
  }

  /**
   * 停止播放
   */
  stop() {
    this.isPlaying = false
    this.isPaused = false

    // 停止所有音频
    for (const audio of this.audioElements.values()) {
      audio.pause()
      audio.currentTime = 0
    }
  }

  /**
   * 跳转到指定对话
   */
  async seekTo(index: number) {
    if (index < 0 || index >= this.dialogues.length)
      return

    // 停止当前播放
    const currentAudio = this.audioElements.get(this.currentIndex)
    if (currentAudio) {
      currentAudio.pause()
      currentAudio.currentTime = 0
    }

    this.currentIndex = index
    this.onProgress?.(this.currentIndex, this.dialogues.length)

    // 如果正在播放，继续播放新位置
    if (this.isPlaying && !this.isPaused) {
      this.playNext()
    }
  }

  /**
   * 获取当前播放状态
   */
  getState() {
    return {
      isPlaying: this.isPlaying,
      isPaused: this.isPaused,
      isLoading: this.isLoading,
      currentIndex: this.currentIndex,
      total: this.dialogues.length,
    }
  }

  /**
   * 获取是否正在加载
   */
  getIsLoading(): boolean {
    return this.isLoading
  }

  /**
   * 检查是否已加载音频
   */
  hasAudio(): boolean {
    return this.audioElements.size > 0
  }

  /**
   * 播放下一段
   */
  private async playNext() {
    if (!this.isPlaying || this.isPaused)
      return

    if (this.currentIndex >= this.dialogues.length) {
      this.isPlaying = false
      this.onEnd?.()
      return
    }

    const audio = this.audioElements.get(this.currentIndex)
    if (!audio) {
      // 如果没有音频，跳过
      console.warn('[CloudPodcastPlayer] No audio for index', this.currentIndex)
      this.currentIndex++
      this.onProgress?.(this.currentIndex, this.dialogues.length)
      this.playNext()
      return
    }

    // 重置音频位置
    audio.currentTime = 0

    const currentIdx = this.currentIndex

    audio.onended = () => {
      // 确保是当前播放的音频
      if (this.currentIndex !== currentIdx)
        return

      this.currentIndex++

      // 对话间隔后继续
      setTimeout(() => {
        if (!this.isPlaying || this.isPaused)
          return
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
    }
    catch (e) {
      console.error('[CloudPodcastPlayer] Play failed:', e)
      this.currentIndex++
      this.onProgress?.(this.currentIndex, this.dialogues.length)
      this.playNext()
    }
  }

  /**
   * 清理资源
   */
  private cleanup() {
    // 释放音频 URL
    for (const url of this.audioUrls) {
      URL.revokeObjectURL(url)
    }
    this.audioUrls = []

    // 清理音频元素
    for (const audio of this.audioElements.values()) {
      audio.pause()
      audio.src = ''
    }
    this.audioElements.clear()
  }

  /**
   * 销毁播放器
   */
  destroy() {
    this.stop()
    this.cleanup()
    this.dialogues = []
  }
}

/**
 * TTS 设置
 */
export interface TTSSettings {
  /** TTS 引擎类型 */
  engine: TTSEngine
  /** 主播（男声）音色 ID */
  hostVoiceId?: number
  /** 嘉宾（女声）音色 ID */
  guestVoiceId?: number
}

/**
 * 获取 TTS 设置
 */
export function getTTSSettings(): TTSSettings {
  try {
    const stored = localStorage.getItem('brewlia_tts_settings')
    if (stored) {
      return JSON.parse(stored)
    }
  }
  catch (e) {
    console.warn('[TTS] Failed to load settings:', e)
  }
  return { engine: 'system' }
}

/**
 * 保存 TTS 设置
 */
export function saveTTSSettings(settings: Partial<TTSSettings>) {
  try {
    const current = getTTSSettings()
    const merged = { ...current, ...settings }
    localStorage.setItem('brewlia_tts_settings', JSON.stringify(merged))
  }
  catch (e) {
    console.warn('[TTS] Failed to save settings:', e)
  }
}

/**
 * 缓存统计响应
 */
export interface CacheStatsResponse {
  /** 缓存文件数量 */
  file_count: number
  /** 缓存目录数量（不同文本） */
  text_count: number
  /** 缓存总大小（字节） */
  total_size: number
  /** 格式化的大小 */
  total_size_formatted: string
}

/**
 * 清除缓存响应
 */
export interface ClearCacheResponse {
  success: boolean
  deleted_files: number
  deleted_dirs: number
  freed_size: number
  freed_size_formatted: string
  error?: string
}

/**
 * 获取 TTS 缓存统计
 */
export async function getCacheStats(): Promise<CacheStatsResponse> {
  return request<CacheStatsResponse>('/cache/stats')
}

/**
 * 清除 TTS 缓存
 */
export async function clearCache(): Promise<ClearCacheResponse> {
  return request<ClearCacheResponse>('/cache/clear', {
    method: 'POST',
  })
}

// ==================== 文章缓存管理 ====================

/**
 * 音色缓存信息
 */
export interface VoiceCacheInfo {
  /** 音色 ID */
  voice_id: number
  /** 音色名称 */
  voice_name: string | null
  /** 角色类型 (host/guest/mixed) */
  role: string
  /** 文件数量 */
  file_count: number
  /** 总大小（字节） */
  total_size: number
  /** 格式化大小 */
  size_formatted: string
  /** 对话索引列表 */
  indices: number[]
}

/**
 * 文章缓存信息响应
 */
export interface ArticleCacheResponse {
  source_id: number
  article_id: number
  /** 各音色的缓存信息 */
  voices: VoiceCacheInfo[]
  /** 总文件数 */
  total_files: number
  /** 总大小 */
  total_size: number
  total_size_formatted: string
}

/**
 * 获取文章缓存信息
 */
export async function getArticleCacheInfo(sourceId: number, articleId: number): Promise<ArticleCacheResponse> {
  return request<ArticleCacheResponse>(`/cache/article?source_id=${sourceId}&article_id=${articleId}`)
}

/**
 * 清除文章特定音色的缓存
 */
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
