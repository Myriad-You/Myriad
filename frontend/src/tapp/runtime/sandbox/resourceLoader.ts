/**
 * Tapp 资源加载器
 *
 * 🎯 设计目标：
 * - 统一的资源加载接口
 * - Widget 和 Page 完全分离处理
 * - CSS 从源头拆分：Widget CSS 和 Page CSS 独立生成
 * - 智能缓存和请求去重
 * - 真正的按需加载（只加载当前模式需要的资源）
 *
 * 架构说明：
 * - Widget 资源：需要按尺寸加载 HTML 模板，CSS 只包含 Widget 相关类
 * - Page 资源：单一 HTML 模板，CSS 只包含 Page 相关类
 * - CSS 策略：
 *   1. 分离式生成：Widget 和 Page 各自有独立的 CSS
 *   2. 后端预编译：优先使用后端存储的分离 CSS
 *   3. 前端降级：如果后端没有分离 CSS，前端按需生成
 *   4. 惰性更新：生成后异步同步到后端
 *
 * 🔒 性能优化：
 * - Widget CSS 只分析: widgetTemplates + widget code + styles
 * - Page CSS 只分析: pageTemplate + page code + styles
 * - 避免加载不必要的资源（Widget 不加载 Page 模板，反之亦然）
 */

import type { TappInstance } from '../../types'
import * as TappApiService from '../../services/TappApiService'
import { generateOnDemandTailwindCSS } from './styles'

// 类型定义

/** Headless core 资源（后台任务专用） */
export interface CoreResources {
  /** core 层依赖图内的模块：相对路径 → 源码 */
  modules: Record<string, string>
  moduleResolutions?: Record<string, Record<string, string>>
  coreEntry?: string
  /** core 可能使用的翻译数据 */
  i18n?: Record<string, unknown>
}

/** Widget 资源（特化类型） */
export interface WidgetResources {
  /** core + widget 层依赖图内的模块 */
  modules: Record<string, string>
  moduleResolutions?: Record<string, Record<string, string>>
  coreEntry?: string
  widgetEntries?: Record<string, string>
  /** Widget HTML 模板（已按尺寸选择） */
  html: string
  /** 作者样式：core 共享样式 + 该 widget 的层样式 */
  styles?: string
  /** 宿主预编译的 Widget Tailwind CSS */
  css: string
  /** Widget 尺寸 */
  size: string
  /** i18n 翻译数据（语言代码 → 键值对） */
  i18n?: Record<string, unknown>
}

/** Page 资源（特化类型） */
export interface PageResources {
  /** core + page 层依赖图内的模块 */
  modules: Record<string, string>
  moduleResolutions?: Record<string, Record<string, string>>
  coreEntry?: string
  pageEntry?: string
  /** Page HTML 模板 */
  html?: string
  /** 作者样式：core 共享样式 + page 层样式 */
  styles?: string
  /** 宿主预编译的 Page Tailwind CSS */
  css: string
  /** i18n 翻译数据（语言代码 → 键值对） */
  i18n?: Record<string, unknown>
}

/** 资源缓存项 */
interface ResourceCacheEntry<T> {
  data: T
  timestamp: number
  ttl: number
}

// 缓存配置

const CACHE_TTL = {
  widget: 5 * 60 * 1000, // Widget 资源 5 分钟
  page: 10 * 60 * 1000, // Page 资源 10 分钟
  separatedCss: 60 * 60 * 1000, // 分离 CSS 1 小时
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

// 请求去重器

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

// CSS 分离器

/**
 * CSS 分离工具
 *
 * 负责将源码按模式分离，生成独立的 CSS
 */
class CssSeparator {
  /**
   * 为某个模式生成 CSS。
   *
   * 扫描范围是该层依赖图内的全部模块，不只是入口——入口 require 进来的文件里
   * 一样会出现 class 名，只扫入口会漏掉它们。
   */
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

// 资源加载器类

/**
 * Tapp 资源加载器
 *
 * 负责加载和缓存 Tapp 的代码、CSS、HTML 模板等资源
 *
 * 🎯 核心特性：
 * - Widget 和 Page CSS 完全分离生成
 * - 按需加载：只加载当前模式需要的资源
 * - 智能缓存：分离的 CSS 独立缓存
 */
export class TappResourceLoader {
  private static instance: TappResourceLoader | null = null

  /** Widget 资源缓存 (key: tappId:widgetId:size) */
  private widgetCache = new Map<string, ResourceCacheEntry<WidgetResources>>()

  /** Page 资源缓存 (key: tappId) */
  private pageCache = new Map<string, ResourceCacheEntry<PageResources>>()

  /** Headless core 资源缓存 (key: tappId) */
  private coreCache = new Map<string, ResourceCacheEntry<CoreResources>>()

  /**
   * 原始资源缓存（从 API 获取）。
   * Key: `${tappId}:${mode}` — widget/page/full 投影互不污染。
   */
  private rawResourceCache = new Map<
    string,
    ResourceCacheEntry<TappApiService.TappResources>
  >()

  /** Widget CSS 缓存 (key: tappId:widgetId:size) */
  private widgetCssCache = new Map<string, ResourceCacheEntry<string>>()

  /** Page CSS 缓存 (key: tappId) */
  private pageCssCache = new Map<string, ResourceCacheEntry<string>>()

  /** 请求去重器 */
  private deduplicator = new RequestDeduplicator()

  /** 每个 Tapp 的缓存代际；清缓存后旧请求不得回填新代际。 */
  private cacheGenerations = new Map<string, number>()

  private constructor() {}

  private generationFor(tappId: string): number {
    const current = this.cacheGenerations.get(tappId)
    if (current !== undefined) return current
    // 记录所有启动过加载的 ID。这样 clearCache() 即使发生在首次请求尚未回填
    // 任意缓存时，也能提升其代际并阻止旧请求复活缓存。
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

  // Headless core 资源加载

  /**
   * 加载后台 core 运行所需的最小资源。
   *
   * 不生成 Page CSS、不保留 Page HTML / 模块，避免后台 runner 因复用
   * loadPageResources 而做无用的样式分析和页面缓存。
   */
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

  // Widget 资源加载

  /**
   * 加载 Widget 资源
   *
   * 🎯 按需加载策略：
   * - 只加载 Widget 需要的 HTML 模板
   * - CSS 只包含 Widget 相关的类
   * - 不加载 Page 相关的资源
   *
   * @param tappInstance - Tapp 实例
   * @param size - Widget 尺寸 (如 '2x2', '4x2')
   * @returns Widget 专用资源
   */
  async loadWidgetResources(
    tappInstance: TappInstance,
    size: string,
    widgetId: string,
  ): Promise<WidgetResources> {
    const cacheKey = `${tappInstance.id}:${widgetId}:${size}`
    const generation = this.generationFor(tappInstance.id)

    // 检查缓存
    const cached = getCachedEntry(this.widgetCache, cacheKey)
    if (cached) return cached.data

    // 使用请求去重
    return this.deduplicator.dedupe(
      `widget:${cacheKey}:${generation}`,
      async () => {
        // Widget 投影：跳过 page 模板/模块/CSS，减小传输与解析开销
        const raw = await this.fetchRawResources(
          tappInstance.id,
          'widget',
          widgetId,
        )
        if (!this.generationIsCurrent(tappInstance.id, generation)) {
          return this.loadWidgetResources(tappInstance, size, widgetId)
        }

        // 选择对应尺寸的 HTML 模板
        let html = ''
        const templates = raw.widgetTemplates?.[widgetId]
        if (templates) {
          html = templates[size] || ''
          if (!html) {
            const defaultKey = Object.keys(templates)[0]
            if (defaultKey) html = templates[defaultKey]
          }
        }

        // 作者样式：core 共享层 + 该 widget 的层样式。层没声明专用样式时，
        // 结果与过去的 unified 模式一致，不需要额外的模式开关。
        const styleParts = [
          raw.coreStyles,
          raw.widgetStyles?.[widgetId],
        ].filter((part): part is string => !!part)
        const effectiveStyles =
          styleParts.length > 0 ? styleParts.join('\n') : undefined

        // 原生 CSS 由 styles 字段单独交给沙箱；这里仅处理 Tailwind CSS。
        const css = await this.ensureWidgetCSS(
          tappInstance.id,
          widgetId,
          size,
          raw.modules,
          html,
          effectiveStyles,
          raw.widgetCSS, // 使用宿主预编译的 Widget CSS
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

        // 存入缓存
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

  // Page 资源加载

  /**
   * 加载 Page 资源
   *
   * 🎯 按需加载策略：
   * - 只加载 Page 需要的 HTML 模板
   * - CSS 只包含 Page 相关的类
   * - 不加载 Widget 相关的资源
   *
   * @param tappInstance - Tapp 实例
   * @returns Page 专用资源
   */
  async loadPageResources(tappInstance: TappInstance): Promise<PageResources> {
    const cacheKey = tappInstance.id
    const generation = this.generationFor(tappInstance.id)

    // 检查缓存
    const cached = getCachedEntry(this.pageCache, cacheKey)
    if (cached) return cached.data

    // 使用请求去重
    return this.deduplicator.dedupe(
      `page:${cacheKey}:${generation}`,
      async () => {
        // Page 投影：跳过 widget 模板/CSS
        const raw = await this.fetchRawResources(tappInstance.id, 'page')
        if (!this.generationIsCurrent(tappInstance.id, generation)) {
          return this.loadPageResources(tappInstance)
        }

        // 作者样式：core 共享层 + page 层。层没声明专用样式时与过去的
        // unified 模式等价，所以不再需要 cssMode 开关。
        const styleParts = [raw.coreStyles, raw.pageStyles].filter(
          (part): part is string => !!part,
        )
        const effectiveStyles =
          styleParts.length > 0 ? styleParts.join('\n') : undefined

        // 原生 CSS 由 styles 字段单独交给沙箱；这里仅处理 Tailwind CSS。
        const css = await this.ensurePageCSS(
          tappInstance.id,
          raw.modules,
          raw.pageTemplate,
          effectiveStyles,
          raw.pageCSS, // 使用宿主预编译的 Page CSS
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

        // 存入缓存
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

  // CSS 处理（分离式）

  /**
   * 确保有 Widget 专用的 CSS
   *
   * 策略（优先级从高到低）：
   * 1. 使用后端预分离的 Widget CSS（widget.css）
   * 2. 前端从源码生成 Widget 专用 CSS
   */
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

    // 检查缓存
    const cached = getCachedEntry(this.widgetCssCache, cacheKey)
    if (cached) return cached.data

    let css: string

    // 策略 1: 优先使用宿主预编译的 Widget CSS
    if (precompiledWidgetCSS !== undefined) {
      css = precompiledWidgetCSS
    } else {
      // 策略 2: 从该层依赖图内的全部模块生成
      css = CssSeparator.generateLayerCSS(modules, widgetHtml, styles)
    }

    // 缓存
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

  /**
   * 确保有 Page 专用的 CSS
   *
   * 策略（优先级从高到低）：
   * 1. 使用后端预分离的 Page CSS（page.css）
   * 2. 前端从源码生成 Page 专用 CSS
   */
  private async ensurePageCSS(
    tappId: string,
    modules: Record<string, string>,
    pageHtml: string | undefined,
    styles: string | undefined,
    precompiledPageCSS: string | undefined,
  ): Promise<string> {
    const cacheKey = tappId

    // 检查缓存
    const cached = getCachedEntry(this.pageCssCache, cacheKey)
    if (cached) return cached.data

    let css: string

    // 策略 1: 优先使用宿主预编译的 Page CSS
    if (precompiledPageCSS !== undefined) {
      css = precompiledPageCSS
    } else {
      // 策略 2: 从该层依赖图内的全部模块生成
      css = CssSeparator.generateLayerCSS(modules, pageHtml, styles)
    }

    // 缓存
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

  // 原始资源获取

  /**
   * 获取原始资源（带缓存，按 mode 投影）
   */
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
        // 没有旧端点回退：投影由后端按 manifest 层声明完成，包结构不符合
        // 当前契约时让 409 照常抛出，而不是换条路把旧格式送进沙箱。
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
            ttl: CACHE_TTL.widget, // 使用较短的 TTL
          },
          CACHE_LIMIT.raw,
        )

        return resources
      },
    )
  }

  // 缓存管理

  /**
   * 清除指定 Tapp 的缓存
   */
  clearCache(tappId?: string): void {
    if (tappId) {
      this.cacheGenerations.set(tappId, this.generationFor(tappId) + 1)
      // 清除该 Tapp 的所有缓存
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
      // 清除所有缓存
      this.coreCache.clear()
      this.widgetCache.clear()
      this.pageCache.clear()
      this.rawResourceCache.clear()
      this.widgetCssCache.clear()
      this.pageCssCache.clear()
    }
  }
}

// 导出便捷函数

/**
 * 获取资源加载器实例
 */
export function getResourceLoader(): TappResourceLoader {
  return TappResourceLoader.getInstance()
}

/**
 * 加载 Widget 资源（便捷函数）
 */
export async function loadWidgetResources(
  tappInstance: TappInstance,
  size: string,
  widgetId: string,
): Promise<WidgetResources> {
  return getResourceLoader().loadWidgetResources(tappInstance, size, widgetId)
}

/** 加载 Headless core 资源（便捷函数） */
export async function loadCoreResources(
  tappInstance: TappInstance,
): Promise<CoreResources> {
  return getResourceLoader().loadCoreResources(tappInstance)
}

/**
 * 加载 Page 资源（便捷函数）
 */
export async function loadPageResources(
  tappInstance: TappInstance,
): Promise<PageResources> {
  return getResourceLoader().loadPageResources(tappInstance)
}

export default TappResourceLoader
