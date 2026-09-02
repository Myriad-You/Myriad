/**
 * Brewlia AI 增强 API 服务
 *
 * 提供 AI 阅读辅助注释、内容理解等增强阅读功能
 */

import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { clearCSRFToken, getCSRFToken } from '../utils/csrf'
import { httpStatusMessage, isUselessErrorText } from '../utils/userFacingError'
import { parseApiErrorBody } from './api'

const API_BASE = `${API_URL}/api/brewlia`

/**
 * 注释类型
 */
export type AnnotationType =
  'term' | 'reference' | 'implicit' | 'context' | 'abbreviation'

/**
 * 注释类型配置
 */
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
      throw new Error(currentCopy().userModal.pleaseLogin)
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
        console.warn(
          '[BrewliaAPI] CSRF rejection — force-refreshing token and retrying once',
        )
        clearCSRFToken()
        const fresh = await getCSRFToken(true)
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

/**
 * 获取文章注释（优先从数据库，不存在则生成）
 */
export async function getAnnotations(
  itemId: number,
): Promise<AnnotationsResponse> {
  return request<AnnotationsResponse>(`/items/${itemId}/annotations`)
}

/**
 * 重新生成文章注释
 */
export async function regenerateAnnotations(
  itemId: number,
): Promise<AnnotationsResponse> {
  return request<AnnotationsResponse>(
    `/items/${itemId}/annotations/regenerate`,
    {
      method: 'POST',
    },
  )
}

// 播客功能

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
export async function getPodcastScript(
  itemId: number,
): Promise<PodcastResponse> {
  return request<PodcastResponse>(`/items/${itemId}/podcast`)
}

/**
 * 强制重新生成播客脚本
 */
export async function regeneratePodcastScript(
  itemId: number,
): Promise<PodcastResponse> {
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
  private language: string = 'zh-CN'
  private utteranceId = 0 // 用于追踪当前 utterance

  constructor(config?: PodcastPlayerConfig) {
    this.synth = window.speechSynthesis
    this.config = {
      voiceA: config?.voiceA || (null as unknown as SpeechSynthesisVoice),
      voiceB: config?.voiceB || (null as unknown as SpeechSynthesisVoice),
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
      } else {
        window.speechSynthesis.onvoiceschanged = () => {
          resolve(window.speechSynthesis.getVoices())
        }
      }
    })
  }

  /**
   * 按语言筛选语音
   */
  static filterVoicesByLanguage(
    voices: SpeechSynthesisVoice[],
    lang: string,
  ): SpeechSynthesisVoice[] {
    const langPrefix = lang.split('-')[0].toLowerCase()
    return voices.filter((v) => v.lang.toLowerCase().startsWith(langPrefix))
  }

  /**
   * 自动选择两个不同的语音（尝试区分性别）
   */
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
      // 回退到任意语音
      console.debug(
        '[PodcastPlayer] No voices found for language, using fallback',
      )
      return {
        voiceA: voices[0] || null,
        voiceB: voices[1] || voices[0] || null,
      }
    }

    // 尝试找到不同性别的语音（通过名称推断）
    // Windows 中文语音: Microsoft Huihui (女), Microsoft Kangkang (男), Microsoft Yaoyao (女)
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

    // 如果没找到明确的性别，就用前两个不同的
    if (!maleVoice) maleVoice = filtered[0]
    if (!femaleVoice)
      femaleVoice = filtered.find((v) => v !== maleVoice) || filtered[0]

    const voicesAreSame =
      maleVoice === femaleVoice || maleVoice?.name === femaleVoice?.name
    console.debug('[PodcastPlayer] Selected voices:', {
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
    // 更新进度
    this.onProgress?.(this.currentIndex, this.dialogues.length)
    this.speakNext()
  }

  /**
   * 暂停播放
   */
  pause() {
    if (!this.isPlaying) return
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
    if (index < 0 || index >= this.dialogues.length) return

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
    } else if (!this.isPlaying) {
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
    const voicesAreSame =
      this.config.voiceA === this.config.voiceB ||
      this.config.voiceA?.name === this.config.voiceB?.name

    if (voicesAreSame) {
      // 如果只有一个语音可用，通过音高和语速来区分
      utterance.rate = isHostA
        ? this.config.rate * 1.05
        : this.config.rate * 0.95
      utterance.pitch = isHostA
        ? this.config.pitch * 0.85
        : this.config.pitch * 1.2
    } else {
      // 有不同语音时，轻微调整即可
      utterance.rate = this.config.rate
      utterance.pitch = isHostA ? this.config.pitch : this.config.pitch * 1.1
    }

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

// AI 风格标签

/**
 * 风格标签响应
 */
export interface StyleTagsResponse {
  success: boolean
  tags: string[]
  from_cache: boolean
  error?: string
}

/**
 * 生成订阅源的 AI 风格标签
 * 基于最近 10 篇文章的标题和前 100 字正文分析
 */
export async function generateStyleTags(
  sourceId: number,
): Promise<StyleTagsResponse> {
  return request<StyleTagsResponse>(`/sources/${sourceId}/style-tags`, {
    method: 'POST',
  })
}

// 文本处理工具

/**
 * 在 HTML 中高亮注释词汇
 *
 * 规则：
 * 1. 每个注释词汇只标记第一次出现
 * 2. 指代类型(reference)优先匹配有上下文的位置，找不到则标记第一次出现
 * 3. 使用主题适配的 CSS 变量来控制颜色
 */
export function highlightAnnotations(
  html: string,
  annotations: AnnotationItem[],
): string {
  if (!annotations || annotations.length === 0) {
    console.debug('[Brewlia] No annotations to highlight')
    return html
  }

  console.debug('[Brewlia] Highlighting', annotations.length, 'annotations')

  // 按词汇长度降序排列（长的先匹配，避免短词覆盖长词）
  const sortedAnnotations = [...annotations].sort(
    (a, b) => b.term.length - a.term.length,
  )

  // 使用 DOMParser 而非 innerHTML 解析，避免脚本执行风险
  const domParser = new DOMParser()
  const parsedDoc = domParser.parseFromString(html, 'text/html')
  const div = parsedDoc.body
  const fullText = div.textContent || ''

  // 记录已标记的词汇（每个词只标记一次）
  const markedTerms = new Set<string>()
  let totalMatches = 0

  // 为指代类型找到最佳匹配位置（在纯文本中）
  const findBestPositionForReference = (
    term: string,
    contextHint?: string,
  ): number => {
    if (!contextHint) return fullText.indexOf(term)

    // 在全文中找到所有出现位置
    const positions: number[] = []
    let pos = fullText.indexOf(term, 0)
    while (pos !== -1) {
      positions.push(pos)
      pos = fullText.indexOf(term, pos + 1)
    }

    if (positions.length === 0) return -1
    if (positions.length === 1) return positions[0]

    // 从 context_hint 中提取关键词（去掉标点和短词）
    const contextKeywords = contextHint
      .replace(/[，。、！？：；"'（）[\]【】]/g, ' ')
      .split(/\s+/)
      .filter((k) => k.length >= 2)

    if (contextKeywords.length === 0) return positions[0]

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

    console.debug(
      `[Brewlia] Reference "${term}" best position: ${bestPos} (score: ${bestScore})`,
    )
    return bestPos
  }

  // 记录指代类型的最佳位置（基于纯文本偏移）
  const referencePositions = new Map<string, number>()
  for (const annotation of sortedAnnotations) {
    if (annotation.type === 'reference') {
      const bestPos = findBestPositionForReference(
        annotation.term,
        annotation.context_hint,
      )
      if (bestPos >= 0) {
        referencePositions.set(annotation.term, bestPos)
      }
    }
  }

  /**
   * 构造一个 <mark> 元素。
   *
   * 全程走 DOM API：`textContent` 与 `setAttribute` 不会把内容当 HTML 解析，
   * 因此 term / explanation 里出现什么字符都不可能变成标签或属性。
   */
  const buildMark = (
    annotation: AnnotationItem,
    matchedText: string,
    fallbackIndex: number,
  ): HTMLElement => {
    const mark = document.createElement('mark')
    mark.className = 'brewlia-annotation'
    const rawId = annotation.id || `${annotation.type}-${fallbackIndex}`
    mark.setAttribute('data-annotation-id', String(rawId).replace(/[^\w-]/g, ''))
    mark.setAttribute('data-type', annotation.type)
    mark.setAttribute('data-term', encodeURIComponent(annotation.term))
    mark.setAttribute(
      'data-explanation',
      encodeURIComponent(annotation.explanation),
    )
    mark.textContent = matchedText
    return mark
  }

  // 追踪当前处理到的文本偏移
  let currentTextOffset = 0

  /**
   * 处理单个文本节点。
   *
   * `allowAnyPosition` 为 true 时跳过 reference 的"最佳位置"约束，用于保底轮。
   *
   * 关键安全点：这里**不再**拼 HTML 字符串再赋给 innerHTML。
   * 原实现是 `let newHtml = node.textContent` → 拼接 → `span.innerHTML = newHtml`。
   * DOMParser 解析原文时已经把 `&lt;img …&gt;` 解码成了文本 `<img …>`，把它
   * 重新交给 innerHTML 等于二次解析，实体编码过的标签会复活成真元素 ——
   * 典型的 mutation XSS。改成拼 DOM 节点后，文本永远只是文本。
   */
  const processTextNode = (node: Node, allowAnyPosition: boolean): boolean => {
    const text = node.textContent || ''
    if (!text.trim()) {
      currentTextOffset += text.length
      return false
    }

    const nodeStartOffset = currentTextOffset
    currentTextOffset += text.length

    // 先在纯文本里收集互不重叠的命中区间，再一次性构造节点。
    // 这样就不需要原来那个「跳过 150 字符大约是 mark 标签长度」的偏移补偿。
    interface Hit {
      start: number
      end: number
      annotation: AnnotationItem
    }
    const hits: Hit[] = []
    const taken: boolean[] = Array.from({ length: text.length }, () => false)

    for (const annotation of sortedAnnotations) {
      if (!annotation.term || annotation.term.length === 0) continue

      const termKey = `${annotation.type}:${annotation.term}`
      if (markedTerms.has(termKey)) continue

      // 找到第一个还没被更长的词占用的位置
      let termIndex = text.indexOf(annotation.term)
      while (termIndex !== -1) {
        const overlaps = taken
          .slice(termIndex, termIndex + annotation.term.length)
          .some(Boolean)
        if (!overlaps) break
        termIndex = text.indexOf(annotation.term, termIndex + 1)
      }
      if (termIndex === -1) continue

      // 指代类型：只在最佳位置附近标记
      if (
        !allowAnyPosition &&
        annotation.type === 'reference' &&
        referencePositions.has(annotation.term)
      ) {
        const bestPos = referencePositions.get(annotation.term)!
        const actualPos = nodeStartOffset + termIndex

        const isOnlyOccurrence =
          !fullText.includes(annotation.term, bestPos + 1) &&
          !fullText.includes(annotation.term, bestPos - 1)

        if (!isOnlyOccurrence && Math.abs(actualPos - bestPos) > 150) {
          continue
        }
      }

      totalMatches++
      markedTerms.add(termKey)
      hits.push({
        start: termIndex,
        end: termIndex + annotation.term.length,
        annotation,
      })
      for (let i = termIndex; i < termIndex + annotation.term.length; i++) {
        taken[i] = true
      }
    }

    if (hits.length === 0 || !node.parentNode) return false

    hits.sort((a, b) => a.start - b.start)

    const fragment = document.createDocumentFragment()
    let cursor = 0
    for (const hit of hits) {
      if (hit.start > cursor) {
        fragment.appendChild(
          document.createTextNode(text.slice(cursor, hit.start)),
        )
      }
      fragment.appendChild(
        buildMark(hit.annotation, text.slice(hit.start, hit.end), totalMatches),
      )
      cursor = hit.end
    }
    if (cursor < text.length) {
      fragment.appendChild(document.createTextNode(text.slice(cursor)))
    }

    node.parentNode.replaceChild(fragment, node)
    return true
  }

  // 递归处理文本节点
  const processNode = (node: Node, allowAnyPosition = false) => {
    if (node.nodeType === Node.TEXT_NODE) {
      processTextNode(node, allowAnyPosition)
    } else if (node.nodeType === Node.ELEMENT_NODE) {
      const tagName = (node as Element).tagName.toLowerCase()
      if (!['script', 'style', 'mark', 'code', 'pre'].includes(tagName)) {
        const children = Array.from(node.childNodes)
        children.forEach((child) => processNode(child, allowAnyPosition))
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
    console.debug(
      '[Brewlia] Fallback pass for',
      unmarkedAnnotations.length,
      'unmarked annotations',
    )

    // 保底轮走同一条 DOM 路径，只是放开 reference 的"最佳位置"约束。
    //
    // 原实现在这里退回到「序列化成 HTML 字符串 → 正则替换 → 重新解析」。
    // 那个正则用 `(?<!<[^>]*)…(?![^<]*>)` 猜测"不在标签内"，词一旦出现在
    // 属性值里（例如上一轮写进 data-explanation 的内容）就会把 <mark> 插进
    // 属性中间，破坏结构。DOM 遍历天然只碰文本节点，没有这个问题。
    currentTextOffset = 0
    processNode(div, true)
  }

  console.debug('[Brewlia] Total matches found:', totalMatches)
  console.debug('[Brewlia] Marked terms:', Array.from(markedTerms))

  return div.innerHTML
}
