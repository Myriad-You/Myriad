/**
 * 动态内容提供者服务
 *
 * 为右上角信息控制岛提供可扩展的动态内容注册能力，
 * 允许 Tapp 应用注册自定义动态内容显示。
 *
 * 内置类型：greeting, weather, quote, theme, music
 * Tapp 自定义类型：tapp-{tappId}
 */

import type { TappInstance } from '../tapp/types'

// ============ 类型定义 ============

/** 内置动态内容类型 */
export type BuiltinContentType = 'greeting' | 'weather' | 'quote' | 'theme' | 'music'

/** 动态内容类型（包含 Tapp 自定义类型） */
export type DynamicContentType = BuiltinContentType | `tapp-${string}`

/** 动态内容项 */
export interface DynamicContentItem {
  /** 内容类型 */
  type: DynamicContentType
  /** 图标（emoji 或图片 URL） */
  icon: string
  /** 主文本 */
  text: string
  /** 副文本（可选） */
  subtext?: string
  /** 优先级（越高越先显示，默认 0） */
  priority?: number
  /** 是否显示副文本（默认根据类型自动判断） */
  showSubtext?: boolean
  /** 点击行为（可选） */
  onClick?: 'expand' | 'custom' | 'none'
  /** 自定义点击处理器（仅 onClick = 'custom' 时有效） */
  onClickHandler?: () => void
  /** 过期时间（毫秒时间戳，可选） */
  expiresAt?: number
  /** 来源 Tapp ID（Tapp 注册的内容） */
  sourceTappId?: string
  /** 多语言文本映射 */
  i18n?: {
    text?: Record<string, string> // { 'zh-CN': '中文', 'en-US': 'English', 'ja-JP': '日本語' }
    subtext?: Record<string, string>
  }
}

/** 内容提供者配置 */
export interface ContentProviderConfig {
  /** 提供者 ID */
  id: string
  /** 提供者名称 */
  name: string
  /** 来源 Tapp */
  tappId?: string
  /** 是否启用 */
  enabled: boolean
  /** 更新间隔（毫秒） */
  updateInterval?: number
  /** 最后更新时间 */
  lastUpdate?: number
}

/** 内容更新事件 */
export interface ContentUpdateEvent {
  type: 'add' | 'update' | 'remove' | 'clear'
  providerId: string
  content?: DynamicContentItem
  contentType?: DynamicContentType
}

/** 内容更新监听器 */
export type ContentUpdateListener = (event: ContentUpdateEvent) => void

// ============ 动态内容提供者服务 ============

class DynamicContentProviderService {
  /** 已注册的内容提供者 */
  private providers: Map<string, ContentProviderConfig> = new Map()

  /** 各提供者的当前内容 */
  private contents: Map<string, DynamicContentItem[]> = new Map()

  /** 更新监听器 */
  private listeners: Set<ContentUpdateListener> = new Set()

  /** 当前语言 */
  private currentLocale: string = 'zh-CN'

  constructor() {
    // 注册内置提供者
    this.registerProvider({
      id: 'builtin',
      name: '内置内容',
      enabled: true,
    })
  }

  // ============ 提供者管理 ============

  /**
   * 注册内容提供者
   */
  registerProvider(config: ContentProviderConfig): void {
    this.providers.set(config.id, config)
    this.contents.set(config.id, [])
  }

  /**
   * 注销内容提供者
   */
  unregisterProvider(providerId: string): void {
    // 清除该提供者的所有内容
    const contents = this.contents.get(providerId) || []
    for (const content of contents) {
      this.notifyListeners({
        type: 'remove',
        providerId,
        contentType: content.type,
      })
    }

    this.providers.delete(providerId)
    this.contents.delete(providerId)
  }

  /**
   * 获取提供者配置
   */
  getProvider(providerId: string): ContentProviderConfig | undefined {
    return this.providers.get(providerId)
  }

  /**
   * 获取所有提供者
   */
  getAllProviders(): ContentProviderConfig[] {
    return Array.from(this.providers.values())
  }

  // ============ 内容管理 ============

  /**
   * 设置当前语言
   */
  setLocale(locale: string): void {
    this.currentLocale = locale
  }

  /**
   * 获取当前语言
   */
  getLocale(): string {
    return this.currentLocale
  }

  /**
   * 添加或更新动态内容
   */
  setContent(providerId: string, content: DynamicContentItem): void {
    const provider = this.providers.get(providerId)
    if (!provider || !provider.enabled) {
      console.warn(`[DynamicContentProvider] Provider ${providerId} not found or disabled`)
      return
    }

    const contents = this.contents.get(providerId) || []
    const existingIndex = contents.findIndex(c => c.type === content.type)

    if (existingIndex >= 0) {
      contents[existingIndex] = content
      this.notifyListeners({
        type: 'update',
        providerId,
        content,
      })
    }
    else {
      contents.push(content)
      this.notifyListeners({
        type: 'add',
        providerId,
        content,
      })
    }

    this.contents.set(providerId, contents)

    // 更新提供者最后更新时间
    provider.lastUpdate = Date.now()
  }

  /**
   * 移除动态内容
   */
  removeContent(providerId: string, contentType: DynamicContentType): void {
    const contents = this.contents.get(providerId)
    if (!contents)
      return

    const index = contents.findIndex(c => c.type === contentType)
    if (index >= 0) {
      contents.splice(index, 1)
      this.notifyListeners({
        type: 'remove',
        providerId,
        contentType,
      })
    }
  }

  /**
   * 清除提供者的所有内容
   */
  clearProviderContents(providerId: string): void {
    this.contents.set(providerId, [])
    this.notifyListeners({
      type: 'clear',
      providerId,
    })
  }

  /**
   * 获取提供者的内容
   */
  getProviderContents(providerId: string): DynamicContentItem[] {
    return this.contents.get(providerId) || []
  }

  /**
   * 获取所有内容（按优先级排序，已本地化）
   */
  getAllContents(): DynamicContentItem[] {
    const now = Date.now()
    const allContents: DynamicContentItem[] = []

    for (const [providerId, contents] of this.contents.entries()) {
      const provider = this.providers.get(providerId)
      if (!provider?.enabled)
        continue

      for (const content of contents) {
        // 过滤过期内容
        if (content.expiresAt && content.expiresAt < now)
          continue

        // 应用本地化
        const localizedContent = this.localizeContent(content)
        allContents.push(localizedContent)
      }
    }

    // 按优先级降序排序
    return allContents.sort((a, b) => (b.priority || 0) - (a.priority || 0))
  }

  /**
   * 应用本地化
   */
  private localizeContent(content: DynamicContentItem): DynamicContentItem {
    if (!content.i18n)
      return content

    const localized = { ...content }
    const locale = this.currentLocale

    // 尝试本地化 text
    if (content.i18n.text) {
      localized.text = content.i18n.text[locale]
        || content.i18n.text['en-US']
        || content.text
    }

    // 尝试本地化 subtext
    if (content.i18n.subtext) {
      localized.subtext = content.i18n.subtext[locale]
        || content.i18n.subtext['en-US']
        || content.subtext
    }

    return localized
  }

  // ============ 监听器管理 ============

  /**
   * 添加更新监听器
   */
  addListener(listener: ContentUpdateListener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  /**
   * 移除更新监听器
   */
  removeListener(listener: ContentUpdateListener): void {
    this.listeners.delete(listener)
  }

  /**
   * 通知所有监听器
   */
  private notifyListeners(event: ContentUpdateEvent): void {
    for (const listener of this.listeners) {
      try {
        listener(event)
      }
      catch (error) {
        console.error('[DynamicContentProvider] Listener error:', error)
      }
    }
  }

  // ============ Tapp 集成 ============

  /**
   * 为 Tapp 注册内容提供者
   */
  registerTappProvider(tappInstance: TappInstance): string {
    const providerId = `tapp-${tappInstance.id}`

    this.registerProvider({
      id: providerId,
      name: tappInstance.manifest.name,
      tappId: tappInstance.id,
      enabled: true,
    })

    return providerId
  }

  /**
   * 注销 Tapp 的内容提供者
   */
  unregisterTappProvider(tappId: string): void {
    const providerId = `tapp-${tappId}`
    this.unregisterProvider(providerId)
  }

  /**
   * Tapp 设置动态内容
   */
  setTappContent(tappId: string, content: Omit<DynamicContentItem, 'sourceTappId'>): void {
    const providerId = `tapp-${tappId}`

    // 确保提供者已注册
    if (!this.providers.has(providerId)) {
      this.registerProvider({
        id: providerId,
        name: `Tapp: ${tappId}`,
        tappId,
        enabled: true,
      })
    }

    // 设置内容类型为 tapp 类型
    const tappContent: DynamicContentItem = {
      ...content,
      type: content.type.startsWith('tapp-') ? content.type : `tapp-${tappId}` as DynamicContentType,
      sourceTappId: tappId,
      priority: content.priority ?? -1, // Tapp 内容默认低优先级
    }

    this.setContent(providerId, tappContent)
  }

  /**
   * Tapp 移除动态内容
   */
  removeTappContent(tappId: string): void {
    const providerId = `tapp-${tappId}`
    const contentType = `tapp-${tappId}` as DynamicContentType
    this.removeContent(providerId, contentType)
  }

  /**
   * 获取 Tapp 的当前内容
   */
  getTappContent(tappId: string): DynamicContentItem | undefined {
    const providerId = `tapp-${tappId}`
    const contents = this.contents.get(providerId) || []
    return contents.find(c => c.sourceTappId === tappId)
  }

  // ============ 辅助方法 ============

  /**
   * 判断内容类型是否应显示副文本
   */
  shouldShowSubtext(content: DynamicContentItem): boolean {
    // 显式指定时使用指定值
    if (content.showSubtext !== undefined) {
      return content.showSubtext
    }

    // 默认规则：天气和主题显示，问候语和一言不显示，Tapp 内容显示
    const typeWithSubtext: DynamicContentType[] = ['weather', 'theme']

    if (content.type.startsWith('tapp-')) {
      return !!content.subtext
    }

    return typeWithSubtext.includes(content.type as BuiltinContentType)
  }

  /**
   * 安全获取内容文本（处理空值）
   */
  getSafeText(content: DynamicContentItem | null | undefined, fallback: string = ''): string {
    if (!content)
      return fallback
    return content.text || fallback
  }

  /**
   * 安全获取内容副文本（处理空值）
   */
  getSafeSubtext(content: DynamicContentItem | null | undefined, fallback?: string): string | undefined {
    if (!content)
      return fallback
    if (!this.shouldShowSubtext(content))
      return undefined
    return content.subtext || fallback
  }
}

// ============ 单例导出 ============

/** 动态内容提供者服务单例 */
export const dynamicContentProvider = new DynamicContentProviderService()

/** 获取动态内容提供者服务 */
export function getDynamicContentProvider(): DynamicContentProviderService {
  return dynamicContentProvider
}

export default dynamicContentProvider
