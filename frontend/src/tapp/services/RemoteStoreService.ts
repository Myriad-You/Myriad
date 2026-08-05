/**
 * 远程应用商店服务
 * 从 GitHub 托管的远程商店获取和安装 Tapp
 *
 * 商店源配置存储在后端数据库中，通过 API 进行管理
 * 缓存仅保存在内存中，刷新页面后重新获取
 */

import type { TappManifest, TappManifestLocales } from '../types'
import type { StorePreviewDescriptor } from '../utils/storePreview'
import api from '../../lib/api'
import { TAPP_ICON_TOKENS } from '../constants/icons'
import {
  storeAssetStorePath,
  storePackageRoot,
} from '../utils/storePackagePaths'
import { parseStorePreview } from '../utils/storePreview'

export {
  storeAssetStorePath,
  storePackageRoot,
} from '../utils/storePackagePaths'
export type { StorePreviewDescriptor } from '../utils/storePreview'

// 类型定义

/** 远程商店源配置 */
export interface RemoteStoreSource {
  /** 数据库 ID */
  id?: number
  /** 商店名称 */
  name: string
  /** 商店描述 */
  description?: string
  /** 商店 URL（index.json 的 URL） */
  url: string
  enabled: boolean
  /** 是否为官方商店 */
  official?: boolean
  icon?: string
}

/** 远程商店索引 */
export interface RemoteStoreIndex {
  /** 商店名称 */
  name: string
  /** 商店描述 */
  description: string
  /** API 版本 */
  api_version: number
  /** 最后更新时间 */
  last_updated: string
  /** 基础 URL */
  base_url: string
  /** 应用列表 */
  apps: RemoteApp[]
  /** 分类列表 */
  categories?: RemoteCategory[]
}

/** 远程应用信息 */
export interface RemoteApp {
  /** 应用 ID */
  id: string
  /** 应用名称 */
  name: string
  /** 版本号 */
  version: string
  /** 简短描述 */
  description: string
  /** 详细描述（可选） */
  long_description?: string
  /**
   * name/description 的多语言覆盖（与 manifest.locales 同结构）。
   * 键为 BCP-47；未命中时回退顶层 name/description。
   */
  locales?: TappManifestLocales
  /** 作者 */
  author: {
    name: string
    email?: string
    url?: string
  }
  /** 图标（emoji 或 URL） */
  icon?: string
  /** 内联 SVG 图标代码（优先于 icon） */
  icon_svg?: string
  /**
   * 可选：全彩自定义图标仍保留 material 色壳（catalog → install manifest 同源语义）。
   * 缺省 false / 未声明 = auto（全彩图 standalone 铺满）。
   */
  icon_shell?: boolean
  /** 主题色（十六进制，如 #6366f1） */
  theme_color?: string
  /** 分类 */
  category: string
  tags?: string[]
  /** 所需权限 */
  permissions: string[]
  /** 下载链接 */
  download: {
    /** manifest.json URL（相对于 base_url） */
    manifest: string
    /** 代码文件 URL（相对于 base_url） */
    code: string
    /** README URL（可选） */
    readme?: string
    /** 统一样式 CSS（可选） */
    styles?: string
    /** Widget 样式 CSS */
    widget_styles?: string
    /** Page 样式 CSS */
    page_styles?: string
    /** Page 模板 HTML */
    page_template?: string
    /** Widget 模板（Widget ID → 尺寸） */
    widget_templates?: Record<string, Record<string, string>>
    /** i18n 翻译文件（lang → 相对路径） */
    i18n?: Record<string, string>
    /** Page 模块（filename → 相对路径） */
    page_modules?: Record<string, string>
  }
  /** 许可证 */
  license?: string
  /** 主页 URL */
  homepage?: string
  /** 仓库 URL */
  repository?: string
  /** 截图 URL 列表 */
  screenshots?: string[]
  /** 商店专用、无脚本的静态预览场景 */
  preview?: StorePreviewDescriptor
  /** 文件大小（字节） */
  size?: number
  /** 是否推荐应用 */
  featured?: boolean
  /** 是否官方验证 */
  verified?: boolean
  /** 安装次数（index 占位或 edge overlay） */
  downloads?: number
  created_at?: string
  updated_at?: string
}

/** 远程分类 */
export interface RemoteCategory {
  id: string
  name: string
  description?: string
  icon?: string
}

// 默认官方商店（用于 API 不可用时的降级）

/** 官方远程商店 */
export const OFFICIAL_STORE: RemoteStoreSource = {
  name: 'Myriad 官方商店',
  description: '官方应用源，托管经审核的 Tapp',
  url: 'https://raw.githubusercontent.com/Myriad-You/tapp-store/main/index.json',
  enabled: true,
  official: true,
  icon: TAPP_ICON_TOKENS.store,
}

// 缓存配置

const CACHE_TTL = 5 * 60 * 1000 // 5 分钟缓存

// 缓存结构（仅内存）

interface CacheEntry {
  data: RemoteStoreIndex
  timestamp: number
  url: string
}

// 服务实现

class RemoteStoreServiceImpl {
  /** 商店源列表（从后端 API 获取） */
  private sources: RemoteStoreSource[] = []
  /** 商店索引缓存（仅内存） */
  private cache: Map<string, CacheEntry> = new Map()
  /** 同一商店只保留一个在途索引请求；删除源时可中止。 */
  private pendingIndexRequests = new Map<
    string,
    { promise: Promise<RemoteStoreIndex>; controller: AbortController }
  >()

  /** 是否已从 API 加载 */
  private sourcesLoaded = false
  /** 加载 Promise（防止并发加载） */
  private loadingPromise: Promise<void> | null = null

  // 商店源管理（通过后端 API）

  /** 从后端 API 加载商店源 */
  private async loadSourcesFromApi(): Promise<void> {
    // 防止并发加载
    if (this.loadingPromise) {
      return this.loadingPromise
    }

    this.loadingPromise = (async () => {
      try {
        const response = await api.get('/api/tapps/store/sources')
        if (response.data?.success && Array.isArray(response.data.data)) {
          this.sources = response.data.data.map((s: any) => ({
            id: s.id,
            name: s.name,
            description: s.description,
            url: s.url,
            enabled: s.enabled,
            official: s.official,
            icon: s.icon,
          }))
          // 确保至少有官方商店
          if (this.sources.length === 0) {
            this.sources = [OFFICIAL_STORE]
          }
        } else {
          console.warn(
            '[RemoteStore] Invalid API response, using default store',
          )
          this.sources = [OFFICIAL_STORE]
        }
        this.sourcesLoaded = true
      } catch (error) {
        console.error('[RemoteStore] Failed to load sources from API:', error)
        // 降级：使用默认官方商店
        this.sources = [OFFICIAL_STORE]
        this.sourcesLoaded = true
      } finally {
        this.pruneCacheToSources()
        this.loadingPromise = null
      }
    })()

    return this.loadingPromise
  }

  /** 确保商店源已加载 */
  private async ensureSourcesLoaded(): Promise<void> {
    if (!this.sourcesLoaded) {
      await this.loadSourcesFromApi()
    }
  }

  /** 获取所有商店源 */
  async getSources(): Promise<RemoteStoreSource[]> {
    await this.ensureSourcesLoaded()
    return [...this.sources]
  }

  /** 获取启用的商店源 */
  async getEnabledSources(): Promise<RemoteStoreSource[]> {
    const sources = await this.getSources()
    return sources.filter((s) => s.enabled)
  }

  /** 添加商店源（需要管理员权限） */
  async addSource(
    source: Omit<RemoteStoreSource, 'id' | 'official'>,
  ): Promise<void> {
    try {
      const response = await api.post('/api/tapps/store/sources', {
        name: source.name,
        description: source.description,
        url: source.url,
        enabled: source.enabled,
        icon: source.icon,
      })

      if (!response.data?.success) {
        throw new Error(response.data?.error || '添加商店源失败')
      }

      // 添加成功，刷新本地缓存
      const newSource: RemoteStoreSource = {
        id: response.data.data.id,
        name: response.data.data.name,
        description: response.data.data.description,
        url: response.data.data.url,
        enabled: response.data.data.enabled,
        official: response.data.data.official,
        icon: response.data.data.icon,
      }
      this.sources.push(newSource)
    } catch (error: any) {
      if (error.response?.status === 403) {
        throw new Error('需要管理员权限')
      }
      if (error.response?.status === 409) {
        throw new Error('该商店源已存在')
      }
      throw new Error(error.message || '添加商店源失败')
    }
  }

  /** 移除商店源（需要管理员权限） */
  async removeSource(sourceId: number): Promise<void> {
    const source = this.sources.find((s) => s.id === sourceId)
    if (source?.official) {
      throw new Error('无法移除官方商店')
    }

    try {
      const response = await api.delete(`/api/tapps/store/sources/${sourceId}`)

      if (!response.data?.success) {
        throw new Error(response.data?.error || '删除商店源失败')
      }

      // 删除成功，更新本地缓存
      this.sources = this.sources.filter((s) => s.id !== sourceId)
      // 同时清除该商店的索引缓存
      if (source) this.clearCachedSource(source.url)
    } catch (error: any) {
      if (error.response?.status === 403) {
        throw new Error('需要管理员权限或无法删除官方商店')
      }
      if (error.response?.status === 404) {
        throw new Error('商店源不存在')
      }
      throw new Error(error.message || '删除商店源失败')
    }
  }

  /** 启用/禁用商店源（需要管理员权限） */
  async toggleSource(sourceId: number, enabled: boolean): Promise<void> {
    try {
      const response = await api.post(`/api/tapps/store/sources/${sourceId}`, {
        enabled,
      })

      if (!response.data?.success) {
        throw new Error(response.data?.error || '更新商店源失败')
      }

      // 更新成功，更新本地缓存
      const source = this.sources.find((s) => s.id === sourceId)
      if (source) {
        source.enabled = enabled
      }
    } catch (error: any) {
      if (error.response?.status === 403) {
        throw new Error('需要管理员权限')
      }
      if (error.response?.status === 404) {
        throw new Error('商店源不存在')
      }
      throw new Error(error.message || '更新商店源失败')
    }
  }

  /**
   * 更新商店源名称 / URL 等（需要管理员权限）。
   * 官方源不可改 URL；自定义源改 URL 后会清索引缓存。
   */
  async updateSource(
    sourceId: number,
    patch: {
      name?: string
      description?: string
      url?: string
      enabled?: boolean
      icon?: string
    },
  ): Promise<RemoteStoreSource> {
    const existing = this.sources.find((s) => s.id === sourceId)
    if (existing?.official && patch.url !== undefined) {
      throw new Error('无法修改官方商店的 URL')
    }

    try {
      const response = await api.post(
        `/api/tapps/store/sources/${sourceId}`,
        patch,
      )

      if (!response.data?.success) {
        throw new Error(response.data?.error || '更新商店源失败')
      }

      const data = response.data.data
      const updated: RemoteStoreSource = {
        id: data.id,
        name: data.name,
        description: data.description,
        url: data.url,
        enabled: data.enabled,
        official: data.official,
        icon: data.icon,
      }

      const idx = this.sources.findIndex((s) => s.id === sourceId)
      if (idx >= 0) {
        const prevUrl = this.sources[idx]!.url
        this.sources[idx] = updated
        if (prevUrl !== updated.url) {
          this.clearCachedSource(prevUrl)
          this.clearCachedSource(updated.url)
        }
      } else {
        this.sources.push(updated)
      }
      return updated
    } catch (error: any) {
      if (error.response?.status === 403) {
        throw new Error(
          error.response?.data?.error || '需要管理员权限或无法修改官方商店 URL',
        )
      }
      if (error.response?.status === 404) {
        throw new Error('商店源不存在')
      }
      if (error.response?.status === 409) {
        throw new Error('该商店源 URL 已存在')
      }
      if (error.response?.status === 400) {
        throw new Error(error.response?.data?.error || '无效的商店源')
      }
      throw new Error(error.message || '更新商店源失败')
    }
  }

  /** 刷新商店源列表（从 API 重新加载） */
  async refreshSources(): Promise<void> {
    this.sourcesLoaded = false
    await this.loadSourcesFromApi()
  }

  // 商店数据获取

  /** 获取商店索引（带内存缓存） */
  async fetchStoreIndex(
    source: RemoteStoreSource,
    forceRefresh = false,
  ): Promise<RemoteStoreIndex> {
    const cacheKey = source.url
    const now = Date.now()

    // 检查内存缓存
    if (!forceRefresh) {
      const cached = this.cache.get(cacheKey)
      if (cached && now - cached.timestamp < CACHE_TTL) {
        return cached.data
      }
    }

    const pending = this.pendingIndexRequests.get(cacheKey)
    if (pending) return pending.promise

    const controller = new AbortController()
    const promise = this.fetchStoreIndexFromNetwork(
      source,
      cacheKey,
      controller.signal,
    ).finally(() => {
      if (this.pendingIndexRequests.get(cacheKey)?.promise === promise) {
        this.pendingIndexRequests.delete(cacheKey)
      }
    })
    this.pendingIndexRequests.set(cacheKey, { promise, controller })
    return promise
  }

  private clearCachedSource(url: string): void {
    this.cache.delete(url)
    this.pendingIndexRequests.get(url)?.controller.abort()
    this.pendingIndexRequests.delete(url)
  }

  private pruneCacheToSources(): void {
    const activeUrls = new Set(this.sources.map((source) => source.url))
    const knownUrls = new Set([
      ...this.cache.keys(),
      ...this.pendingIndexRequests.keys(),
    ])
    for (const url of knownUrls) {
      if (!activeUrls.has(url)) this.clearCachedSource(url)
    }
  }

  private async fetchStoreIndexFromNetwork(
    source: RemoteStoreSource,
    cacheKey: string,
    signal: AbortSignal,
  ): Promise<RemoteStoreIndex> {
    // 从远程获取
    try {
      const response = await fetch(
        this.withStoreCacheBust(source.url, this.newStoreDownloadSessionId()),
        {
          // Simple request only — see storeResourceFetchInit CORS note.
          headers: {
            Accept: 'application/json',
          },
          cache: 'no-store',
          credentials: 'omit',
          signal,
        },
      )

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`)
      }

      const data = (await response.json()) as RemoteStoreIndex
      if (signal.aborted)
        throw new DOMException('Request aborted', 'AbortError')

      // 验证数据
      if (!data.name || !data.apps || !Array.isArray(data.apps)) {
        throw new Error('无效的商店索引格式')
      }

      data.apps = data.apps.map((app) => ({
        ...app,
        preview: parseStorePreview(
          (app as RemoteApp & { preview?: unknown }).preview,
        ),
      }))

      // 更新内存缓存
      this.cache.set(cacheKey, {
        data,
        timestamp: Date.now(),
        url: source.url,
      })

      return data
    } catch (error) {
      console.error(
        `[RemoteStore] Failed to fetch index from ${source.url}:`,
        error,
      )
      throw new Error(
        `无法获取商店数据: ${error instanceof Error ? error.message : '未知错误'}`,
      )
    }
  }

  /** 获取所有启用商店的应用列表 */
  async fetchAllApps(forceRefresh = false): Promise<{
    apps: Array<
      RemoteApp & {
        sourceUrl: string
        sourceName: string
        sourceBaseUrl: string
        sourceOfficial?: boolean
      }
    >
    sources: Array<{ source: RemoteStoreSource; error?: string }>
  }> {
    const enabledSources = await this.getEnabledSources()
    const prioritizedSources = enabledSources
      .map((source, configuredIndex) => ({ source, configuredIndex }))
      .sort((a, b) => {
        const officialRank =
          Number(Boolean(b.source.official)) -
          Number(Boolean(a.source.official))
        return officialRank || a.configuredIndex - b.configuredIndex
      })
      .map(({ source }) => source)

    // 并行获取，但保持显式源优先级；不能让网络返回顺序决定同 id 应用的来源。
    const results = await Promise.all(
      prioritizedSources.map(async (source) => {
        try {
          const index = await this.fetchStoreIndex(source, forceRefresh)
          return { source, index }
        } catch (error) {
          return {
            source,
            error: error instanceof Error ? error.message : '未知错误',
          }
        }
      }),
    )

    // 多源同 id：官方源优先；其余按配置顺序优先。同一 id 只产生一个安装来源。
    const apps: Array<
      RemoteApp & {
        sourceUrl: string
        sourceName: string
        sourceBaseUrl: string
        sourceOfficial?: boolean
      }
    > = []
    const seenIds = new Set<string>()
    for (const result of results) {
      if (result.index) {
        const sourceDirectory = new URL('.', result.source.url).toString()
        const sourceBaseUrl = result.index.base_url
          ? new URL(result.index.base_url, sourceDirectory).toString()
          : sourceDirectory
        for (const app of result.index.apps) {
          if (!app?.id || seenIds.has(app.id)) continue
          seenIds.add(app.id)
          apps.push({
            ...app,
            sourceUrl: result.source.url,
            sourceName: result.source.name,
            sourceBaseUrl,
            sourceOfficial: Boolean(result.source.official),
          })
        }
      }
    }

    // Overlay live install counts from edge stats (non-blocking on failure).
    try {
      const { fetchStoreDownloadCounts } = await import('./storeStats')
      const officialIds = apps
        .filter((a) => a.sourceOfficial)
        .map((a) => a.id)
      const counts = await fetchStoreDownloadCounts(officialIds)
      for (const app of apps) {
        const n = counts[app.id]
        if (typeof n === 'number' && n > 0) {
          app.downloads = n
        }
      }
    } catch {
      // stats optional
    }

    return {
      apps,
      sources: results.map((r) => ({ source: r.source, error: r.error })),
    }
  }

  /** 获取远程分类列表 */
  async fetchCategories(source: RemoteStoreSource): Promise<RemoteCategory[]> {
    const index = await this.fetchStoreIndex(source)
    return index.categories || []
  }

  // 应用下载

  /**
   * Store package fetches must bypass browser/CDN intermediate caches.
   * GitHub raw serves `Cache-Control: max-age=300`; without this, delete+reinstall
   * on a production host can mix a fresh manifest with a stale page.css/page.html.
   *
   * **CORS:** Do not set `Cache-Control` / `Pragma` request headers. They are not
   * CORS-safelisted and force a preflight OPTIONS that raw.githubusercontent.com
   * rejects (install fallback then fails with "Failed to fetch" from localhost).
   * Use `cache: 'no-store'` + query cache-bust instead.
   */
  private storeResourceFetchInit(
    extraHeaders?: Record<string, string>,
  ): RequestInit {
    const headers: Record<string, string> = {
      // Keep Accept simple (CORS-safelisted) when provided by callers.
      ...(extraHeaders || {}),
    }
    return {
      cache: 'no-store',
      // mode default is cors for cross-origin; omit credentials for public store URLs
      credentials: 'omit',
      headers,
    }
  }

  /**
   * Unique token per install/download session so every package file is fetched
   * with the same bust id (consistent snapshot) but never reuses a prior session.
   */
  private newStoreDownloadSessionId(): string {
    if (
      typeof crypto !== 'undefined' &&
      typeof crypto.randomUUID === 'function'
    ) {
      return crypto.randomUUID().replace(/-/g, '')
    }
    return `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}`
  }

  /**
   * Append cache-bust query so CDN/browser cannot serve a previous package file.
   * Applied to **all** store hosts (not only GitHub) for install-path downloads.
   * CORS-safe: no non-simple request headers required.
   */
  private withStoreCacheBust(url: string, sessionId: string): string {
    try {
      const parsed = new URL(url)
      // Stable per download session; replace any prior bust params.
      parsed.searchParams.delete('_myriad_cb')
      parsed.searchParams.delete('_')
      parsed.searchParams.set('_myriad_cb', sessionId)
      return parsed.toString()
    } catch {
      const sep = url.includes('?') ? '&' : '?'
      return `${url}${sep}_myriad_cb=${encodeURIComponent(sessionId)}`
    }
  }

  private storeFetchUrl(
    relativeOrAbsolute: string,
    baseUrl: string,
    sessionId: string,
  ): string {
    return this.withStoreCacheBust(
      this.resolveUrl(relativeOrAbsolute, baseUrl),
      sessionId,
    )
  }

  /** 下载应用的 manifest */
  async downloadManifest(
    app: RemoteApp,
    storeIndex: RemoteStoreIndex,
    sessionId?: string,
  ): Promise<TappManifest> {
    const sid = sessionId || this.newStoreDownloadSessionId()
    const manifestUrl = this.storeFetchUrl(
      app.download.manifest,
      storeIndex.base_url,
      sid,
    )

    const response = await fetch(
      manifestUrl,
      this.storeResourceFetchInit({ Accept: 'application/json' }),
    )
    if (!response.ok) {
      throw new Error(`无法下载 manifest: HTTP ${response.status}`)
    }

    const manifest = (await response.json()) as TappManifest
    return manifest
  }

  /** 下载应用代码 */
  async downloadCode(
    app: RemoteApp,
    storeIndex: RemoteStoreIndex,
    sessionId?: string,
  ): Promise<string> {
    const sid = sessionId || this.newStoreDownloadSessionId()
    const codeUrl = this.storeFetchUrl(
      app.download.code,
      storeIndex.base_url,
      sid,
    )

    const response = await fetch(codeUrl, this.storeResourceFetchInit())
    if (!response.ok) {
      throw new Error(`无法下载代码: HTTP ${response.status}`)
    }

    const code = await response.text()
    return code
  }

  /**
   * Static resources for store detail / featured previews.
   *
   * Prefer catalog `preview` (type: snapshot) paths. If none, fall back to
   * `download.page_template` (+ page styles). Never re-renders local installed
   * package HTML — that path produced empty white shells after sanitization.
   */
  async downloadAppPreview(
    app: RemoteApp,
    baseUrl: string,
  ): Promise<{ html?: string; css?: string }> {
    const sessionId = this.newStoreDownloadSessionId()
    const downloadText = async (path?: string): Promise<string | undefined> => {
      if (!path) return undefined
      const response = await fetch(
        this.storeFetchUrl(path, baseUrl, sessionId),
        this.storeResourceFetchInit(),
      )
      if (!response.ok) return undefined
      const text = await response.text()
      return text.slice(0, 512 * 1024)
    }

    // 1. Explicit merchandising snapshot
    if (app.preview?.html) {
      const [html, ...styles] = await Promise.all([
        downloadText(app.preview.html),
        ...app.preview.styles.map((path) => downloadText(path)),
      ])
      return {
        html,
        css: styles.filter(Boolean).join('\n') || undefined,
      }
    }

    // 2. Catalog page_template (static shell only — not local install resources)
    const htmlPath = app.download.page_template
    if (!htmlPath) return {}

    const stylePaths = [app.download.styles, app.download.page_styles].filter(
      (path): path is string => Boolean(path),
    )
    const [html, ...styles] = await Promise.all([
      downloadText(htmlPath),
      ...stylePaths.map((path) => downloadText(path)),
    ])

    return {
      html,
      css: styles.filter(Boolean).join('\n') || undefined,
    }
  }

  /**
   * 在浏览器侧完整下载远程应用包（供后端无法出站时的安装回退）
   *
   * 商店列表本就由浏览器直连远程 index；当 server 侧 /api/tapps/install(store)
   * 因容器无外网/GitHub 不可达返回 502 时，可用此方法拉取后走 direct 安装。
   *
   * When `manifest.assets` is present, also downloads each binary asset as base64
   * (path → base64) so `Tapp.assets` works after install.
   */
  async downloadAppPackage(
    app: RemoteApp,
    storeIndex: RemoteStoreIndex,
    options?: {
      onProgress?: import('../utils/tappInstallProgress').TappInstallProgressCallback
      estimatedBytes?: number
    },
  ): Promise<{
    manifest: TappManifest
    code: string
    styles?: string
    widgetCss?: string
    pageCss?: string
    pageTemplate?: string
    widgetTemplates?: Record<string, Record<string, string>>
    i18n?: Record<string, unknown>
    pageModules?: Record<string, string>
    /** Package-static assets (manifest path → standard base64) */
    assets?: Record<string, string>
  }> {
    const baseUrl = storeIndex.base_url || this.deriveBaseUrl(storeIndex)
    const { clampInstallPercent } = await import('../utils/tappInstallProgress')
    const report = options?.onProgress
    // One session id for the whole package so all files share the same bust
    // token (coherent snapshot) and never hit a prior install's CDN entry.
    const downloadSessionId = this.newStoreDownloadSessionId()

    const downloadText = async (
      relativePath?: string,
      requiredLabel?: string,
    ): Promise<string | undefined> => {
      if (!relativePath) {
        if (requiredLabel) {
          throw new Error(
            `Store index is missing download path for required ${requiredLabel}`,
          )
        }
        return undefined
      }
      try {
        const url = this.storeFetchUrl(relativePath, baseUrl, downloadSessionId)
        const response = await fetch(url, this.storeResourceFetchInit())
        if (!response.ok) {
          if (requiredLabel) {
            throw new Error(
              `Failed to download ${requiredLabel} (${relativePath}): HTTP ${response.status}`,
            )
          }
          return undefined
        }
        return await response.text()
      } catch (e) {
        if (requiredLabel) {
          throw e instanceof Error
            ? e
            : new Error(`Failed to download ${requiredLabel}: ${String(e)}`)
        }
        return undefined
      }
    }

    const downloadJson = async (
      relativePath?: string,
    ): Promise<unknown | undefined> => {
      if (!relativePath) return undefined
      try {
        const url = this.storeFetchUrl(relativePath, baseUrl, downloadSessionId)
        const response = await fetch(
          url,
          this.storeResourceFetchInit({ Accept: 'application/json' }),
        )
        if (!response.ok) return undefined
        return await response.json()
      } catch {
        return undefined
      }
    }

    report?.({
      phase: 'download',
      message: 'download',
      percent: 8,
      detail: 'manifest',
    })

    const indexWithBase = { ...storeIndex, base_url: baseUrl }
    // Manifest first so we know which package fields are required (pageStyles etc.)
    const downloadedManifest = await this.downloadManifest(
      app,
      indexWithBase,
      downloadSessionId,
    )
    const manifest: TappManifest = downloadedManifest

    // Catalog entry version must match the package we just pulled (stale index / CDN).
    if (
      app.version &&
      manifest.version &&
      app.version.trim() !== manifest.version.trim()
    ) {
      throw new Error(
        `Store package version mismatch: catalog lists ${app.version} but manifest.json is ${manifest.version}. Refresh the store and retry.`,
      )
    }

    const needsPageCss = !!manifest.pageStyles
    const needsPageTemplate = !!manifest.pageTemplate
    const needsWidgetCss = !!manifest.widgetStyles

    const [code, styles, widgetCss, pageCss, pageTemplate] = await Promise.all([
      this.downloadCode(app, indexWithBase, downloadSessionId),
      downloadText(app.download.styles),
      downloadText(
        app.download.widget_styles,
        needsWidgetCss ? 'widgetStyles' : undefined,
      ),
      downloadText(
        app.download.page_styles,
        needsPageCss ? 'pageStyles' : undefined,
      ),
      downloadText(
        app.download.page_template,
        needsPageTemplate ? 'pageTemplate' : undefined,
      ),
    ])

    if (needsPageCss && !pageCss) {
      throw new Error(
        'Downloaded package is missing pageStyles content (page.css). Check store download.page_styles.',
      )
    }
    if (needsPageTemplate && !pageTemplate) {
      throw new Error(
        'Downloaded package is missing pageTemplate content. Check store download.page_template.',
      )
    }

    report?.({
      phase: 'download',
      message: 'download',
      percent: 18,
      detail: 'package',
    })

    let widgetTemplates: Record<string, Record<string, string>> | undefined
    if (app.download.widget_templates) {
      const templates: Record<string, Record<string, string>> = {}
      await Promise.all(
        Object.entries(app.download.widget_templates).map(
          async ([widgetId, paths]) => {
            const downloaded: Record<string, string> = {}
            await Promise.all(
              Object.entries(paths).map(async ([size, path]) => {
                const content = await downloadText(path)
                if (content) downloaded[size] = content
              }),
            )
            if (Object.keys(downloaded).length > 0) {
              templates[widgetId] = downloaded
            }
          },
        ),
      )
      if (Object.keys(templates).length > 0) {
        widgetTemplates = templates
      }
    }

    let i18n: Record<string, unknown> | undefined
    if (app.download.i18n) {
      const i18nData: Record<string, unknown> = {}
      await Promise.all(
        Object.entries(app.download.i18n).map(async ([lang, path]) => {
          const data = await downloadJson(path)
          if (data !== undefined) i18nData[lang] = data
        }),
      )
      if (Object.keys(i18nData).length > 0) i18n = i18nData
    }

    let pageModules: Record<string, string> | undefined
    if (app.download.page_modules) {
      const entries = Object.entries(app.download.page_modules)
      const modules: Record<string, string> = {}
      const totalPm = entries.length
      let donePm = 0
      // Bound concurrency (same idea as assets) + progress for multi-file page packs
      const concurrency = 4
      let nextPm = 0
      const worker = async () => {
        while (nextPm < entries.length) {
          const i = nextPm++
          const [filename, path] = entries[i]!
          const content = await downloadText(path, `page module ${filename}`)
          if (!content) {
            throw new Error(
              `Failed to download page module ${filename} (${path})`,
            )
          }
          modules[filename] = content
          donePm++
          // Map page-module downloads into 20–75% of the download bar
          const frac = totalPm > 0 ? donePm / totalPm : 1
          report?.({
            phase: 'download',
            message: 'download',
            percent: clampInstallPercent(20 + frac * 55),
            detail: filename,
          })
        }
      }
      await Promise.all(
        Array.from(
          { length: Math.min(concurrency, Math.max(1, totalPm)) },
          () => worker(),
        ),
      )
      // Manifest may declare pageModules; store index map must cover them all
      const declared = Array.isArray(manifest.pageModules)
        ? manifest.pageModules
        : []
      for (const name of declared) {
        if (!modules[name]) {
          throw new Error(
            `Store package is missing page module declared in manifest: ${name}`,
          )
        }
      }
      if (Object.keys(modules).length > 0) pageModules = modules
    }

    // Binary package assets declared in manifest.assets
    const packageRoot = storePackageRoot(
      app.download.code || app.download.manifest || '',
    )
    const assets = await this.downloadPackageAssets(
      manifest,
      packageRoot,
      baseUrl,
      downloadSessionId,
      {
        onProgress: report,
        estimatedBytes: options?.estimatedBytes ?? app.size,
      },
    )

    report?.({
      phase: 'download',
      message: 'download',
      percent: clampInstallPercent(90),
    })

    return {
      manifest,
      code,
      styles,
      widgetCss,
      pageCss,
      pageTemplate,
      widgetTemplates,
      i18n,
      pageModules,
      assets,
    }
  }

  /**
   * Download manifest.assets files as base64 map for direct install.
   * URL = `{base}/{packageRoot}/{assetPath}` e.g.
   * `…/apps/com.myriad.doudizhu/assets/felt/table_felt.png`
   */
  private async downloadPackageAssets(
    manifest: TappManifest,
    packageRoot: string,
    baseUrl: string,
    sessionId: string,
    options?: {
      onProgress?: import('../utils/tappInstallProgress').TappInstallProgressCallback
      estimatedBytes?: number
    },
  ): Promise<Record<string, string> | undefined> {
    const declared = manifest.assets
    if (!declared || declared.length === 0) return undefined
    if (declared.length > 64) {
      throw new Error(
        `Tapp assets accepts at most 64 entries (got ${declared.length})`,
      )
    }

    const { clampInstallPercent } = await import('../utils/tappInstallProgress')
    const report = options?.onProgress
    const out: Record<string, string> = {}
    const total = declared.length
    let completed = 0

    // Bound concurrency so progress updates are visible and we don't melt the browser
    const concurrency = 4
    let nextIndex = 0

    const worker = async () => {
      while (nextIndex < declared.length) {
        const i = nextIndex++
        const assetPath = declared[i]!
        if (!assetPath.startsWith('assets/')) {
          throw new Error(
            `Invalid asset path (must be under assets/): ${assetPath}`,
          )
        }
        const storeRel = storeAssetStorePath(packageRoot, assetPath)
        const url = this.storeFetchUrl(storeRel, baseUrl, sessionId)
        const response = await fetch(
          url,
          this.storeResourceFetchInit({ Accept: '*/*' }),
        )
        if (!response.ok) {
          throw new Error(
            `Failed to fetch asset ${assetPath}: HTTP ${response.status}`,
          )
        }
        const buffer = await response.arrayBuffer()
        out[assetPath] = arrayBufferToBase64(buffer)
        completed += 1
        // Assets occupy ~20%–90% of the install bar
        const pct = 20 + (completed / total) * 70
        report?.({
          phase: 'download',
          message: 'download',
          percent: clampInstallPercent(pct),
          detail: assetPath,
          loadedBytes: completed,
          totalBytes: total,
        })
      }
    }

    await Promise.all(
      Array.from({ length: Math.min(concurrency, total) }, () => worker()),
    )
    return out
  }

  /** index 未提供 base_url 时返回空，由调用方用商店 URL 推导 */
  private deriveBaseUrl(storeIndex: RemoteStoreIndex): string {
    return storeIndex.base_url || ''
  }

  /** 下载应用的 README */
  async downloadReadme(
    app: RemoteApp,
    storeIndex: RemoteStoreIndex,
  ): Promise<string | null> {
    if (!app.download.readme) return null

    try {
      const readmeUrl = this.storeFetchUrl(
        app.download.readme,
        storeIndex.base_url || this.deriveBaseUrl(storeIndex),
        this.newStoreDownloadSessionId(),
      )
      const response = await fetch(readmeUrl, this.storeResourceFetchInit())
      if (!response.ok) return null
      return await response.text()
    } catch {
      return null
    }
  }

  /** 解析相对 URL */
  private resolveUrl(relativePath: string, baseUrl: string): string {
    // 如果是绝对 URL，直接返回
    if (
      relativePath.startsWith('http://') ||
      relativePath.startsWith('https://')
    ) {
      return relativePath
    }
    // 组合基础 URL 和相对路径
    const base = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`
    return base + relativePath
  }

  // 缓存管理

  /** 清除内存缓存 */
  clearCache(): void {
    this.cache.clear()
  }

  /** 获取缓存状态 */
  getCacheStatus(): { count: number; oldestEntry: number | null } {
    const entries = Array.from(this.cache.values())
    const oldestEntry =
      entries.length > 0 ? Math.min(...entries.map((e) => e.timestamp)) : null

    return {
      count: entries.length,
      oldestEntry,
    }
  }
}

function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer)
  const chunk = 0x8000
  let binary = ''
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

// 单例导出
export const RemoteStoreService = new RemoteStoreServiceImpl()

export default RemoteStoreService
