/** 内置 greeting/weather/quote/theme/music/notification；Tapp 为 `tapp-{id}`。 */

import type { TappInstance } from '@myriad/tapp-runtime'
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

export class DynamicContentProviderService {
  private providers: Map<string, ContentProviderConfig> = new Map()
  private contents: Map<string, DynamicContentItem[]> = new Map()
  private revision = 0
  private subscribers = new Set<() => void>()
  private expiryTimer: ReturnType<typeof setTimeout> | null = null

  getSnapshot = (): number => this.revision

  /** Recheck wall-clock expiry after the browser has suspended timers. */
  refreshSnapshot(): void {
    this.publish()
  }

  subscribe = (callback: () => void): (() => void) => {
    this.subscribers.add(callback)
    this.scheduleExpiry()
    return () => {
      this.subscribers.delete(callback)
      this.scheduleExpiry()
    }
  }

  private publish() {
    // Commit all data before subscribers read their next snapshot.
    this.revision++
    this.scheduleExpiry()
    for (const callback of [...this.subscribers]) {
      if (!this.subscribers.has(callback)) continue
      try { callback() } catch (error) {
        console.error('[DynamicContentProvider] Subscriber error:', error)
      }
    }
  }

  private scheduleExpiry() {
    if (this.expiryTimer !== null) clearTimeout(this.expiryTimer)
    this.expiryTimer = null
    if (this.subscribers.size === 0) return
    const now = Date.now()
    let next = Infinity
    for (const [id, contents] of this.contents) {
      if (!this.providers.get(id)?.enabled) continue
      for (const content of contents) {
        if (content.expiresAt && content.expiresAt > now) next = Math.min(next, content.expiresAt)
      }
    }
    if (!Number.isFinite(next)) return
    this.expiryTimer = setTimeout(() => {
      this.expiryTimer = null
      this.publish()
    }, Math.min(next - now, 2_147_483_647))
  }

  private currentLocale: string = getDefaultLocale()

  constructor() {
    this.registerProvider({
      id: 'builtin',
      name: 'Built-in',
      enabled: true,
    })
  }

  registerProvider(config: ContentProviderConfig): void {
    this.providers.set(config.id, { ...config })
    this.contents.set(config.id, [])
    this.publish()
  }

  unregisterProvider(providerId: string): void {
    if (!this.providers.has(providerId)) return
    this.providers.delete(providerId)
    this.contents.delete(providerId)
    this.publish()
  }

  getProvider(providerId: string): ContentProviderConfig | undefined {
    const provider = this.providers.get(providerId)
    return provider ? { ...provider } : undefined
  }

  getAllProviders(): ContentProviderConfig[] {
    return [...this.providers.values()].map(provider => ({ ...provider }))
  }

  setLocale(locale: string): void {
    if (this.currentLocale === locale) return
    this.currentLocale = locale
    this.publish()
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

    const stored = copyContent(content)
    this.contents.set(providerId, existingIndex >= 0
      ? contents.toSpliced(existingIndex, 1, stored)
      : [...contents, stored])
    provider.lastUpdate = Date.now()
    this.publish()
  }

  removeContent(providerId: string, contentType: DynamicContentType): void {
    const contents = this.contents.get(providerId)
    if (!contents) return

    const index = contents.findIndex((c) => c.type === contentType)
    if (index >= 0) {
      this.contents.set(providerId, contents.toSpliced(index, 1))
      this.publish()
    }
  }

  clearProviderContents(providerId: string): void {
    if (!this.providers.has(providerId)) return
    this.contents.set(providerId, [])
    this.publish()
  }

  getProviderContents(providerId: string): DynamicContentItem[] {
    return (this.contents.get(providerId) ?? []).map(copyContent)
  }

  getAllContents(): DynamicContentItem[] {
    const now = Date.now()
    const allContents: DynamicContentItem[] = []

    for (const [providerId, contents] of this.contents.entries()) {
      const provider = this.providers.get(providerId)
      if (!provider?.enabled) continue

      for (const content of contents) {
        if (content.expiresAt && content.expiresAt <= now) continue
        allContents.push(this.localizeContent(content))
      }
    }

    return allContents.toSorted((a, b) => (b.priority || 0) - (a.priority || 0))
  }

  /** 未命中当前语言则 en-US，再原文。 */
  private localizeContent(content: DynamicContentItem): DynamicContentItem {
    if (!content.i18n) return { ...content }

    const localized = copyContent(content)
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
    const content = contents.find((c) => c.sourceTappId === tappId)
    return content ? copyContent(content) : undefined
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

function copyContent(content: DynamicContentItem): DynamicContentItem {
  return {
    ...content,
    ...(content.i18n ? { i18n: {
      ...(content.i18n.text ? { text: { ...content.i18n.text } } : {}),
      ...(content.i18n.subtext ? { subtext: { ...content.i18n.subtext } } : {}),
    } } : {}),
  }
}

export const dynamicContentProvider = new DynamicContentProviderService()

export function getDynamicContentProvider(): DynamicContentProviderService {
  return dynamicContentProvider
}

export default dynamicContentProvider
