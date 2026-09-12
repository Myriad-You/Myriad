import type { TappInstance } from '../../types'
import * as TappApiService from '../../services/TappApiService'
import { generateOnDemandTailwindCSS } from './styles'

export interface CoreResources {
  modules: Record<string, string>
  moduleResolutions?: Record<string, Record<string, string>>
  coreEntry?: string
  i18n?: Record<string, unknown>
}

export interface WidgetResources {
  modules: Record<string, string>
  moduleResolutions?: Record<string, Record<string, string>>
  coreEntry?: string
  widgetEntries?: Record<string, string>
  html: string
  /** 作者样式（core + 该 widget 层） */
  styles?: string
  /** 宿主预编译 Tailwind */
  css: string
  size: string
  i18n?: Record<string, unknown>
}

export interface PageResources {
  modules: Record<string, string>
  moduleResolutions?: Record<string, Record<string, string>>
  coreEntry?: string
  pageEntry?: string
  html?: string
  /** 作者样式（core + page 层） */
  styles?: string
  /** 宿主预编译 Tailwind */
  css: string
  i18n?: Record<string, unknown>
}

interface ResourceCacheEntry<T> {
  data: T
  timestamp: number
  ttl: number
}

const CACHE_TTL = {
  widget: 5 * 60 * 1000,
  page: 10 * 60 * 1000,
  separatedCss: 60 * 60 * 1000,
}

const CACHE_LIMIT = {
  widget: 60,
  page: 30,
  raw: 30,
  widgetCss: 60,
  pageCss: 30,
} as const

function getCachedEntry<T>(
  cache: Map<string, ResourceCacheEntry<T>>,
  key: string,
): ResourceCacheEntry<T> | undefined {
  const entry = cache.get(key)
  if (!entry) return undefined
  if (Date.now() - entry.timestamp >= entry.ttl) {
    cache.delete(key)
    return undefined
  }
  cache.delete(key)
  cache.set(key, entry)
  return entry
}

function setCachedEntry<T>(
  cache: Map<string, ResourceCacheEntry<T>>,
  key: string,
  entry: ResourceCacheEntry<T>,
  maxEntries: number,
): void {
  cache.delete(key)
  cache.set(key, entry)
  while (cache.size > maxEntries) {
    const oldestKey = cache.keys().next().value
    if (oldestKey === undefined) break
    cache.delete(oldestKey)
  }
}

class RequestDeduplicator {
  private pending = new Map<string, Promise<unknown>>()

  async dedupe<T>(key: string, factory: () => Promise<T>): Promise<T> {
    const existing = this.pending.get(key)
    if (existing) return existing as Promise<T>

    const promise = factory().finally(() => this.pending.delete(key))
    this.pending.set(key, promise)
    return promise
  }
}

class CssSeparator {
  static generateLayerCSS(
    modules: Record<string, string>,
    html: string | undefined,
    styles: string | undefined,
  ): string {
    const sources = [...Object.values(modules), html || '', styles || ''].join(
      '\n',
    )

    return generateOnDemandTailwindCSS(sources)
  }
}

export class TappResourceLoader {
  private static instance: TappResourceLoader | null = null

  private widgetCache = new Map<string, ResourceCacheEntry<WidgetResources>>()

  private pageCache = new Map<string, ResourceCacheEntry<PageResources>>()

  private coreCache = new Map<string, ResourceCacheEntry<CoreResources>>()

  private rawResourceCache = new Map<
    string,
    ResourceCacheEntry<TappApiService.TappResources>
  >()

  private widgetCssCache = new Map<string, ResourceCacheEntry<string>>()

  private pageCssCache = new Map<string, ResourceCacheEntry<string>>()

  private deduplicator = new RequestDeduplicator()

  /** 每个 Tapp 的缓存代际；清缓存后旧请求不得回填新代际。 */
  private cacheGenerations = new Map<string, number>()

  private constructor() {}

  private generationFor(tappId: string): number {
    const current = this.cacheGenerations.get(tappId)
    if (current !== undefined) return current
    this.cacheGenerations.set(tappId, 0)
    return 0
  }

  private generationIsCurrent(tappId: string, generation: number): boolean {
    return this.generationFor(tappId) === generation
  }

  static getInstance(): TappResourceLoader {
    if (!TappResourceLoader.instance) {
      TappResourceLoader.instance = new TappResourceLoader()
    }
    return TappResourceLoader.instance
  }

  /** 后台只加载 core，不生成 Page CSS/HTML。 */
  async loadCoreResources(tappInstance: TappInstance): Promise<CoreResources> {
    const cacheKey = tappInstance.id
    const generation = this.generationFor(tappInstance.id)
    const cached = getCachedEntry(this.coreCache, cacheKey)
    if (cached) return cached.data

    return this.deduplicator.dedupe(
      `core:${cacheKey}:${generation}`,
      async () => {
        const raw = await this.fetchRawResources(tappInstance.id, 'core')
        if (!this.generationIsCurrent(tappInstance.id, generation)) {
          return this.loadCoreResources(tappInstance)
        }
        const resources: CoreResources = {
          modules: raw.modules,
          moduleResolutions: raw.moduleResolutions,
          coreEntry: raw.coreEntry,
          i18n: raw.i18n,
        }

        setCachedEntry(
          this.coreCache,
          cacheKey,
          {
            data: resources,
            timestamp: Date.now(),
            ttl: CACHE_TTL.page,
          },
          CACHE_LIMIT.page,
        )

        return resources
      },
    )
  }

  async loadWidgetResources(
    tappInstance: TappInstance,
    size: string,
    widgetId: string,
  ): Promise<WidgetResources> {
    const cacheKey = `${tappInstance.id}:${widgetId}:${size}`
    const generation = this.generationFor(tappInstance.id)

    const cached = getCachedEntry(this.widgetCache, cacheKey)
    if (cached) return cached.data

    return this.deduplicator.dedupe(
      `widget:${cacheKey}:${generation}`,
      async () => {
        const raw = await this.fetchRawResources(
          tappInstance.id,
          'widget',
          widgetId,
        )
        if (!this.generationIsCurrent(tappInstance.id, generation)) {
          return this.loadWidgetResources(tappInstance, size, widgetId)
        }

        let html = ''
        const templates = raw.widgetTemplates?.[widgetId]
        if (templates) {
          html = templates[size] || ''
          if (!html) {
            const defaultKey = Object.keys(templates)[0]
            if (defaultKey) html = templates[defaultKey]
          }
        }

        const styleParts = [
          raw.coreStyles,
          raw.widgetStyles?.[widgetId],
        ].filter((part): part is string => !!part)
        const effectiveStyles =
          styleParts.length > 0 ? styleParts.join('\n') : undefined

        const css = await this.ensureWidgetCSS(
          tappInstance.id,
          widgetId,
          size,
          raw.modules,
          html,
          effectiveStyles,
          raw.widgetCSS,
        )

        const resources: WidgetResources = {
          modules: raw.modules,
          moduleResolutions: raw.moduleResolutions,
          coreEntry: raw.coreEntry,
          widgetEntries: raw.widgetEntries,
          html,
          styles: effectiveStyles,
          css,
          size,
          i18n: raw.i18n,
        }

        if (!this.generationIsCurrent(tappInstance.id, generation)) {
          return this.loadWidgetResources(tappInstance, size, widgetId)
        }

        setCachedEntry(
          this.widgetCache,
          cacheKey,
          {
            data: resources,
            timestamp: Date.now(),
            ttl: CACHE_TTL.widget,
          },
          CACHE_LIMIT.widget,
        )

        return resources
      },
    )
  }

  async loadPageResources(tappInstance: TappInstance): Promise<PageResources> {
    const cacheKey = tappInstance.id
    const generation = this.generationFor(tappInstance.id)

    const cached = getCachedEntry(this.pageCache, cacheKey)
    if (cached) return cached.data

    return this.deduplicator.dedupe(
      `page:${cacheKey}:${generation}`,
      async () => {
        const raw = await this.fetchRawResources(tappInstance.id, 'page')
        if (!this.generationIsCurrent(tappInstance.id, generation)) {
          return this.loadPageResources(tappInstance)
        }

        const styleParts = [raw.coreStyles, raw.pageStyles].filter(
          (part): part is string => !!part,
        )
        const effectiveStyles =
          styleParts.length > 0 ? styleParts.join('\n') : undefined

        const css = await this.ensurePageCSS(
          tappInstance.id,
          raw.modules,
          raw.pageTemplate,
          effectiveStyles,
          raw.pageCSS,
        )

        const resources: PageResources = {
          modules: raw.modules,
          moduleResolutions: raw.moduleResolutions,
          coreEntry: raw.coreEntry,
          pageEntry: raw.pageEntry,
          html: raw.pageTemplate,
          styles: effectiveStyles,
          css,
          i18n: raw.i18n,
        }

        if (!this.generationIsCurrent(tappInstance.id, generation)) {
          return this.loadPageResources(tappInstance)
        }

        setCachedEntry(
          this.pageCache,
          cacheKey,
          {
            data: resources,
            timestamp: Date.now(),
            ttl: CACHE_TTL.page,
          },
          CACHE_LIMIT.page,
        )

        return resources
      },
    )
  }

  private async ensureWidgetCSS(
    tappId: string,
    widgetId: string,
    size: string,
    modules: Record<string, string>,
    widgetHtml: string,
    styles: string | undefined,
    precompiledWidgetCSS: string | undefined,
  ): Promise<string> {
    const cacheKey = `${tappId}:${widgetId}:${size}`

    const cached = getCachedEntry(this.widgetCssCache, cacheKey)
    if (cached) return cached.data

    let css: string

    if (precompiledWidgetCSS !== undefined) {
      css = precompiledWidgetCSS
    } else {
      css = CssSeparator.generateLayerCSS(modules, widgetHtml, styles)
    }

    setCachedEntry(
      this.widgetCssCache,
      cacheKey,
      {
        data: css,
        timestamp: Date.now(),
        ttl: CACHE_TTL.separatedCss,
      },
      CACHE_LIMIT.widgetCss,
    )

    return css
  }

  private async ensurePageCSS(
    tappId: string,
    modules: Record<string, string>,
    pageHtml: string | undefined,
    styles: string | undefined,
    precompiledPageCSS: string | undefined,
  ): Promise<string> {
    const cacheKey = tappId

    const cached = getCachedEntry(this.pageCssCache, cacheKey)
    if (cached) return cached.data

    let css: string

    if (precompiledPageCSS !== undefined) {
      css = precompiledPageCSS
    } else {
      css = CssSeparator.generateLayerCSS(modules, pageHtml, styles)
    }

    setCachedEntry(
      this.pageCssCache,
      cacheKey,
      {
        data: css,
        timestamp: Date.now(),
        ttl: CACHE_TTL.separatedCss,
      },
      CACHE_LIMIT.pageCss,
    )

    return css
  }

  private async fetchRawResources(
    tappId: string,
    mode: TappApiService.TappResourceMode = 'full',
    widgetId?: string,
  ): Promise<TappApiService.TappResources> {
    const generation = this.generationFor(tappId)
    const cacheKey = `${tappId}:${mode}:${widgetId || ''}`
    const cached = getCachedEntry(this.rawResourceCache, cacheKey)
    if (cached) return cached.data

    return this.deduplicator.dedupe(
      `raw:${cacheKey}:${generation}`,
      async () => {
        // 契约不符时让 409 抛出，不换路把不受支持的包送进沙箱。
        const resources = await TappApiService.getTappResources(tappId, {
          mode,
          widgetId,
        })

        if (!this.generationIsCurrent(tappId, generation)) {
          return this.fetchRawResources(tappId, mode, widgetId)
        }

        setCachedEntry(
          this.rawResourceCache,
          cacheKey,
          {
            data: resources,
            timestamp: Date.now(),
            ttl: CACHE_TTL.widget,
          },
          CACHE_LIMIT.raw,
        )

        return resources
      },
    )
  }

  clearCache(tappId?: string): void {
    if (tappId) {
      this.cacheGenerations.set(tappId, this.generationFor(tappId) + 1)
      for (const key of this.widgetCache.keys()) {
        if (key.startsWith(`${tappId}:`)) {
          this.widgetCache.delete(key)
        }
      }
      for (const key of this.widgetCssCache.keys()) {
        if (key.startsWith(`${tappId}:`)) {
          this.widgetCssCache.delete(key)
        }
      }
      for (const key of this.rawResourceCache.keys()) {
        if (key === tappId || key.startsWith(`${tappId}:`)) {
          this.rawResourceCache.delete(key)
        }
      }
      this.coreCache.delete(tappId)
      this.pageCache.delete(tappId)
      this.pageCssCache.delete(tappId)
    } else {
      for (const tappId of this.cacheGenerations.keys()) {
        this.cacheGenerations.set(tappId, this.generationFor(tappId) + 1)
      }
      this.coreCache.clear()
      this.widgetCache.clear()
      this.pageCache.clear()
      this.rawResourceCache.clear()
      this.widgetCssCache.clear()
      this.pageCssCache.clear()
    }
  }
}

export function getResourceLoader(): TappResourceLoader {
  return TappResourceLoader.getInstance()
}

export async function loadWidgetResources(
  tappInstance: TappInstance,
  size: string,
  widgetId: string,
): Promise<WidgetResources> {
  return getResourceLoader().loadWidgetResources(tappInstance, size, widgetId)
}

export async function loadCoreResources(
  tappInstance: TappInstance,
): Promise<CoreResources> {
  return getResourceLoader().loadCoreResources(tappInstance)
}

export async function loadPageResources(
  tappInstance: TappInstance,
): Promise<PageResources> {
  return getResourceLoader().loadPageResources(tappInstance)
}

export default TappResourceLoader
