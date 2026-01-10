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

// ============ 类型定义 ============

/** 渲染模式 */
export type RenderMode = 'widget' | 'page'

/** Widget 资源（特化类型） */
export interface WidgetResources {
  /** 核心 JS 代码 */
  core: string
  /** Widget 专用 JS 代码 */
  widget?: string
  /** Widget HTML 模板（已按尺寸选择） */
  html: string
  /** 自定义 CSS（通用样式或 Widget 专用样式） */
  styles?: string
  /** Widget 专用 Tailwind CSS */
  css: string
  /** Widget 尺寸 */
  size: string
  /** CSS 架构模式 */
  cssMode?: 'unified' | 'separated'
}

/** Page 资源（特化类型） */
export interface PageResources {
  /** 核心 JS 代码 */
  core: string
  /** Page 专用 JS 代码 */
  page?: string
  /** Page HTML 模板 */
  html?: string
  /** 自定义 CSS（通用样式或 Page 专用样式） */
  styles?: string
  /** Page 专用 Tailwind CSS */
  css: string
  /** CSS 架构模式 */
  cssMode?: 'unified' | 'separated'
}

/** 分离式 CSS 结构 */
export interface SeparatedCSS {
  /** Widget 专用 CSS */
  widget: string
  /** Page 专用 CSS */
  page: string
  /** 共享 CSS（core + styles 中的类） */
  shared: string
}

/** 资源缓存项 */
interface ResourceCacheEntry<T> {
  data: T
  timestamp: number
  ttl: number
}

// ============ 缓存配置 ============

const CACHE_TTL = {
  widget: 5 * 60 * 1000, // Widget 资源 5 分钟
  page: 10 * 60 * 1000, // Page 资源 10 分钟
  css: 30 * 60 * 1000, // CSS 30 分钟
  separatedCss: 60 * 60 * 1000, // 分离 CSS 1 小时
}

// ============ 请求去重器 ============

class RequestDeduplicator {
  private pending = new Map<string, Promise<unknown>>()

  async dedupe<T>(key: string, factory: () => Promise<T>): Promise<T> {
    const existing = this.pending.get(key)
    if (existing)
      return existing as Promise<T>

    const promise = factory().finally(() => this.pending.delete(key))
    this.pending.set(key, promise)
    return promise
  }
}

// ============ CSS 分离器 ============

/**
 * CSS 分离工具
 *
 * 负责将源码按模式分离，生成独立的 CSS
 */
class CssSeparator {
  /**
   * 为 Widget 模式生成 CSS
   * 只分析 Widget 相关的源码
   */
  static generateWidgetCSS(
    core: string,
    widgetCode: string | undefined,
    widgetHtml: string,
    styles: string | undefined,
  ): string {
    const sources = [
      core || '',
      widgetCode || '',
      widgetHtml || '',
      styles || '',
    ].join('\n')

    return generateOnDemandTailwindCSS(sources)
  }

  /**
   * 为 Page 模式生成 CSS
   * 只分析 Page 相关的源码
   */
  static generatePageCSS(
    core: string,
    pageCode: string | undefined,
    pageHtml: string | undefined,
    styles: string | undefined,
  ): string {
    const sources = [
      core || '',
      pageCode || '',
      pageHtml || '',
      styles || '',
    ].join('\n')

    return generateOnDemandTailwindCSS(sources)
  }
}

// ============ 资源加载器类 ============

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

  /** Widget 资源缓存 (key: tappId:size) */
  private widgetCache = new Map<string, ResourceCacheEntry<WidgetResources>>()

  /** Page 资源缓存 (key: tappId) */
  private pageCache = new Map<string, ResourceCacheEntry<PageResources>>()

  /** 原始资源缓存（从 API 获取，多个尺寸共享） */
  private rawResourceCache = new Map<string, ResourceCacheEntry<TappApiService.TappResources>>()

  /** Widget CSS 缓存 (key: tappId:size) */
  private widgetCssCache = new Map<string, ResourceCacheEntry<string>>()

  /** Page CSS 缓存 (key: tappId) */
  private pageCssCache = new Map<string, ResourceCacheEntry<string>>()

  /** 请求去重器 */
  private deduplicator = new RequestDeduplicator()

  private constructor() {}

  static getInstance(): TappResourceLoader {
    if (!TappResourceLoader.instance) {
      TappResourceLoader.instance = new TappResourceLoader()
    }
    return TappResourceLoader.instance
  }

  // ============ Widget 资源加载 ============

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
  ): Promise<WidgetResources> {
    const cacheKey = `${tappInstance.id}:${size}`

    // 检查缓存
    const cached = this.widgetCache.get(cacheKey)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }

    // 使用请求去重
    return this.deduplicator.dedupe(`widget:${cacheKey}`, async () => {
      // 获取原始资源
      const raw = await this.fetchRawResources(tappInstance.id)

      // 提取代码
      const coreCode = this.extractCoreCode(raw.code)
      const widgetCode = this.extractModeCode(raw.code, 'widget')

      // 选择对应尺寸的 HTML 模板
      let html = ''
      if (raw.widgetTemplates) {
        // 优先使用精确匹配的尺寸
        html = raw.widgetTemplates[size] || ''

        // 如果没有精确匹配，尝试使用默认模板
        if (!html) {
          const defaultKey = Object.keys(raw.widgetTemplates)[0]
          if (defaultKey) {
            html = raw.widgetTemplates[defaultKey]
          }
        }
      }

      // 🎯 确定 CSS 架构模式和样式来源
      const cssMode = raw.cssMode || 'unified'
      // 分离模式：合并 styles（共享）+ widgetStyles（专用）
      // 统一模式：仅使用 styles
      let effectiveStyles: string | undefined
      if (cssMode === 'separated') {
        // 混合模式：共享样式 + Widget 专用样式
        const parts: string[] = []
        if (raw.styles)
          parts.push(raw.styles)
        if (raw.widgetStyles)
          parts.push(raw.widgetStyles)
        effectiveStyles = parts.length > 0 ? parts.join('\n') : undefined
      }
      else {
        effectiveStyles = raw.styles
      }

      // 🎯 生成 Widget 专用 CSS
      // 分离模式：widgetStyles（原生 CSS）+ Tailwind CSS 合并
      // 统一模式：从源码提取 Tailwind CSS
      const tailwindCSS = await this.ensureWidgetCSS(
        tappInstance.id,
        size,
        coreCode,
        widgetCode,
        html,
        effectiveStyles,
        raw.widgetCSS, // 使用后端预分离的 Widget CSS
      )
      // 合并：原生 CSS 在前，Tailwind 在后（Tailwind 可覆盖）
      const css = cssMode === 'separated' && effectiveStyles
        ? `${effectiveStyles}\n${tailwindCSS}`
        : tailwindCSS

      const resources: WidgetResources = {
        core: coreCode,
        widget: widgetCode,
        html,
        styles: effectiveStyles,
        css,
        size,
        cssMode,
      }

      // 存入缓存
      this.widgetCache.set(cacheKey, {
        data: resources,
        timestamp: Date.now(),
        ttl: CACHE_TTL.widget,
      })

      return resources
    })
  }

  // ============ Page 资源加载 ============

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

    // 检查缓存
    const cached = this.pageCache.get(cacheKey)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }

    // 使用请求去重
    return this.deduplicator.dedupe(`page:${cacheKey}`, async () => {
      // 获取原始资源
      const raw = await this.fetchRawResources(tappInstance.id)

      // 提取代码
      const coreCode = this.extractCoreCode(raw.code)
      const pageCode = this.extractModeCode(raw.code, 'page')

      // 🎯 确定 CSS 架构模式和样式来源
      const cssMode = raw.cssMode || 'unified'
      // 分离模式：合并 styles（共享）+ pageStyles（专用）
      // 统一模式：仅使用 styles
      let effectiveStyles: string | undefined
      if (cssMode === 'separated') {
        // 混合模式：共享样式 + Page 专用样式
        const parts: string[] = []
        if (raw.styles)
          parts.push(raw.styles)
        if (raw.pageStyles)
          parts.push(raw.pageStyles)
        effectiveStyles = parts.length > 0 ? parts.join('\n') : undefined
      }
      else {
        effectiveStyles = raw.styles
      }

      // 🎯 生成 Page 专用 CSS
      // 分离模式：pageStyles（原生 CSS）+ Tailwind CSS 合并
      // 统一模式：从源码提取 Tailwind CSS
      const tailwindCSS = await this.ensurePageCSS(
        tappInstance.id,
        coreCode,
        pageCode,
        raw.pageTemplate,
        effectiveStyles,
        raw.pageCSS, // 使用后端预分离的 Page CSS
      )
      // 合并：原生 CSS 在前，Tailwind 在后（Tailwind 可覆盖）
      const css = cssMode === 'separated' && effectiveStyles
        ? `${effectiveStyles}\n${tailwindCSS}`
        : tailwindCSS

      const resources: PageResources = {
        core: coreCode,
        page: pageCode,
        html: raw.pageTemplate,
        styles: effectiveStyles,
        css,
        cssMode,
      }

      // 存入缓存
      this.pageCache.set(cacheKey, {
        data: resources,
        timestamp: Date.now(),
        ttl: CACHE_TTL.page,
      })

      return resources
    })
  }

  // ============ CSS 处理（分离式） ============

  /**
   * 确保有 Widget 专用的 CSS
   *
   * 策略（优先级从高到低）：
   * 1. 使用后端预分离的 Widget CSS（widget.css）
   * 2. 前端从源码生成 Widget 专用 CSS
   */
  private async ensureWidgetCSS(
    tappId: string,
    size: string,
    coreCode: string,
    widgetCode: string | undefined,
    widgetHtml: string,
    styles: string | undefined,
    precompiledWidgetCSS: string | undefined,
  ): Promise<string> {
    const cacheKey = `${tappId}:${size}`

    // 检查缓存
    const cached = this.widgetCssCache.get(cacheKey)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }

    let css: string

    // 🎯 策略 1: 优先使用后端预分离的 Widget CSS
    if (precompiledWidgetCSS && precompiledWidgetCSS.length > 50) {
      css = precompiledWidgetCSS
    }
    else {
      // 策略 2: 从源码生成 Widget 专用 CSS
      css = CssSeparator.generateWidgetCSS(coreCode, widgetCode, widgetHtml, styles)
    }

    // 缓存
    this.widgetCssCache.set(cacheKey, {
      data: css,
      timestamp: Date.now(),
      ttl: CACHE_TTL.separatedCss,
    })

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
    coreCode: string,
    pageCode: string | undefined,
    pageHtml: string | undefined,
    styles: string | undefined,
    precompiledPageCSS: string | undefined,
  ): Promise<string> {
    const cacheKey = tappId

    // 检查缓存
    const cached = this.pageCssCache.get(cacheKey)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }

    let css: string

    // 🎯 策略 1: 优先使用后端预分离的 Page CSS
    if (precompiledPageCSS && precompiledPageCSS.length > 50) {
      css = precompiledPageCSS
    }
    else {
      // 策略 2: 从源码生成 Page 专用 CSS
      css = CssSeparator.generatePageCSS(coreCode, pageCode, pageHtml, styles)
    }

    // 缓存
    this.pageCssCache.set(cacheKey, {
      data: css,
      timestamp: Date.now(),
      ttl: CACHE_TTL.separatedCss,
    })

    return css
  }

  // ============ 原始资源获取 ============

  /**
   * 获取原始资源（带缓存）
   */
  private async fetchRawResources(tappId: string): Promise<TappApiService.TappResources> {
    const cached = this.rawResourceCache.get(tappId)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }

    return this.deduplicator.dedupe(`raw:${tappId}`, async () => {
      try {
        const resources = await TappApiService.getTappResources(tappId)

        this.rawResourceCache.set(tappId, {
          data: resources,
          timestamp: Date.now(),
          ttl: CACHE_TTL.widget, // 使用较短的 TTL
        })

        return resources
      }
      catch {
        // 回退到只获取代码
        const code = await TappApiService.getTappCode(tappId)
        return { code }
      }
    })
  }

  // ============ 代码提取 ============

  /**
   * 提取核心代码（去除 widget/page 专用部分）
   */
  private extractCoreCode(fullCode: string): string {
    // 查找分隔标记
    const widgetMarker = '// ========== Widget Code =========='
    const pageMarker = '// ========== Page Code =========='

    let code = fullCode

    // 移除 Widget 代码部分
    const widgetIdx = code.indexOf(widgetMarker)
    if (widgetIdx !== -1) {
      const pageIdx = code.indexOf(pageMarker, widgetIdx)
      if (pageIdx !== -1) {
        code = code.substring(0, widgetIdx) + code.substring(pageIdx)
      }
      else {
        code = code.substring(0, widgetIdx)
      }
    }

    // 移除 Page 代码部分
    const pageOnlyIdx = code.indexOf(pageMarker)
    if (pageOnlyIdx !== -1) {
      code = code.substring(0, pageOnlyIdx)
    }

    return code.trim()
  }

  /**
   * 提取特定模式的代码
   */
  private extractModeCode(fullCode: string, mode: 'widget' | 'page'): string | undefined {
    const marker = mode === 'widget'
      ? '// ========== Widget Code =========='
      : '// ========== Page Code =========='

    const startIdx = fullCode.indexOf(marker)
    if (startIdx === -1)
      return undefined

    const codeStart = startIdx + marker.length

    // 查找下一个标记或结尾
    const otherMarker = mode === 'widget'
      ? '// ========== Page Code =========='
      : '// ========== Widget Code =========='

    const endIdx = fullCode.indexOf(otherMarker, codeStart)

    if (endIdx !== -1) {
      return fullCode.substring(codeStart, endIdx).trim()
    }

    return fullCode.substring(codeStart).trim()
  }

  // ============ 缓存管理 ============

  /**
   * 清除指定 Tapp 的缓存
   */
  clearCache(tappId?: string): void {
    if (tappId) {
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
      this.pageCache.delete(tappId)
      this.pageCssCache.delete(tappId)
      this.rawResourceCache.delete(tappId)
    }
    else {
      // 清除所有缓存
      this.widgetCache.clear()
      this.pageCache.clear()
      this.rawResourceCache.clear()
      this.widgetCssCache.clear()
      this.pageCssCache.clear()
    }
  }

  /**
   * 获取缓存统计
   */
  getCacheStats(): {
    widget: number
    page: number
    raw: number
    widgetCss: number
    pageCss: number
  } {
    return {
      widget: this.widgetCache.size,
      page: this.pageCache.size,
      raw: this.rawResourceCache.size,
      widgetCss: this.widgetCssCache.size,
      pageCss: this.pageCssCache.size,
    }
  }

  /**
   * 预加载 Widget 资源（后台执行，不阻塞）
   */
  prefetchWidgetResources(tappInstance: TappInstance, size: string): void {
    this.loadWidgetResources(tappInstance, size).catch(() => {
      // 静默失败，预加载不影响主流程
    })
  }

  /**
   * 预加载 Page 资源（后台执行，不阻塞）
   */
  prefetchPageResources(tappInstance: TappInstance): void {
    this.loadPageResources(tappInstance).catch(() => {
      // 静默失败
    })
  }

  /**
   * 获取 Widget CSS（仅 CSS，不加载其他资源）
   * 用于需要提前获取 CSS 但不需要其他资源的场景
   */
  async getWidgetCSSOnly(
    tappId: string,
    size: string,
  ): Promise<string | null> {
    const cacheKey = `${tappId}:${size}`
    const cached = this.widgetCssCache.get(cacheKey)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }
    return null
  }

  /**
   * 获取 Page CSS（仅 CSS）
   */
  async getPageCSSOnly(tappId: string): Promise<string | null> {
    const cached = this.pageCssCache.get(tappId)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }
    return null
  }
}

// ============ 导出便捷函数 ============

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
): Promise<WidgetResources> {
  return getResourceLoader().loadWidgetResources(tappInstance, size)
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
