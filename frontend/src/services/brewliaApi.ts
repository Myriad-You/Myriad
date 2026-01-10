/**
 * Brewlia AI 增强 API 服务
 *
 * 提供 AI 阅读辅助注释、内容理解等增强阅读功能
 */

import { clearCSRFToken, getCSRFToken } from '../utils/csrf'

const API_BASE = '/api/brewlia'

/**
 * 注释类型
 */
export type AnnotationType = 'term' | 'reference' | 'implicit' | 'context' | 'abbreviation'

/**
 * 注释类型配置
 */
export const ANNOTATION_TYPE_CONFIG: Record<AnnotationType, {
  label: string
  color: string
  bgColor: string
  icon: string
}> = {
  reference: {
    label: '指代',
    color: 'text-blue-600 dark:text-blue-400',
    bgColor: 'bg-blue-100 dark:bg-blue-900/30',
    icon: '👆',
  },
  implicit: {
    label: '隐含',
    color: 'text-purple-600 dark:text-purple-400',
    bgColor: 'bg-purple-100 dark:bg-purple-900/30',
    icon: '💭',
  },
  term: {
    label: '术语',
    color: 'text-orange-600 dark:text-orange-400',
    bgColor: 'bg-orange-100 dark:bg-orange-900/30',
    icon: '📚',
  },
  context: {
    label: '背景',
    color: 'text-green-600 dark:text-green-400',
    bgColor: 'bg-green-100 dark:bg-green-900/30',
    icon: '🌍',
  },
  abbreviation: {
    label: '缩写',
    color: 'text-pink-600 dark:text-pink-400',
    bgColor: 'bg-pink-100 dark:bg-pink-900/30',
    icon: '🔤',
  },
}

/**
 * 注释项
 */
export interface AnnotationItem {
  /** 注释 ID（持久化后有值） */
  id?: number
  /** 注释类型 */
  type: AnnotationType
  /** 原词/短语 */
  term: string
  /** 注释说明 */
  explanation: string
  /** 位置（可选） */
  position?: number
  /** 上下文提示 */
  context_hint?: string
}

/**
 * 注释响应
 */
export interface AnnotationsResponse {
  success: boolean
  annotations: AnnotationItem[]
  /** 是否从缓存读取 */
  from_cache: boolean
  /** 文章语言 */
  detected_language?: string
  /** 错误信息 */
  error?: string
}

/**
 * 通用 API 请求（带 CSRF 重试机制）
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

  const method = options.method?.toUpperCase() || 'GET'
  const needsCSRF = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method)

  if (needsCSRF) {
    const csrfToken = await getCSRFToken()
    if (!csrfToken) {
      throw new Error('请先登录后再使用此功能')
    }
    headers['X-CSRF-Token'] = csrfToken
  }

  const response = await fetch(`${API_BASE}${endpoint}`, {
    ...options,
    headers,
    credentials: 'include',
  })

  const data = await response.json()

  if (!response.ok) {
    if (response.status === 403 && retryOnCSRFError && needsCSRF) {
      const errorMsg = data.error || ''
      if (errorMsg.includes('CSRF') || errorMsg.includes('csrf')) {
        console.warn('CSRF token invalid, refreshing and retrying...')
        clearCSRFToken()
        return request<T>(endpoint, options, false)
      }
    }
    throw new Error(data.error || `HTTP ${response.status}`)
  }

  return data
}

/**
 * 获取文章注释（优先从数据库，不存在则生成）
 */
export async function getAnnotations(itemId: number): Promise<AnnotationsResponse> {
  return request<AnnotationsResponse>(`/items/${itemId}/annotations`)
}

/**
 * 重新生成文章注释
 */
export async function regenerateAnnotations(itemId: number): Promise<AnnotationsResponse> {
  return request<AnnotationsResponse>(`/items/${itemId}/annotations/regenerate`, {
    method: 'POST',
  })
}

// ==================== 播客功能 ====================

/**
 * 播客对话项
 */
export interface PodcastDialogue {
  /** 说话者：host_a（主持人A）或 host_b（主持人B） */
  speaker: 'host_a' | 'host_b'
  /** 对话内容 */
  text: string
}

/**
 * 播客脚本响应
 */
export interface PodcastResponse {
  success: boolean
  /** 播客标题 */
  title: string
  /** 对话列表 */
  dialogues: PodcastDialogue[]
  /** 检测到的语言 */
  language?: string
  /** 预计时长（秒） */
  estimated_duration: number
  /** 错误信息 */
  error?: string
}

/**
 * 获取文章的播客脚本
 */
export async function getPodcastScript(itemId: number): Promise<PodcastResponse> {
  return request<PodcastResponse>(`/items/${itemId}/podcast`)
}

/**
 * 强制重新生成播客脚本
 */
export async function regeneratePodcastScript(itemId: number): Promise<PodcastResponse> {
  return request<PodcastResponse>(`/items/${itemId}/podcast/regenerate`, {
    method: 'POST',
  })
}

/**
 * 播客播放器配置
 */
export interface PodcastPlayerConfig {
  /** 主持人 A 的语音（男声） */
  voiceA?: SpeechSynthesisVoice
  /** 主持人 B 的语音（女声） */
  voiceB?: SpeechSynthesisVoice
  /** 语速（0.5 - 2.0） */
  rate?: number
  /** 音高（0 - 2） */
  pitch?: number
  /** 对话间隔（毫秒） */
  dialogueGap?: number
}

/**
 * 播客播放器类
 * 使用 Web Speech API 播放对话式播客
 */
export class PodcastPlayer {
  private dialogues: PodcastDialogue[] = []
  private currentIndex = 0
  private isPlaying = false
  private isPaused = false
  private isSeeking = false // 防止 cancel 触发 onend 时修改索引
  private synth: SpeechSynthesis
  private config: Required<PodcastPlayerConfig>
  private onProgress?: (index: number, total: number) => void
  private onEnd?: () => void
  private onStateChange?: (state: 'playing' | 'paused' | 'stopped') => void
  private currentUtterance: SpeechSynthesisUtterance | null = null
  private language: string = 'zh-CN'
  private utteranceId = 0 // 用于追踪当前 utterance

  constructor(config?: PodcastPlayerConfig) {
    this.synth = window.speechSynthesis
    this.config = {
      voiceA: config?.voiceA || null as unknown as SpeechSynthesisVoice,
      voiceB: config?.voiceB || null as unknown as SpeechSynthesisVoice,
      rate: config?.rate ?? 1.0,
      pitch: config?.pitch ?? 1.0,
      dialogueGap: config?.dialogueGap ?? 500,
    }
  }

  /**
   * 获取可用的语音列表
   */
  static getAvailableVoices(): Promise<SpeechSynthesisVoice[]> {
    return new Promise((resolve) => {
      const voices = window.speechSynthesis.getVoices()
      if (voices.length > 0) {
        resolve(voices)
      }
      else {
        window.speechSynthesis.onvoiceschanged = () => {
          resolve(window.speechSynthesis.getVoices())
        }
      }
    })
  }

  /**
   * 按语言筛选语音
   */
  static filterVoicesByLanguage(voices: SpeechSynthesisVoice[], lang: string): SpeechSynthesisVoice[] {
    const langPrefix = lang.split('-')[0].toLowerCase()
    return voices.filter(v => v.lang.toLowerCase().startsWith(langPrefix))
  }

  /**
   * 自动选择两个不同的语音（尝试区分性别）
   */
  static selectVoicePair(voices: SpeechSynthesisVoice[], lang: string): { voiceA: SpeechSynthesisVoice | null, voiceB: SpeechSynthesisVoice | null } {
    const filtered = this.filterVoicesByLanguage(voices, lang)

    console.log('[PodcastPlayer] Available voices for language', lang, ':', filtered.map(v => ({ name: v.name, lang: v.lang })))

    if (filtered.length === 0) {
      // 回退到任意语音
      console.log('[PodcastPlayer] No voices found for language, using fallback')
      return { voiceA: voices[0] || null, voiceB: voices[1] || voices[0] || null }
    }

    // 尝试找到不同性别的语音（通过名称推断）
    // Windows 中文语音: Microsoft Huihui (女), Microsoft Kangkang (男), Microsoft Yaoyao (女)
    const maleKeywords = ['male', 'man', '男', 'david', 'mark', 'james', 'kangkang', 'yunxi', 'yunyang']
    const femaleKeywords = ['female', 'woman', '女', 'samantha', 'victoria', 'huihui', 'yaoyao', 'xiaoxiao', 'xiaoyi']

    let maleVoice = filtered.find(v => maleKeywords.some(k => v.name.toLowerCase().includes(k)))
    let femaleVoice = filtered.find(v => femaleKeywords.some(k => v.name.toLowerCase().includes(k)))

    // 如果没找到明确的性别，就用前两个不同的
    if (!maleVoice)
      maleVoice = filtered[0]
    if (!femaleVoice)
      femaleVoice = filtered.find(v => v !== maleVoice) || filtered[0]

    const voicesAreSame = maleVoice === femaleVoice || maleVoice?.name === femaleVoice?.name
    console.log('[PodcastPlayer] Selected voices:', {
      voiceA: maleVoice?.name,
      voiceB: femaleVoice?.name,
      sameVoice: voicesAreSame,
    })

    return { voiceA: maleVoice, voiceB: femaleVoice }
  }

  /**
   * 加载播客脚本
   */
  load(dialogues: PodcastDialogue[], language?: string) {
    this.dialogues = dialogues
    this.currentIndex = 0
    this.isPlaying = false
    this.isPaused = false
    if (language) {
      this.language = language
    }
  }

  /**
   * 设置语音
   */
  setVoices(voiceA: SpeechSynthesisVoice, voiceB: SpeechSynthesisVoice) {
    this.config.voiceA = voiceA
    this.config.voiceB = voiceB
  }

  /**
   * 设置回调
   */
  setCallbacks(callbacks: {
    onProgress?: (index: number, total: number) => void
    onEnd?: () => void
    onStateChange?: (state: 'playing' | 'paused' | 'stopped') => void
  }) {
    this.onProgress = callbacks.onProgress
    this.onEnd = callbacks.onEnd
    this.onStateChange = callbacks.onStateChange
  }

  /**
   * 开始/继续播放
   */
  play() {
    if (this.dialogues.length === 0)
      return

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
    // 更新进度
    this.onProgress?.(this.currentIndex, this.dialogues.length)
    this.speakNext()
  }

  /**
   * 暂停播放
   */
  pause() {
    if (!this.isPlaying)
      return
    this.synth.pause()
    this.isPaused = true
    this.isPlaying = false
    this.onStateChange?.('paused')
  }

  /**
   * 停止播放
   */
  stop() {
    this.isSeeking = true // 防止 onend 触发
    this.synth.cancel()
    this.isSeeking = false
    this.isPlaying = false
    this.isPaused = false
    this.currentIndex = 0
    this.currentUtterance = null
    this.utteranceId++
    this.onStateChange?.('stopped')
    this.onProgress?.(0, this.dialogues.length)
  }

  /**
   * 跳转到指定对话
   * @param index 目标索引
   * @param autoPlay 是否自动开始播放（默认 true）
   */
  seekTo(index: number, autoPlay: boolean = true) {
    if (index < 0 || index >= this.dialogues.length)
      return

    // 标记正在 seek，防止 cancel 触发的 onend 修改索引
    this.isSeeking = true
    this.synth.cancel()
    this.utteranceId++ // 使旧的 utterance 回调失效
    this.isSeeking = false

    // 更新索引
    this.currentIndex = index
    this.isPaused = false

    // 更新进度
    this.onProgress?.(index, this.dialogues.length)

    // 自动播放
    if (autoPlay) {
      this.isPlaying = true
      this.onStateChange?.('playing')
      this.speakNext()
    }
    else if (!this.isPlaying) {
      // 如果不自动播放且当前没在播放，保持停止状态
      this.onStateChange?.('stopped')
    }
  }

  /**
   * 获取当前状态
   */
  getState() {
    return {
      isPlaying: this.isPlaying,
      isPaused: this.isPaused,
      currentIndex: this.currentIndex,
      total: this.dialogues.length,
    }
  }

  /**
   * 播放下一段对话
   */
  private speakNext() {
    if (!this.isPlaying || this.currentIndex >= this.dialogues.length) {
      this.isPlaying = false
      this.onEnd?.()
      this.onStateChange?.('stopped')
      return
    }

    const dialogue = this.dialogues[this.currentIndex]
    const utterance = new SpeechSynthesisUtterance(dialogue.text)

    // 根据说话者选择语音
    const isHostA = dialogue.speaker === 'host_a'
    const voice = isHostA ? this.config.voiceA : this.config.voiceB
    if (voice) {
      utterance.voice = voice
    }

    utterance.lang = this.language

    // 通过语速和音高区分两位主持人
    // Host A (Alex): 较低音高，稍快语速，更沉稳
    // Host B (Blake): 较高音高，稍慢语速，更活泼
    const voicesAreSame = this.config.voiceA === this.config.voiceB
      || (this.config.voiceA?.name === this.config.voiceB?.name)

    if (voicesAreSame) {
      // 如果只有一个语音可用，通过音高和语速来区分
      utterance.rate = isHostA ? this.config.rate * 1.05 : this.config.rate * 0.95
      utterance.pitch = isHostA ? this.config.pitch * 0.85 : this.config.pitch * 1.2
    }
    else {
      // 有不同语音时，轻微调整即可
      utterance.rate = this.config.rate
      utterance.pitch = isHostA ? this.config.pitch : this.config.pitch * 1.1
    }

    this.currentUtterance = utterance

    // 捕获当前 utterance 的 ID，用于验证回调是否有效
    const myUtteranceId = this.utteranceId
    const myIndex = this.currentIndex

    utterance.onend = () => {
      // 如果正在 seek 或 utterance ID 已变，忽略此回调
      if (this.isSeeking || myUtteranceId !== this.utteranceId) {
        return
      }

      // 增加索引到下一段
      this.currentIndex = myIndex + 1

      // 对话间隔后继续
      setTimeout(() => {
        // 再次检查状态
        if (this.isSeeking || myUtteranceId !== this.utteranceId) {
          return
        }
        if (this.isPlaying && !this.isPaused) {
          // 更新进度
          if (this.currentIndex < this.dialogues.length) {
            this.onProgress?.(this.currentIndex, this.dialogues.length)
          }
          this.speakNext()
        }
      }, this.config.dialogueGap)
    }

    utterance.onerror = (e) => {
      // 如果正在 seek 或 utterance ID 已变，忽略此回调
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

  /**
   * 销毁播放器
   */
  destroy() {
    this.stop()
    this.dialogues = []
  }
}

/**
 * 从 HTML 中提取纯文本
 * 安全：使用 DOMParser 避免 innerHTML 触发脚本
 */
export function extractTextFromHtml(html: string): string {
  try {
    const parser = new DOMParser()
    const doc = parser.parseFromString(html, 'text/html')

    const scripts = doc.querySelectorAll('script, style')
    scripts.forEach(s => s.remove())

    return doc.body.textContent || ''
  }
  catch {
    // 回退：直接移除所有 HTML 标签
    return html.replace(/<[^>]*>/g, '')
  }
}

/**
 * HTML 转义，防止 XSS 攻击
 */
function escapeHtml(text: string): string {
  const escapeMap: Record<string, string> = {
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    '\'': '&#39;',
  }
  return text.replace(/[&<>"']/g, char => escapeMap[char] || char)
}

/**
 * 在 HTML 中高亮注释词汇
 *
 * 规则：
 * 1. 每个注释词汇只标记第一次出现
 * 2. 指代类型(reference)优先匹配有上下文的位置，找不到则标记第一次出现
 * 3. 使用主题适配的 CSS 变量来控制颜色
 */
export function highlightAnnotations(html: string, annotations: AnnotationItem[]): string {
  if (!annotations || annotations.length === 0) {
    console.debug('[Brewlia] No annotations to highlight')
    return html
  }

  console.debug('[Brewlia] Highlighting', annotations.length, 'annotations')

  // 按词汇长度降序排列（长的先匹配，避免短词覆盖长词）
  const sortedAnnotations = [...annotations].sort((a, b) => b.term.length - a.term.length)

  // 只解析一次 DOM，同时获取纯文本和进行 DOM 操作
  const div = document.createElement('div')
  div.innerHTML = html
  const fullText = div.textContent || ''

  // 记录已标记的词汇（每个词只标记一次）
  const markedTerms = new Set<string>()
  let totalMatches = 0

  // 为指代类型找到最佳匹配位置（在纯文本中）
  const findBestPositionForReference = (term: string, contextHint?: string): number => {
    if (!contextHint)
      return fullText.indexOf(term)

    // 在全文中找到所有出现位置
    const positions: number[] = []
    let pos = 0
    while ((pos = fullText.indexOf(term, pos)) !== -1) {
      positions.push(pos)
      pos += 1
    }

    if (positions.length === 0)
      return -1
    if (positions.length === 1)
      return positions[0]

    // 从 context_hint 中提取关键词（去掉标点和短词）
    const contextKeywords = contextHint
      .replace(/[，。、！？：；"'（）[\]【】]/g, ' ')
      .split(/\s+/)
      .filter(k => k.length >= 2)

    if (contextKeywords.length === 0)
      return positions[0]

    // 为每个位置打分
    let bestPos = positions[0]
    let bestScore = 0

    for (const termPos of positions) {
      // 扩大上下文窗口到200字符
      const start = Math.max(0, termPos - 200)
      const end = Math.min(fullText.length, termPos + term.length + 200)
      const context = fullText.slice(start, end).toLowerCase()

      // 计算匹配的关键词数量
      let score = 0
      for (const keyword of contextKeywords) {
        if (context.includes(keyword.toLowerCase())) {
          score += keyword.length // 长关键词权重更高
        }
      }

      // 位置靠前有轻微加分（首次提及更可能需要解释）
      score += (1 - termPos / fullText.length) * 2

      if (score > bestScore) {
        bestScore = score
        bestPos = termPos
      }
    }

    console.debug(`[Brewlia] Reference "${term}" best position: ${bestPos} (score: ${bestScore})`)
    return bestPos
  }

  // 记录指代类型的最佳位置（基于纯文本偏移）
  const referencePositions = new Map<string, number>()
  for (const annotation of sortedAnnotations) {
    if (annotation.type === 'reference') {
      const bestPos = findBestPositionForReference(annotation.term, annotation.context_hint)
      if (bestPos >= 0) {
        referencePositions.set(annotation.term, bestPos)
      }
    }
  }

  // 追踪当前处理到的文本偏移
  let currentTextOffset = 0

  // 递归处理文本节点
  const processNode = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      const text = node.textContent || ''
      if (!text.trim()) {
        currentTextOffset += text.length
        return
      }

      const nodeStartOffset = currentTextOffset
      const nodeEndOffset = currentTextOffset + text.length

      let newHtml = text
      let hasMatch = false
      let localOffset = 0 // 记录在当前节点内已处理的偏移

      for (const annotation of sortedAnnotations) {
        if (!annotation.term || annotation.term.length === 0)
          continue

        // 已标记过的词跳过
        const termKey = `${annotation.type}:${annotation.term}`
        if (markedTerms.has(termKey))
          continue

        // 查找位置
        const termIndex = newHtml.indexOf(annotation.term, localOffset)
        if (termIndex === -1)
          continue

        // 对于指代类型，检查是否是最佳位置
        if (annotation.type === 'reference' && referencePositions.has(annotation.term)) {
          const bestPos = referencePositions.get(annotation.term)!
          const actualPos = nodeStartOffset + termIndex

          // 如果当前位置不在最佳位置的合理范围内（±150字符），跳过
          // 但如果这是唯一一次出现（bestPos 就是第一次），则允许
          const isOnlyOccurrence = !fullText.includes(annotation.term, bestPos + 1)
            && !fullText.includes(annotation.term, bestPos - 1)

          if (!isOnlyOccurrence && Math.abs(actualPos - bestPos) > 150) {
            continue
          }
        }

        hasMatch = true
        totalMatches++
        markedTerms.add(termKey)

        // 只替换这一次出现
        const before = newHtml.slice(0, termIndex)
        const after = newHtml.slice(termIndex + annotation.term.length)
        // 安全：确保 annotationId 只包含安全字符
        const rawId = annotation.id || `${annotation.type}-${totalMatches}`
        const annotationId = String(rawId).replace(/[^\w-]/g, '')

        // 安全：对 term 进行 HTML 转义，防止 XSS
        const safeTerm = escapeHtml(annotation.term)
        newHtml = `${before}<mark class="brewlia-annotation" data-annotation-id="${annotationId}" data-type="${escapeHtml(annotation.type)}" data-term="${encodeURIComponent(annotation.term)}" data-explanation="${encodeURIComponent(annotation.explanation)}">${safeTerm}</mark>${after}`

        // 更新本地偏移，跳过刚插入的标记
        localOffset = termIndex + annotation.term.length + 150 // 大约是 mark 标签的长度
      }

      currentTextOffset = nodeEndOffset

      if (hasMatch && node.parentNode) {
        const span = document.createElement('span')
        span.innerHTML = newHtml
        node.parentNode.replaceChild(span, node)
      }
    }
    else if (node.nodeType === Node.ELEMENT_NODE) {
      const tagName = (node as Element).tagName.toLowerCase()
      if (!['script', 'style', 'mark', 'code', 'pre'].includes(tagName)) {
        const children = Array.from(node.childNodes)
        children.forEach(child => processNode(child))
      }
    }
  }

  processNode(div)

  // 保底机制：检查未标记的注释，在第二轮中标记第一次出现
  const unmarkedAnnotations = sortedAnnotations.filter((a) => {
    const termKey = `${a.type}:${a.term}`
    return !markedTerms.has(termKey)
  })

  if (unmarkedAnnotations.length > 0) {
    console.debug('[Brewlia] Fallback pass for', unmarkedAnnotations.length, 'unmarked annotations')

    // 获取当前 HTML 并重新处理
    let currentHtml = div.innerHTML

    for (const annotation of unmarkedAnnotations) {
      if (!annotation.term || annotation.term.length === 0)
        continue

      const termKey = `${annotation.type}:${annotation.term}`
      if (markedTerms.has(termKey))
        continue

      // 简单文本替换第一次出现（需要避免替换已有的 mark 标签内容）
      // 使用正则匹配：不在 < 和 > 之间的文本
      const escapedTerm = annotation.term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const regex = new RegExp(`(?<!<[^>]*)${escapedTerm}(?![^<]*>)`, '')

      if (regex.test(currentHtml)) {
        totalMatches++
        markedTerms.add(termKey)
        // 安全：确保 annotationId 只包含安全字符
        const rawId = annotation.id || `${annotation.type}-${totalMatches}`
        const annotationId = String(rawId).replace(/[^\w-]/g, '')

        // 安全：对 term 进行 HTML 转义，防止 XSS
        const safeTerm = escapeHtml(annotation.term)
        currentHtml = currentHtml.replace(regex, `<mark class="brewlia-annotation" data-annotation-id="${annotationId}" data-type="${escapeHtml(annotation.type)}" data-term="${encodeURIComponent(annotation.term)}" data-explanation="${encodeURIComponent(annotation.explanation)}">${safeTerm}</mark>`,
        )

        console.debug(`[Brewlia] Fallback marked: "${annotation.term}"`)
      }
    }

    div.innerHTML = currentHtml
  }

  console.debug('[Brewlia] Total matches found:', totalMatches)
  console.debug('[Brewlia] Marked terms:', Array.from(markedTerms))

  return div.innerHTML
}
