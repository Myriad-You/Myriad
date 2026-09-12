/** 内置 greeting/weather/quote/theme/music/notification；Tapp 为 `tapp-{id}`。 */

import type { TappInstance } from '../tapp/types'
import { getDefaultLocale } from '../i18n/locales'

export type BuiltinContentType =
  'greeting' | 'weather' | 'quote' | 'theme' | 'music' | 'notification'

export type DynamicContentType = BuiltinContentType | `tapp-${string}`

export interface DynamicContentItem {
  type: DynamicContentType
  icon: string
  text: string
  subtext?: string
  /** 越高越先显示，默认 0 */
  priority?: number
  showSubtext?: boolean
  onClick?: 'expand' | 'custom' | 'none'
  /** 仅 `onClick = 'custom'` */
  onClickHandler?: () => void
  /** unix ms */
  expiresAt?: number
  sourceTappId?: string
  i18n?: {
    text?: Record<string, string>
    subtext?: Record<string, string>
  }
}

export interface ContentProviderConfig {
  id: string
  name: string
  tappId?: string
  enabled: boolean
  /** ms */
  updateInterval?: number
  lastUpdate?: number
}

export interface ContentUpdateEvent {
  type: 'add' | 'update' | 'remove' | 'clear'
  providerId: string
  content?: DynamicContentItem
  contentType?: DynamicContentType
}

export type ContentUpdateListener = (event: ContentUpdateEvent) => void

class DynamicContentProviderService {
  private providers: Map<string, ContentProviderConfig> = new Map()
  private contents: Map<string, DynamicContentItem[]> = new Map()
  private listeners: Set<ContentUpdateListener> = new Set()
  private currentLocale: string = getDefaultLocale()

  constructor() {
    this.registerProvider({
      id: 'builtin',
      name: 'Built-in',
      enabled: true,
    })
  }

  registerProvider(config: ContentProviderConfig): void {
    this.providers.set(config.id, config)
    this.contents.set(config.id, [])
  }

  unregisterProvider(providerId: string): void {
    const contents = this.contents.get(providerId) ?? []
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

  getProvider(providerId: string): ContentProviderConfig | undefined {
    return this.providers.get(providerId)
  }

  getAllProviders(): ContentProviderConfig[] {
    return Iterator.from(this.providers.values()).toArray()
  }

  setLocale(locale: string): void {
    this.currentLocale = locale
  }

  getLocale(): string {
    return this.currentLocale
  }

  setContent(providerId: string, content: DynamicContentItem): void {
    const provider = this.providers.get(providerId)
    if (!provider || !provider.enabled) {
      console.warn(
        `[DynamicContentProvider] Provider ${providerId} not found or disabled`,
      )
      return
    }

    const contents = this.contents.get(providerId) ?? []
    const existingIndex = contents.findIndex((c) => c.type === content.type)

    if (existingIndex >= 0) {
      contents[existingIndex] = content
      this.notifyListeners({
        type: 'update',
        providerId,
        content,
      })
    } else {
      contents.push(content)
      this.notifyListeners({
        type: 'add',
        providerId,
        content,
      })
    }

    this.contents.set(providerId, contents)

    provider.lastUpdate = Date.now()
  }

  removeContent(providerId: string, contentType: DynamicContentType): void {
    const contents = this.contents.get(providerId)
    if (!contents) return

    const index = contents.findIndex((c) => c.type === contentType)
    if (index >= 0) {
      this.contents.set(providerId, contents.toSpliced(index, 1))
      this.notifyListeners({
        type: 'remove',
        providerId,
        contentType,
      })
    }
  }

  clearProviderContents(providerId: string): void {
    this.contents.set(providerId, [])
    this.notifyListeners({
      type: 'clear',
      providerId,
    })
  }

  getProviderContents(providerId: string): DynamicContentItem[] {
    return this.contents.get(providerId) ?? []
  }

  getAllContents(): DynamicContentItem[] {
    const now = Date.now()
    const allContents: DynamicContentItem[] = []

    for (const [providerId, contents] of this.contents.entries()) {
      const provider = this.providers.get(providerId)
      if (!provider?.enabled) continue

      for (const content of contents) {
        if (content.expiresAt && content.expiresAt < now) continue
        allContents.push(this.localizeContent(content))
      }
    }

    return allContents.toSorted((a, b) => (b.priority || 0) - (a.priority || 0))
  }

  /** 未命中当前语言则 en-US，再原文。 */
  private localizeContent(content: DynamicContentItem): DynamicContentItem {
    if (!content.i18n) return content

    const localized = { ...content }
    const locale = this.currentLocale

    if (content.i18n.text) {
      localized.text =
        content.i18n.text[locale] || content.i18n.text['en-US'] || content.text
    }

    if (content.i18n.subtext) {
      localized.subtext =
        content.i18n.subtext[locale] ||
        content.i18n.subtext['en-US'] ||
        content.subtext
    }

    return localized
  }

  addListener(listener: ContentUpdateListener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  removeListener(listener: ContentUpdateListener): void {
    this.listeners.delete(listener)
  }

  private notifyListeners(event: ContentUpdateEvent): void {
    for (const listener of this.listeners) {
      try {
        listener(event)
      } catch (error) {
        console.error('[DynamicContentProvider] Listener error:', error)
      }
    }
  }

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

  unregisterTappProvider(tappId: string): void {
    const providerId = `tapp-${tappId}`
    this.unregisterProvider(providerId)
  }

  setTappContent(
    tappId: string,
    content: Omit<DynamicContentItem, 'sourceTappId'>,
  ): void {
    const providerId = `tapp-${tappId}`

    if (!this.providers.has(providerId)) {
      this.registerProvider({
        id: providerId,
        name: `Tapp: ${tappId}`,
        tappId,
        enabled: true,
      })
    }

    const tappContent: DynamicContentItem = {
      ...content,
      type: content.type.startsWith('tapp-')
        ? content.type
        : (`tapp-${tappId}` as DynamicContentType),
      sourceTappId: tappId,
      priority: content.priority ?? -1, // Tapp 默认低于内置
    }

    this.setContent(providerId, tappContent)
  }

  removeTappContent(tappId: string): void {
    const providerId = `tapp-${tappId}`
    const contentType = `tapp-${tappId}` as DynamicContentType
    this.removeContent(providerId, contentType)
  }

  getTappContent(tappId: string): DynamicContentItem | undefined {
    const providerId = `tapp-${tappId}`
    const contents = this.contents.get(providerId) ?? []
    return contents.find((c) => c.sourceTappId === tappId)
  }

  /** 默认仅 weather/theme；tapp 有 subtext 才显示。 */
  shouldShowSubtext(content: DynamicContentItem): boolean {
    if (content.showSubtext !== undefined) {
      return content.showSubtext
    }

    const typeWithSubtext: DynamicContentType[] = ['weather', 'theme']

    if (content.type.startsWith('tapp-')) {
      return !!content.subtext
    }

    return typeWithSubtext.includes(content.type as BuiltinContentType)
  }

  getSafeText(
    content: DynamicContentItem | null | undefined,
    fallback: string = '',
  ): string {
    if (!content) return fallback
    return content.text || fallback
  }

  getSafeSubtext(
    content: DynamicContentItem | null | undefined,
    fallback?: string,
  ): string | undefined {
    if (!content) return fallback
    if (!this.shouldShowSubtext(content)) return undefined
    return content.subtext || fallback
  }
}

export const dynamicContentProvider = new DynamicContentProviderService()

export function getDynamicContentProvider(): DynamicContentProviderService {
  return dynamicContentProvider
}

export default dynamicContentProvider
