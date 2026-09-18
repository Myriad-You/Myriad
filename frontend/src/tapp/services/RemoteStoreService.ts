import type { TappManifest } from '../types'
import type { RemoteStoreLocales } from '../utils/storeLocale'
import type { StorePreviewDescriptor } from '../utils/storePreview'
import { currentCopy, formatCurrent } from '../../i18n/localeCopy'
import api from '../../lib/api'
import {
  httpStatusMessage,
  isUselessErrorText,
  userFacingError,
} from '../../utils/userFacingError'
import { TAPP_ICON_TOKENS } from '../constants/icons'
import { parseStoreLocales } from '../utils/storeLocale'
import {
  storeAssetStorePath,
  storePackageRoot,
} from '../utils/storePackagePaths'
import { isStoreAppAvailable } from '../utils/storePolicy'
import { parseStorePreview } from '../utils/storePreview'
import { maxDeclaredAssets } from '../utils/tappPackageLimits'

export {
  storeAssetStorePath,
  storePackageRoot,
} from '../utils/storePackagePaths'
export type { StorePreviewDescriptor } from '../utils/storePreview'

export interface RemoteStoreSource {
  id?: number
  name: string
  description?: string
  url: string
  enabled: boolean
  official?: boolean
  icon?: string
}

export interface RemoteStoreIndex {
  name: string
  description: string
  api_version: number
  last_updated: string
  base_url: string
  apps: RemoteApp[]
  categories?: RemoteCategory[]
}

export interface RemoteApp {
  id: string
  name: string
  version: string
  description: string
  long_description?: string
  /** name/description 与 manifest.locales 同源；长介绍与预览只属 catalog。 */
  locales?: RemoteStoreLocales
  author: {
    name: string
    email?: string
    url?: string
  }
  icon?: string
  icon_svg?: string
  /** 缺省 auto：全彩图 standalone。 */
  icon_shell?: boolean
  theme_color?: string
  category: string
  tags?: string[]
  permissions: string[]
  download: {
    manifest: string
    code: string
    readme?: string
    styles?: string
    widget_styles?: string
    page_styles?: string
    page_template?: string
    widget_templates?: Record<string, Record<string, string>>
    i18n?: Record<string, string>
    modules?: Record<string, string>
  }
  license?: string
  homepage?: string
  repository?: string
  screenshots?: string[]
  preview?: StorePreviewDescriptor
  size?: number
  featured?: boolean
  verified?: boolean
  downloads?: number
  created_at?: string
  updated_at?: string
}

export interface RemoteCategory {
  id: string
  name: string
  description?: string
  icon?: string
}

export const OFFICIAL_STORE: RemoteStoreSource = {
  get name() {
    return currentCopy().tapp.officialStoreName
  },
  get description() {
    return currentCopy().tapp.officialStoreDescription
  },
  url: 'https://raw.githubusercontent.com/Myriad-You/tapp-store/main/index.json',
  enabled: true,
  official: true,
  icon: TAPP_ICON_TOKENS.store,
}

const CACHE_TTL = 5 * 60 * 1000

interface CacheEntry {
  data: RemoteStoreIndex
  timestamp: number
  url: string
}

class RemoteStoreServiceImpl {
  private sources: RemoteStoreSource[] = []
  private cache: Map<string, CacheEntry> = new Map()
  /** 同一商店一个在途索引请求；删源时可中止。 */
  private pendingIndexRequests = new Map<
    string,
    { promise: Promise<RemoteStoreIndex>; controller: AbortController }
  >()

  private sourcesLoaded = false
  private loadingPromise: Promise<void> | null = null

  private async loadSourcesFromApi(): Promise<void> {
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
        this.sources = [OFFICIAL_STORE]
        this.sourcesLoaded = true
        void import('../../utils/toastManager').then(({ showError }) => {
          showError(
            userFacingError(error, currentCopy().tapp.loadRemoteFailed),
          )
        })
      } finally {
        this.pruneCacheToSources()
        this.loadingPromise = null
      }
    })()

    return this.loadingPromise
  }

  private async ensureSourcesLoaded(): Promise<void> {
    if (!this.sourcesLoaded) {
      await this.loadSourcesFromApi()
    }
  }

  async getSources(): Promise<RemoteStoreSource[]> {
    await this.ensureSourcesLoaded()
    return Iterator.from(this.sources).toArray()
  }

  async getEnabledSources(): Promise<RemoteStoreSource[]> {
    const sources = await this.getSources()
    return sources.filter((s) => s.enabled)
  }

  private storeSourceFailure(error: unknown, fallback: string): Error {
    const axiosError = error as {
      response?: { data?: { error?: unknown } }
      message?: string
    }
    const bodyError =
      typeof axiosError.response?.data?.error === 'string'
        ? axiosError.response.data.error
        : ''
    const raw = bodyError || axiosError.message || ''
    return new Error(
      raw && !isUselessErrorText(raw) ? raw : userFacingError(error, fallback),
    )
  }

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
        throw new Error(
          typeof response.data?.error === 'string' &&
            !isUselessErrorText(response.data.error)
            ? response.data.error
            : currentCopy().tapp.storeAddFailed,
        )
      }

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
        throw new Error(currentCopy().tapp.storeAdminRequired)
      }
      if (error.response?.status === 409) {
        throw new Error(currentCopy().tapp.storeSourceExists)
      }
      throw this.storeSourceFailure(error, currentCopy().tapp.storeAddFailed)
    }
  }

  async removeSource(sourceId: number): Promise<void> {
    const source = this.sources.find((s) => s.id === sourceId)
    if (source?.official) {
      throw new Error(currentCopy().tapp.storeCannotRemoveOfficial)
    }

    try {
      const response = await api.delete(`/api/tapps/store/sources/${sourceId}`)

      if (!response.data?.success) {
        throw new Error(
          typeof response.data?.error === 'string' &&
            !isUselessErrorText(response.data.error)
            ? response.data.error
            : currentCopy().tapp.storeRemoveFailed,
        )
      }

      this.sources = this.sources.filter((s) => s.id !== sourceId)
      if (source) this.clearCachedSource(source.url)
    } catch (error: any) {
      if (error.response?.status === 403) {
        throw new Error(currentCopy().tapp.storeCannotRemoveOfficial)
      }
      if (error.response?.status === 404) {
        throw new Error(currentCopy().tapp.storeSourceNotFound)
      }
      throw this.storeSourceFailure(error, currentCopy().tapp.storeRemoveFailed)
    }
  }

  async toggleSource(sourceId: number, enabled: boolean): Promise<void> {
    try {
      const response = await api.post(`/api/tapps/store/sources/${sourceId}`, {
        enabled,
      })

      if (!response.data?.success) {
        throw new Error(
          typeof response.data?.error === 'string' &&
            !isUselessErrorText(response.data.error)
            ? response.data.error
            : currentCopy().tapp.storeUpdateFailed,
        )
      }

      const source = this.sources.find((s) => s.id === sourceId)
      if (source) {
        source.enabled = enabled
      }
    } catch (error: any) {
      if (error.response?.status === 403) {
        throw new Error(currentCopy().tapp.storeAdminRequired)
      }
      if (error.response?.status === 404) {
        throw new Error(currentCopy().tapp.storeSourceNotFound)
      }
      throw this.storeSourceFailure(error, currentCopy().tapp.storeUpdateFailed)
    }
  }

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
      throw new Error(currentCopy().tapp.storeCannotEditOfficialUrl)
    }

    try {
      const response = await api.post(
        `/api/tapps/store/sources/${sourceId}`,
        patch,
      )

      if (!response.data?.success) {
        throw new Error(
          typeof response.data?.error === 'string' &&
            !isUselessErrorText(response.data.error)
            ? response.data.error
            : currentCopy().tapp.storeUpdateFailed,
        )
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
          error.response?.data?.error || currentCopy().tapp.storeCannotEditOfficialUrl,
        )
      }
      if (error.response?.status === 404) {
        throw new Error(currentCopy().tapp.storeSourceNotFound)
      }
      if (error.response?.status === 409) {
        throw new Error(currentCopy().tapp.storeUrlExists)
      }
      if (error.response?.status === 400) {
        throw new Error(error.response?.data?.error || currentCopy().tapp.storeInvalidSource)
      }
      throw this.storeSourceFailure(error, currentCopy().tapp.storeUpdateFailed)
    }
  }

  async refreshSources(): Promise<void> {
    this.sourcesLoaded = false
    await this.loadSourcesFromApi()
  }

  async fetchStoreIndex(
    source: RemoteStoreSource,
    forceRefresh = false,
  ): Promise<RemoteStoreIndex> {
    const cacheKey = source.url
    const now = Date.now()

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
    const knownUrls = new Set(this.cache.keys()).union(
      new Set(this.pendingIndexRequests.keys()),
    )
    for (const url of knownUrls) {
      if (!activeUrls.has(url)) this.clearCachedSource(url)
    }
  }

  private async fetchStoreIndexFromNetwork(
    source: RemoteStoreSource,
    cacheKey: string,
    signal: AbortSignal,
  ): Promise<RemoteStoreIndex> {
    try {
      const response = await fetch(
        this.withStoreCacheBust(source.url, this.newStoreDownloadSessionId()),
        {
          // 只发简单请求；见 storeResourceFetchInit 的 CORS 约束。
          headers: {
            Accept: 'application/json',
          },
          cache: 'no-store',
          credentials: 'omit',
          signal,
        },
      )

      if (!response.ok) {
        throw new Error(httpStatusMessage(response.status))
      }

      const data = (await response.json()) as RemoteStoreIndex
      if (signal.aborted)
        throw new DOMException('Request aborted', 'AbortError')

      if (!data.name || !data.apps || !Array.isArray(data.apps)) {
        throw new Error(currentCopy().tapp.storeInvalidIndex)
      }

      data.apps = data.apps.map((app) => ({
        ...app,
        preview: parseStorePreview(
          (app as RemoteApp & { preview?: unknown }).preview,
        ),
        locales: parseStoreLocales(
          (app as RemoteApp & { locales?: unknown }).locales,
        ),
      }))

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
        userFacingError(error, currentCopy().tapp.loadRemoteFailed),
      )
    }
  }

  async fetchPolicy(): Promise<{ federationEnabled: boolean }> {
    const response = await api.get('/api/tapps/store/policy')
    const policy = response.data?.data
    if (!response.data?.success || typeof policy?.federationEnabled !== 'boolean') {
      throw new Error(currentCopy().tapp.loadRemoteFailed)
    }
    return policy
  }

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
    federationEnabled: boolean
  }> {
    const { federationEnabled } = await this.fetchPolicy()
    const enabledSources = await this.getEnabledSources()
    const prioritizedSources = enabledSources
      .map((source, configuredIndex) => ({ source, configuredIndex }))
      .toSorted((a, b) => {
        const officialRank =
          Number(Boolean(b.source.official)) -
          Number(Boolean(a.source.official))
        return officialRank || a.configuredIndex - b.configuredIndex
      })
      .map(({ source }) => source)

    // 并行拉取；同 id 来源由源优先级决定，不是返回顺序。
    const results = await Promise.all(
      prioritizedSources.map(async (source) => {
        try {
          const index = await this.fetchStoreIndex(source, forceRefresh)
          return { source, index }
        } catch (error) {
          return {
            source,
            error: userFacingError(error, currentCopy().tapp.loadRemoteFailed),
          }
        }
      }),
    )

    // 同 id：官方源优先，其余按配置顺序；只产生一个安装来源。
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
          if (!isStoreAppAvailable(app, federationEnabled)) continue
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
    }

    return {
      apps,
      sources: results.map((r) => ({ source: r.source, error: r.error })),
      federationEnabled,
    }
  }

  async fetchCategories(source: RemoteStoreSource): Promise<RemoteCategory[]> {
    const index = await this.fetchStoreIndex(source)
    return index.categories || []
  }

  /** 包下载须绕过中间缓存。禁止 Cache-Control/Pragma 请求头（非 CORS 简单头，会预检失败）。用 cache:no-store + query bust。 */
  private storeResourceFetchInit(
    extraHeaders?: Record<string, string>,
  ): RequestInit {
    const headers: Record<string, string> = {
      // Accept 保持 CORS 简单头。
      ...(extraHeaders || {}),
    }
    return {
      cache: 'no-store',
      // 公共商店 URL 不带 credentials。
      credentials: 'omit',
      headers,
    }
  }

  /** 同一次下载共用 bust id；不复用上次会话。 */
  private newStoreDownloadSessionId(): string {
    if (
      typeof crypto !== 'undefined' &&
      typeof crypto.randomUUID === 'function'
    ) {
      return crypto.randomUUID().replaceAll('-', '')
    }
    return `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}`
  }

  /** 所有商店宿主都 bust；不靠非简单请求头。 */
  private withStoreCacheBust(url: string, sessionId: string): string {
    try {
      const parsed = new URL(url)
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
      throw new Error(
        `${currentCopy().tapp.loadAppFailed} (HTTP ${response.status})`,
      )
    }

    const manifest = (await response.json()) as TappManifest
    return manifest
  }

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
      throw new Error(
        `${currentCopy().tapp.appCodeLoadFailed} (HTTP ${response.status})`,
      )
    }

    const code = await response.text()
    return code
  }

  /** 优先 catalog preview；否则 page_template。不用已安装包 HTML。 */
  async downloadAppPreview(
    app: RemoteApp,
    baseUrl: string,
    snapshot = app.preview,
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

    if (snapshot?.html) {
      const [html, ...styles] = await Promise.all([
        downloadText(snapshot.html),
        ...snapshot.styles.map((path) => downloadText(path)),
      ])
      return {
        html,
        css: styles.filter(Boolean).join('\n') || undefined,
      }
    }

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

  /** 浏览器整包下载，供后端无法出站时走 direct。 */
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
    /** 作者 widget 层 CSS，不是宿主预编译产物。 */
    widgetStyles?: string
    pageCss?: string
    pageTemplate?: string
    widgetTemplates?: Record<string, Record<string, string>>
    i18n?: Record<string, unknown>
    modules?: Record<string, string>
    assets?: Record<string, string>
  }> {
    const { federationEnabled } = await this.fetchPolicy()
    if (!isStoreAppAvailable(app, federationEnabled)) {
      throw new Error(currentCopy().errors.federationDisabledRegion)
    }
    const baseUrl = storeIndex.base_url || this.deriveBaseUrl(storeIndex)
    const { clampInstallPercent } = await import('../utils/tappInstallProgress')
    const report = options?.onProgress
    // 同一次下载共用 bust id；不复用上次会话。
    const downloadSessionId = this.newStoreDownloadSessionId()

    const downloadText = async (
      relativePath?: string,
      requiredLabel?: string,
    ): Promise<string | undefined> => {
      if (!relativePath) {
        if (requiredLabel) {
          throw new Error(
            formatCurrent(currentCopy().tapp.storeDownloadFailed, {
              name: requiredLabel,
            }),
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
              userFacingError(
                `HTTP ${response.status}`,
                formatCurrent(currentCopy().tapp.storeDownloadFailed, {
                  name: requiredLabel,
                }),
              ),
            )
          }
          return undefined
        }
        return await response.text()
      } catch (e) {
        if (requiredLabel) {
          throw e instanceof Error
            ? e
            : new Error(
                userFacingError(
                  e,
                  formatCurrent(currentCopy().tapp.storeDownloadFailed, {
                    name: requiredLabel,
                  }),
                ),
              )
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
    // 先下 manifest，才能知道哪些层资源必填。
    const downloadedManifest = await this.downloadManifest(
      app,
      indexWithBase,
      downloadSessionId,
    )
    const manifest: TappManifest = downloadedManifest
    if (!isStoreAppAvailable(manifest, federationEnabled)) {
      throw new Error(currentCopy().errors.federationDisabledRegion)
    }

    // catalog version 必须等于刚拉到的包。
    if (
      app.version &&
      manifest.version &&
      app.version.trim() !== manifest.version.trim()
    ) {
      throw new Error(
        formatCurrent(currentCopy().tapp.storeVersionMismatch, {
          catalog: app.version.trim(),
          manifest: manifest.version.trim(),
        }),
      )
    }

    const needsPageCss = !!manifest.page?.styles
    const needsPageTemplate = !!manifest.page?.template
    const needsWidgetCss = (manifest.widgets ?? []).some(
      (widget) => !!widget.styles,
    )

    const [code, styles, widgetStyles, pageCss, pageTemplate] = await Promise.all([
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
        'Downloaded package is missing page.styles content. Check store download.page_styles.',
      )
    }
    if (needsPageTemplate && !pageTemplate) {
      throw new Error(
        'Downloaded package is missing page.template content. Check store download.page_template.',
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

    // download.modules 的 key 是包内相对路径。
    let modules: Record<string, string> | undefined
    if (app.download.modules) {
      const entries = Object.entries(app.download.modules)
      const downloaded: Record<string, string> = {}
      const total = entries.length
      let done = 0
      const concurrency = 4
      let next = 0
      const worker = async () => {
        while (next < entries.length) {
          const index = next++
          const [relative, path] = entries[index]!
          const content = await downloadText(path, `module ${relative}`)
          if (!content) {
            throw new Error(
              formatCurrent(currentCopy().tapp.storeDownloadFailed, {
                name: `${relative}`,
              }),
            )
          }
          downloaded[relative] = content
          done++
          const frac = total > 0 ? done / total : 1
          report?.({
            phase: 'download',
            message: 'download',
            percent: clampInstallPercent(20 + frac * 55),
            detail: relative,
          })
        }
      }
      await Promise.all(
        Array.from({ length: Math.min(concurrency, Math.max(1, total)) }, () =>
          worker(),
        ),
      )
      if (Object.keys(downloaded).length > 0) modules = downloaded
    }

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
      widgetStyles,
      pageCss,
      pageTemplate,
      widgetTemplates,
      i18n,
      modules,
      assets,
    }
  }

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
    const maxAssets = maxDeclaredAssets(manifest)
    if (declared.length > maxAssets) {
      throw new Error(
        `Tapp assets accepts at most ${maxAssets} entries (got ${declared.length})`,
      )
    }

    const { clampInstallPercent } = await import('../utils/tappInstallProgress')
    const report = options?.onProgress
    const out: Record<string, string> = {}
    const total = declared.length
    let completed = 0

    // 并发上限，避免打满浏览器。
    const concurrency = 4
    let nextIndex = 0

    const worker = async () => {
      while (nextIndex < declared.length) {
        const i = nextIndex++
        const assetPath = declared[i]!
        if (!assetPath.startsWith('assets/')) {
          throw new Error(
            userFacingError(
              assetPath,
              currentCopy().tapp.installFailed,
            ),
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
            userFacingError(
              `HTTP ${response.status}`,
              formatCurrent(currentCopy().tapp.storeDownloadFailed, {
                name: assetPath,
              }),
            ),
          )
        }
        const buffer = await response.arrayBuffer()
        out[assetPath] = arrayBufferToBase64(buffer)
        completed += 1
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

  private deriveBaseUrl(storeIndex: RemoteStoreIndex): string {
    return storeIndex.base_url || ''
  }

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

  private resolveUrl(relativePath: string, baseUrl: string): string {
    if (
      relativePath.startsWith('http://') ||
      relativePath.startsWith('https://')
    ) {
      return relativePath
    }
    const base = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`
    return base + relativePath
  }

  clearCache(): void {
    this.cache.clear()
  }

  getCacheStatus(): { count: number; oldestEntry: number | null } {
    const entries = Iterator.from(this.cache.values()).toArray()
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

export const RemoteStoreService = new RemoteStoreServiceImpl()

export default RemoteStoreService
