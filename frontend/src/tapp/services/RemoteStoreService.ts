/**
 * 远程应用商店服务
 * 从 GitHub 托管的远程商店获取和安装 Tapp
 *
 * 商店源配置存储在后端数据库中，通过 API 进行管理
 * 缓存仅保存在内存中，刷新页面后重新获取
 */

import type { TappManifest } from '../types'
import api from '../../lib/api'

// ============ 类型定义 ============

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
  /** 是否启用 */
  enabled: boolean
  /** 是否为官方商店 */
  official?: boolean
  /** 图标 */
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
  /** 主题色（十六进制，如 #6366f1） */
  theme_color?: string
  /** 分类 */
  category: string
  /** 标签 */
  tags?: string[]
  /** 所需权限 */
  permissions: string[]
  /** 最低 Myriad 版本 */
  min_myriad_version?: string
  /** 下载链接 */
  download: {
    /** manifest.json URL（相对于 base_url） */
    manifest: string
    /** 代码文件 URL（相对于 base_url） */
    code: string
    /** README URL（可选） */
    readme?: string
    /** Widget 样式 CSS */
    widget_styles?: string
    /** Page 样式 CSS */
    page_styles?: string
    /** Page 模板 HTML */
    page_template?: string
    /** Widget 模板（按尺寸） */
    widget_templates?: Record<string, string>
  }
  /** 许可证 */
  license?: string
  /** 主页 URL */
  homepage?: string
  /** 仓库 URL */
  repository?: string
  /** 截图 URL 列表 */
  screenshots?: string[]
  /** 文件大小（字节） */
  size?: number
  /** 是否推荐应用 */
  featured?: boolean
  /** 是否官方验证 */
  verified?: boolean
  /** 创建时间 */
  created_at?: string
  /** 更新时间 */
  updated_at?: string
}

/** 远程分类 */
export interface RemoteCategory {
  id: string
  name: string
  description?: string
  icon?: string
}

// ============ 默认官方商店（用于 API 不可用时的降级） ============

/** 官方远程商店 */
export const OFFICIAL_STORE: RemoteStoreSource = {
  name: 'Myriad 官方商店',
  description: '官方应用商店，提供经过审核的高质量应用',
  url: 'https://raw.githubusercontent.com/Myriad-You/tapp-store/main/index.json',
  enabled: true,
  official: true,
  icon: '🏪',
}

// ============ 缓存配置 ============

const CACHE_TTL = 5 * 60 * 1000 // 5 分钟缓存

// ============ 缓存结构（仅内存） ============

interface CacheEntry {
  data: RemoteStoreIndex
  timestamp: number
  url: string
}

// ============ 服务实现 ============

class RemoteStoreServiceImpl {
  /** 商店源列表（从后端 API 获取） */
  private sources: RemoteStoreSource[] = []
  /** 商店索引缓存（仅内存） */
  private cache: Map<string, CacheEntry> = new Map()
  /** 是否已从 API 加载 */
  private sourcesLoaded = false
  /** 加载 Promise（防止并发加载） */
  private loadingPromise: Promise<void> | null = null

  // ============ 商店源管理（通过后端 API） ============

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
        }
        else {
          console.warn('[RemoteStore] Invalid API response, using default store')
          this.sources = [OFFICIAL_STORE]
        }
        this.sourcesLoaded = true
      }
      catch (error) {
        console.error('[RemoteStore] Failed to load sources from API:', error)
        // 降级：使用默认官方商店
        this.sources = [OFFICIAL_STORE]
        this.sourcesLoaded = true
      }
      finally {
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
    return sources.filter(s => s.enabled)
  }

  /** 添加商店源（需要管理员权限） */
  async addSource(source: Omit<RemoteStoreSource, 'id' | 'official'>): Promise<void> {
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
    }
    catch (error: any) {
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
    const source = this.sources.find(s => s.id === sourceId)
    if (source?.official) {
      throw new Error('无法移除官方商店')
    }

    try {
      const response = await api.delete(`/api/tapps/store/sources/${sourceId}`)

      if (!response.data?.success) {
        throw new Error(response.data?.error || '删除商店源失败')
      }

      // 删除成功，更新本地缓存
      this.sources = this.sources.filter(s => s.id !== sourceId)
      // 同时清除该商店的索引缓存
      const cachedSource = this.sources.find(s => s.id === sourceId)
      if (cachedSource) {
        this.cache.delete(cachedSource.url)
      }
    }
    catch (error: any) {
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
      const source = this.sources.find(s => s.id === sourceId)
      if (source) {
        source.enabled = enabled
      }
    }
    catch (error: any) {
      if (error.response?.status === 403) {
        throw new Error('需要管理员权限')
      }
      if (error.response?.status === 404) {
        throw new Error('商店源不存在')
      }
      throw new Error(error.message || '更新商店源失败')
    }
  }

  /** 刷新商店源列表（从 API 重新加载） */
  async refreshSources(): Promise<void> {
    this.sourcesLoaded = false
    await this.loadSourcesFromApi()
  }

  // ============ 商店数据获取 ============

  /** 获取商店索引（带内存缓存） */
  async fetchStoreIndex(source: RemoteStoreSource, forceRefresh = false): Promise<RemoteStoreIndex> {
    const cacheKey = source.url
    const now = Date.now()

    // 检查内存缓存
    if (!forceRefresh) {
      const cached = this.cache.get(cacheKey)
      if (cached && now - cached.timestamp < CACHE_TTL) {
        return cached.data
      }
    }

    // 从远程获取
    try {
      const response = await fetch(source.url, {
        headers: {
          Accept: 'application/json',
        },
        cache: 'no-cache',
      })

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`)
      }

      const data = await response.json() as RemoteStoreIndex

      // 验证数据
      if (!data.name || !data.apps || !Array.isArray(data.apps)) {
        throw new Error('无效的商店索引格式')
      }

      // 更新内存缓存
      this.cache.set(cacheKey, {
        data,
        timestamp: now,
        url: source.url,
      })

      return data
    }
    catch (error) {
      console.error(`[RemoteStore] Failed to fetch index from ${source.url}:`, error)
      throw new Error(`无法获取商店数据: ${error instanceof Error ? error.message : '未知错误'}`)
    }
  }

  /** 获取所有启用商店的应用列表 */
  async fetchAllApps(forceRefresh = false): Promise<{
    apps: Array<RemoteApp & { sourceUrl: string, sourceName: string }>
    sources: Array<{ source: RemoteStoreSource, error?: string }>
  }> {
    const enabledSources = await this.getEnabledSources()
    const results: Array<{ source: RemoteStoreSource, index?: RemoteStoreIndex, error?: string }> = []

    // 并行获取所有商店数据
    await Promise.all(
      enabledSources.map(async (source) => {
        try {
          const index = await this.fetchStoreIndex(source, forceRefresh)
          results.push({ source, index })
        }
        catch (error) {
          results.push({ source, error: error instanceof Error ? error.message : '未知错误' })
        }
      }),
    )

    // 合并应用列表
    const apps: Array<RemoteApp & { sourceUrl: string, sourceName: string }> = []
    for (const result of results) {
      if (result.index) {
        for (const app of result.index.apps) {
          apps.push({
            ...app,
            sourceUrl: result.source.url,
            sourceName: result.source.name,
          })
        }
      }
    }

    return {
      apps,
      sources: results.map(r => ({ source: r.source, error: r.error })),
    }
  }

  /** 获取远程分类列表 */
  async fetchCategories(source: RemoteStoreSource): Promise<RemoteCategory[]> {
    const index = await this.fetchStoreIndex(source)
    return index.categories || []
  }

  // ============ 应用下载 ============

  /** 下载应用的 manifest */
  async downloadManifest(app: RemoteApp, storeIndex: RemoteStoreIndex): Promise<TappManifest> {
    const manifestUrl = this.resolveUrl(app.download.manifest, storeIndex.base_url)

    const response = await fetch(manifestUrl)
    if (!response.ok) {
      throw new Error(`无法下载 manifest: HTTP ${response.status}`)
    }

    const manifest = await response.json() as TappManifest
    return manifest
  }

  /** 下载应用代码 */
  async downloadCode(app: RemoteApp, storeIndex: RemoteStoreIndex): Promise<string> {
    const codeUrl = this.resolveUrl(app.download.code, storeIndex.base_url)

    const response = await fetch(codeUrl)
    if (!response.ok) {
      throw new Error(`无法下载代码: HTTP ${response.status}`)
    }

    const code = await response.text()
    return code
  }

  /** 下载应用的 README */
  async downloadReadme(app: RemoteApp, storeIndex: RemoteStoreIndex): Promise<string | null> {
    if (!app.download.readme)
      return null

    try {
      const readmeUrl = this.resolveUrl(app.download.readme, storeIndex.base_url)
      const response = await fetch(readmeUrl)
      if (!response.ok)
        return null
      return await response.text()
    }
    catch {
      return null
    }
  }

  /** 解析相对 URL */
  private resolveUrl(relativePath: string, baseUrl: string): string {
    // 如果是绝对 URL，直接返回
    if (relativePath.startsWith('http://') || relativePath.startsWith('https://')) {
      return relativePath
    }
    // 组合基础 URL 和相对路径
    const base = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`
    return base + relativePath
  }

  // ============ 缓存管理 ============

  /** 清除内存缓存 */
  clearCache(): void {
    this.cache.clear()
  }

  /** 获取缓存状态 */
  getCacheStatus(): { count: number, oldestEntry: number | null } {
    const entries = Array.from(this.cache.values())
    const oldestEntry = entries.length > 0
      ? Math.min(...entries.map(e => e.timestamp))
      : null

    return {
      count: entries.length,
      oldestEntry,
    }
  }
}

// 单例导出
export const RemoteStoreService = new RemoteStoreServiceImpl()

export default RemoteStoreService
