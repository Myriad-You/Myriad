import type { TappManifest } from '../types'
import type { TappListItem } from './TappLifecycleApi'
import type { TappPlaygroundCode } from './TappPlaygroundService'
import { API_URL } from '../../config'
import { hostLocaleHeaders } from '../../i18n/hostLocaleHeaders'
import { currentCopy, formatCurrent } from '../../i18n/localeCopy'
import { parseApiErrorBody } from '../../services/api'
import { getCSRFToken } from '../../utils/csrf'
import { generateOnDemandTailwindCSS } from '../runtime/sandbox/styles'
import { tappLayerEntries } from '../utils/manifestLayers'

import {
  buildPlaygroundPackageFiles,
  packageFilesToDirectInstallBody,
} from '../utils/playgroundPackageFiles'
import { apiRequest, TappHttpError } from './TappHttpClient'

/** 商店模块表须覆盖每个层入口；缺一个就拒绝。 */
export function storeInstallModules(
  manifest: TappManifest,
  code: string,
  downloaded: Record<string, string> | undefined,
): Record<string, string> {
  const modules: Record<string, string> = { ...(downloaded || {}) }
  if (manifest.core?.entry) modules[manifest.core.entry] = code
  for (const entry of tappLayerEntries(manifest)) {
    if (!Object.hasOwn(modules, entry)) {
      throw new Error(
        `Store index is missing download.modules entry for declared layer entry ${entry}`,
      )
    }
  }
  return modules
}

/** download.widget_styles 摊给声明了 styles 的 widget。走 widgetStyles（作者样式），不是 widgetCss（宿主预编译）。 */
export function storeWidgetStyles(
  manifest: TappManifest,
  content: string | undefined,
): Record<string, string> | undefined {
  const declared = (manifest.widgets ?? []).filter((widget) => widget.styles)
  if (declared.length === 0) return undefined
  if (content == null || content === '') {
    throw new Error(
      `Store package is missing widget styles for ${declared
        .map((widget) => `${widget.id}=${widget.styles}`)
        .join(', ')}`,
    )
  }
  return Object.fromEntries(
    declared.map((widget) => [widget.id, content] as const),
  )
}

interface CompiledCssPayload {
  widgetCss?: string
  pageCss?: string
}

export interface InstallTappRequest {
  source: 'direct' | 'store'
  manifest?: TappManifest
  /** 包内 .js：相对路径 → 源码，须覆盖每个层入口。 */
  modules?: Record<string, string>
  coreStyles?: string
  pageStyles?: string
  widgetStyles?: Record<string, string>
  pageTemplate?: string
  widgetTemplates?: Record<string, Record<string, string>>
  /** 宿主预编译；与作者 widgetStyles / pageStyles 分通道。 */
  widgetCss?: string
  pageCss?: string
  i18n?: Record<string, unknown>
  assets?: Record<string, string>
  storeSource?: string
  tappId?: string
  permissions?: string[]
}

export type DirectInstallPackage = Omit<InstallTappRequest, 'source' | 'storeSource' | 'tappId'> & {
  manifest: TappManifest
  modules: Record<string, string>
}

export async function installTapp(
  manifest: TappManifest,
  code: TappPlaygroundCode,
  permissions?: string[],
  compiledCss?: CompiledCssPayload,
): Promise<TappListItem> {
  const requestBody = buildDirectTappRequest(
    manifest,
    code,
    permissions,
    compiledCss,
  )

  return apiRequest('/api/tapps/install', {
    method: 'POST',
    body: JSON.stringify(requestBody),
  })
}

function buildDirectTappRequest(
  manifest: TappManifest,
  code: TappPlaygroundCode,
  permissions?: string[],
  compiledCss?: CompiledCssPayload,
): InstallTappRequest {
  const pkg = buildPlaygroundPackageFiles(manifest, code)
  const mapped = packageFilesToDirectInstallBody(pkg, code.assets)

  const requestBody: InstallTappRequest = {
    source: 'direct',
    manifest: mapped.manifest,
    modules: mapped.modules,
    permissions,
  }

  if (mapped.coreStyles !== undefined) {
    requestBody.coreStyles = mapped.coreStyles
  }
  if (mapped.pageTemplate !== undefined) {
    requestBody.pageTemplate = mapped.pageTemplate
  }
  if (mapped.widgetTemplates) {
    requestBody.widgetTemplates = mapped.widgetTemplates
  }
  if (mapped.i18n) {
    requestBody.i18n = mapped.i18n
  }
  if (mapped.assets) {
    requestBody.assets = mapped.assets
  }
  if (compiledCss?.widgetCss !== undefined) {
    requestBody.widgetCss = compiledCss.widgetCss
  }
  if (compiledCss?.pageCss !== undefined) {
    requestBody.pageCss = compiledCss.pageCss
  }

  return requestBody
}

export async function installFromCode(
  manifest: TappManifest,
  code: TappPlaygroundCode,
): Promise<TappListItem> {
  const widgetSources = [
    code.widgetHtml || '',
    code.styles || '',
    code.core || '',
    code.widget || '',
  ].join('\n')
  const widgetCss = generateOnDemandTailwindCSS(widgetSources)

  const pageSources = [
    code.pageHtml || '',
    code.styles || '',
    code.core || '',
    code.page || '',
  ].join('\n')
  const pageCss = generateOnDemandTailwindCSS(pageSources)

  return installTapp(manifest, code, manifest.permissions, {
    widgetCss,
    pageCss,
  })
}

export async function updateTappFromCode(
  manifest: TappManifest,
  code: TappPlaygroundCode,
): Promise<TappListItem> {
  const widgetSources = [
    code.widgetHtml || '',
    code.styles || '',
    code.core || '',
    code.widget || '',
  ].join('\n')
  const widgetCss = generateOnDemandTailwindCSS(widgetSources)

  const pageSources = [
    code.pageHtml || '',
    code.styles || '',
    code.core || '',
    code.page || '',
  ].join('\n')
  const pageCss = generateOnDemandTailwindCSS(pageSources)

  const requestBody = buildDirectTappRequest(
    manifest,
    code,
    manifest.permissions,
    { widgetCss, pageCss },
  )
  return apiRequest<TappListItem>(
    `/api/tapps/${encodeURIComponent(manifest.id)}/update`,
    {
      method: 'POST',
      body: JSON.stringify(requestBody),
    },
  )
}

export async function installTappFile(
  file: File,
  permissions?: string[],
  overwrite?: boolean,
): Promise<TappListItem> {
  const formData = new FormData()
  formData.append('file', file)
  if (permissions) {
    formData.append('permissions', JSON.stringify(permissions))
  }

  const csrfToken = (await getCSRFToken()) || ''

  const query = overwrite ? '?overwrite=true' : ''
  const response = await fetch(`${API_URL}/api/tapps/install-file${query}`, {
    method: 'POST',
    headers: {
      'X-CSRF-Token': csrfToken,
      ...hostLocaleHeaders(),
    },
    body: formData,
    credentials: 'include',
  })

  if (!response.ok) {
    const errorData = await response.json().catch(() => ({}))
    const parsed = parseApiErrorBody(errorData, response.status)
    throw new TappHttpError(
      parsed.message || currentCopy().tapp.installFailed,
      response.status,
      { body: errorData, code: parsed.code },
    )
  }

  const result = await response.json()
  if (result.success === false) {
    throw new Error(result.error || currentCopy().tapp.installFailed)
  }
  return result.data || result
}

/** source 是目录 URL 或商店源 id，不是 mode 字符串 store。 */
export interface InstallFromStoreRequest {
  source: string
  tappId: string
  permissions?: string[]
}

/** 官方目录 URL（跨实例可移植；不用本地 DB id）。 */
export const OFFICIAL_TAPP_STORE_URL =
  'https://raw.githubusercontent.com/Myriad-You/tapp-store/main/index.json'

function isHttpStoreSource(value: string | undefined | null): boolean {
  if (!value) return false
  const v = value.trim().toLowerCase()
  return v.startsWith('https://') || v.startsWith('http://')
}

function isInstallModePlaceholder(value: string | undefined | null): boolean {
  if (!value) return true
  const v = value.trim().toLowerCase()
  return v === 'store' || v === 'direct' || v === ''
}

export function normalizeStoreCatalogUrl(url: string): string {
  return url
    .trim()
    .replaceAll(/\/+$/g, '')
    .replaceAll(/\/index\.json$/gi, '')
}

/** 解析 tappId 所在目录，返回 URL，不是本地 DB id。 */
export async function resolveStoreSourceForTapp(tappId: string): Promise<{
  storeSource: string
  sourceName?: string
  matchedApp: boolean
}> {
  const { default: RemoteStoreService, OFFICIAL_STORE } = await import(
    './RemoteStoreService',
  )
  const sources = await RemoteStoreService.getEnabledSources()
  const ordered = sources.toSorted((a, b) => {
    if (a.official && !b.official) return -1
    if (!a.official && b.official) return 1
    return 0
  })

  for (const source of ordered) {
    try {
      const index = await RemoteStoreService.fetchStoreIndex(source)
      if (index.apps?.some((app) => app.id === tappId)) {
        const url = source.url.includes('index.json')
          ? source.url
          : `${normalizeStoreCatalogUrl(source.url)}/index.json`
        return {
          storeSource: url,
          sourceName: source.name,
          matchedApp: true,
        }
      }
    } catch (e) {
      console.warn('[Tapp] resolveStoreSource: index fetch failed', source.url, e)
    }
  }

  const fallback =
    ordered.find((s) => s.official)?.url ||
    OFFICIAL_STORE.url ||
    OFFICIAL_TAPP_STORE_URL
  return {
    storeSource: fallback.includes('index.json')
      ? fallback
      : `${normalizeStoreCatalogUrl(fallback)}/index.json`,
    sourceName: ordered.find((s) => s.official)?.name || 'Myriad Official',
    matchedApp: false,
  }
}

export interface InstallFromStoreOptions {
  onProgress?: import('../utils/tappInstallProgress').TappInstallProgressCallback
  estimatedBytes?: number
}

/** 优先后端 store 安装；502 时浏览器下载 + direct。source 必须是目录 URL/id。 */
export async function installFromStore(
  request: InstallFromStoreRequest,
  options?: InstallFromStoreOptions,
): Promise<TappListItem> {
  if (isInstallModePlaceholder(request.source)) {
    throw new Error(
      'Invalid storeSource: expected catalog URL or store source id, not install mode "store"',
    )
  }

  const { clampInstallPercent } = await import('../utils/tappInstallProgress')
  const report = options?.onProgress

  // 仅后端出站失败时才由浏览器代下。
  try {
    report?.({
      phase: 'install',
      message: 'server',
      percent: 30,
    })
    const result = await apiRequest<TappListItem>('/api/tapps/install', {
      method: 'POST',
      body: JSON.stringify({
        source: 'store',
        storeSource: request.source,
        tappId: request.tappId,
        permissions: request.permissions,
      }),
    })
    report?.({
      phase: 'done',
      message: 'done',
      percent: 100,
    })
    try {
      const { clearStoreStatsCache } = await import('./storeStats')
      clearStoreStatsCache()
    } catch {
    }
    return result
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    const shouldFallback =
      (error instanceof TappHttpError && error.status === 502) ||
      /502|BAD_GATEWAY|Failed to fetch store|cannot reach store|Failed to fetch manifest|Failed to fetch code|Failed to fetch|NetworkError|ECONNREFUSED|timeout|Load failed|Store source not found|not found/i.test(
        message,
      )

    if (!shouldFallback) {
      throw error
    }

    console.warn(
      '[Tapp] Backend store install failed, falling back to client-side download:',
      message,
    )
    report?.({
      phase: 'download',
      message: 'download',
      percent: clampInstallPercent(5),
    })
    return installFromStoreViaClient(request, options)
  }
}

export async function installDirect(
  packagePayload: DirectInstallPackage,
): Promise<TappListItem> {
  if (
    !packagePayload?.manifest ||
    !packagePayload.modules ||
    Object.keys(packagePayload.modules).length === 0
  ) {
    throw new Error('Direct install requires manifest and modules')
  }
  const requestBody: InstallTappRequest = {
    source: 'direct',
    manifest: packagePayload.manifest,
    modules: packagePayload.modules,
    permissions:
      packagePayload.permissions ?? packagePayload.manifest.permissions,
  }
  if (packagePayload.coreStyles !== undefined) {
    requestBody.coreStyles = packagePayload.coreStyles
  }
  if (packagePayload.pageStyles !== undefined) {
    requestBody.pageStyles = packagePayload.pageStyles
  }
  if (packagePayload.widgetStyles) {
    requestBody.widgetStyles = packagePayload.widgetStyles
  }
  if (packagePayload.pageTemplate !== undefined) {
    requestBody.pageTemplate = packagePayload.pageTemplate
  }
  if (packagePayload.widgetTemplates) {
    requestBody.widgetTemplates = packagePayload.widgetTemplates
  }
  if (packagePayload.widgetCss !== undefined) {
    requestBody.widgetCss = packagePayload.widgetCss
  }
  if (packagePayload.pageCss !== undefined) {
    requestBody.pageCss = packagePayload.pageCss
  }
  if (packagePayload.i18n) requestBody.i18n = packagePayload.i18n
  if (packagePayload.assets) requestBody.assets = packagePayload.assets

  return apiRequest('/api/tapps/install', {
    method: 'POST',
    body: JSON.stringify(requestBody),
  })
}

export async function buildInstallPackageFromInstalled(
  tappId: string,
  options?: { maxBytes?: number },
): Promise<{
  package: DirectInstallPackage | null
  sizeBytes: number
  omitted: boolean
  reason?: string
}> {
  const { getTapp } = await import('./TappLifecycleApi')
  const { getTappResources } = await import('./TappPackageResourceApi')

  const detail = await getTapp(tappId)
  const resources = await getTappResources(tappId)

  const pkg: DirectInstallPackage = {
    manifest: detail.manifest,
    modules: resources.modules || {},
    permissions: detail.granted_permissions?.length
      ? detail.granted_permissions
      : detail.manifest.permissions,
  }
  if (resources.coreStyles) pkg.coreStyles = resources.coreStyles
  if (resources.pageStyles) pkg.pageStyles = resources.pageStyles
  if (resources.widgetStyles) pkg.widgetStyles = resources.widgetStyles
  if (resources.pageTemplate) pkg.pageTemplate = resources.pageTemplate
  if (resources.widgetTemplates) pkg.widgetTemplates = resources.widgetTemplates
  // 宿主预编译产物按安装 API 的字段名传递，与作者层样式分开。
  if (resources.widgetCSS) pkg.widgetCss = resources.widgetCSS
  if (resources.pageCSS) pkg.pageCss = resources.pageCSS
  if (resources.i18n) pkg.i18n = resources.i18n

  if (Object.keys(pkg.modules).length === 0) {
    return {
      package: null,
      sizeBytes: 0,
      omitted: true,
      reason: 'Installed Tapp has no code to share',
    }
  }

  let serialized = JSON.stringify(pkg)
  let sizeBytes = new Blob([serialized]).size
  const max = options?.maxBytes

  if (max != null && sizeBytes > max) {
    delete pkg.assets
    delete pkg.i18n
    serialized = JSON.stringify(pkg)
    sizeBytes = new Blob([serialized]).size
    if (sizeBytes > max) {
      return {
        package: null,
        sizeBytes,
        omitted: true,
        reason: `Package too large to share (${sizeBytes} bytes, max ${max})`,
      }
    }
  }

  return { package: pkg, sizeBytes, omitted: false }
}

/** 浏览器下载后 direct 安装。即使对端没加该源，也接受目录 URL。 */
async function installFromStoreViaClient(
  request: InstallFromStoreRequest,
  options?: InstallFromStoreOptions,
): Promise<TappListItem> {
  const { default: RemoteStoreService } = await import('./RemoteStoreService')
  const { clampInstallPercent } = await import('../utils/tappInstallProgress')
  const report = options?.onProgress

  if (isInstallModePlaceholder(request.source)) {
    throw new Error(
      'Invalid storeSource for client install: expected catalog URL',
    )
  }

  const sources = await RemoteStoreService.getSources()
  const reqNorm = normalizeStoreCatalogUrl(request.source)
  let source = sources.find(
    (s) =>
      String(s.id) === request.source ||
      normalizeStoreCatalogUrl(s.url) === reqNorm,
  )

  if (!source && isHttpStoreSource(request.source)) {
    const url = request.source.includes('index.json')
      ? request.source.trim()
      : `${reqNorm}/index.json`
    source = {
      name: 'Shared catalog',
      url,
      enabled: true,
    }
  }

  if (!source) {
    throw new Error(
      `Store source not configured on this instance: ${request.source}. Add this catalog in Tapp Store settings, or use the official Myriad store.`,
    )
  }

  report?.({
    phase: 'prepare',
    message: 'prepare',
    percent: 2,
  })

  RemoteStoreService.clearCache()
  const index = await RemoteStoreService.fetchStoreIndex(source, true)
  const baseUrl =
    index.base_url ||
    source.url.replaceAll(/\/index\.json$/g, '').replaceAll(/\/$/g, '')
  const storeIndex = { ...index, base_url: baseUrl }

  const app = storeIndex.apps.find((a) => a.id === request.tappId)
  if (!app) {
    throw new Error(
      formatCurrent(currentCopy().tapp.storeAppNotFound, { id: request.tappId }),
    )
  }

  report?.({
    phase: 'download',
    message: 'download',
    percent: 5,
  })

  const pkg = await RemoteStoreService.downloadAppPackage(app, storeIndex, {
    onProgress: report,
    estimatedBytes: options?.estimatedBytes ?? app.size,
  })

  report?.({
    phase: 'install',
    message: 'register',
    percent: clampInstallPercent(92),
  })

  const modules = storeInstallModules(pkg.manifest, pkg.code, pkg.modules)

  const requestBody: InstallTappRequest = {
    source: 'direct',
    manifest: pkg.manifest,
    modules,
    permissions: request.permissions ?? pkg.manifest.permissions,
  }

  if (pkg.styles) requestBody.coreStyles = pkg.styles
  if (pkg.pageTemplate) requestBody.pageTemplate = pkg.pageTemplate
  if (pkg.widgetTemplates) requestBody.widgetTemplates = pkg.widgetTemplates
  // 作者声明了层样式就必须带上内容；宿主预编译顶不了它。
  const widgetStyles = storeWidgetStyles(pkg.manifest, pkg.widgetStyles)
  if (widgetStyles) requestBody.widgetStyles = widgetStyles
  if (pkg.pageCss != null && pkg.pageCss !== '') {
    requestBody.pageStyles = pkg.pageCss
  } else if (pkg.manifest.page?.styles) {
    throw new Error(
      `Client install package is missing page styles for manifest.page.styles=${pkg.manifest.page.styles}`,
    )
  }
  if (pkg.i18n) requestBody.i18n = pkg.i18n
  if (pkg.assets && Object.keys(pkg.assets).length > 0) {
    requestBody.assets = pkg.assets
  }

  const result = await apiRequest<TappListItem>('/api/tapps/install', {
    method: 'POST',
    body: JSON.stringify(requestBody),
  })

  try {
    const { reportStoreInstallHit, clearStoreStatsCache } = await import(
      './storeStats',
    )
    reportStoreInstallHit({
      appId: pkg.manifest.id,
      version: pkg.manifest.version,
      event: 'install',
    })
    clearStoreStatsCache()
  } catch {
    // 不阻塞安装。
  }

  report?.({
    phase: 'done',
    message: 'done',
    percent: 100,
  })
  return result
}

export interface UninstallOptions {
  keepData?: boolean
}

export async function uninstallTapp(
  tappId: string,
  options?: UninstallOptions,
): Promise<void> {
  const params = new URLSearchParams()
  if (options?.keepData) {
    params.set('keep_data', 'true')
  }
  const queryString = params.toString()
  const url = `/api/tapps/${encodeURIComponent(tappId)}${queryString ? `?${queryString}` : ''}`

  return apiRequest(url, {
    method: 'DELETE',
  })
}

export interface UpdateTappFromStoreRequest {
  source: string
  permissions?: string[]
}

export async function updateTappFromStore(
  tappId: string,
  request: UpdateTappFromStoreRequest,
  options?: InstallFromStoreOptions,
): Promise<TappListItem> {
  if (isInstallModePlaceholder(request.source)) {
    throw new Error(
      'Invalid storeSource: expected catalog URL or store source id, not install mode "store"',
    )
  }

  const { clampInstallPercent } = await import('../utils/tappInstallProgress')

  try {
    return await apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/update`, {
      method: 'POST',
      body: JSON.stringify({
        source: 'store',
        storeSource: request.source,
        permissions: request.permissions,
      }),
    })
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    const shouldFallback =
      (error instanceof TappHttpError && error.status === 502) ||
      /502|BAD_GATEWAY|Failed to fetch store|cannot reach store|Failed to fetch manifest|Failed to fetch code|Failed to fetch|NetworkError|ECONNREFUSED|timeout|Load failed|Store source not found|not found|413|Payload Too Large|body.*limit|too large/i.test(
        message,
      )
    if (!shouldFallback) throw error
    console.warn(
      '[Tapp] Backend store update failed, falling back to client-side download:',
      message,
    )
    options?.onProgress?.({
      phase: 'download',
      message: 'download',
      percent: clampInstallPercent(5),
    })
    return updateFromStoreViaClient(tappId, request, options)
  }
}

async function updateFromStoreViaClient(
  tappId: string,
  request: UpdateTappFromStoreRequest,
  options?: InstallFromStoreOptions,
): Promise<TappListItem> {
  const { default: RemoteStoreService } = await import('./RemoteStoreService')
  const { clampInstallPercent } = await import('../utils/tappInstallProgress')
  const report = options?.onProgress

  const sources = await RemoteStoreService.getSources()
  const reqNorm = normalizeStoreCatalogUrl(request.source)
  let source = sources.find(
    (s) =>
      String(s.id) === request.source ||
      normalizeStoreCatalogUrl(s.url) === reqNorm,
  )
  if (!source && isHttpStoreSource(request.source)) {
    const url = request.source.includes('index.json')
      ? request.source.trim()
      : `${reqNorm}/index.json`
    source = { name: 'Shared catalog', url, enabled: true }
  }
  if (!source) {
    throw new Error(
      `Store source not configured on this instance: ${request.source}`,
    )
  }

  report?.({ phase: 'prepare', message: 'prepare', percent: 2 })
  RemoteStoreService.clearCache()
  const index = await RemoteStoreService.fetchStoreIndex(source, true)
  const baseUrl =
    index.base_url ||
    source.url.replaceAll(/\/index\.json$/g, '').replaceAll(/\/$/g, '')
  const storeIndex = { ...index, base_url: baseUrl }
  const app = storeIndex.apps.find((a) => a.id === tappId)
  if (!app) {
    throw new Error(
      formatCurrent(currentCopy().tapp.storeAppNotFound, { id: tappId }),
    )
  }

  report?.({ phase: 'download', message: 'download', percent: 5 })
  const pkg = await RemoteStoreService.downloadAppPackage(app, storeIndex, {
    onProgress: report,
    estimatedBytes: options?.estimatedBytes ?? app.size,
  })

  report?.({
    phase: 'install',
    message: 'register',
    percent: clampInstallPercent(92),
  })

  const modules = storeInstallModules(pkg.manifest, pkg.code, pkg.modules)

  const body: Record<string, unknown> = {
    source: 'direct',
    manifest: pkg.manifest,
    modules,
    permissions: request.permissions ?? pkg.manifest.permissions,
  }
  if (pkg.styles) body.coreStyles = pkg.styles
  if (pkg.pageTemplate) body.pageTemplate = pkg.pageTemplate
  if (pkg.widgetTemplates) body.widgetTemplates = pkg.widgetTemplates
  const updateWidgetStyles = storeWidgetStyles(pkg.manifest, pkg.widgetStyles)
  if (updateWidgetStyles) body.widgetStyles = updateWidgetStyles
  if (pkg.pageCss != null && pkg.pageCss !== '') {
    body.pageStyles = pkg.pageCss
  } else if (pkg.manifest.page?.styles) {
    throw new Error(
      `Client update package is missing page styles for manifest.page.styles=${pkg.manifest.page.styles}`,
    )
  }
  if (pkg.i18n) body.i18n = pkg.i18n
  if (pkg.assets && Object.keys(pkg.assets).length > 0) body.assets = pkg.assets

  const result = await apiRequest<TappListItem>(
    `/api/tapps/${encodeURIComponent(tappId)}/update`,
    { method: 'POST', body: JSON.stringify(body) },
  )

  try {
    const { reportStoreInstallHit, clearStoreStatsCache } = await import(
      './storeStats',
    )
    reportStoreInstallHit({
      appId: pkg.manifest.id,
      version: pkg.manifest.version,
      event: 'update',
    })
    clearStoreStatsCache()
  } catch {
    // 不阻塞更新。
  }
  report?.({ phase: 'done', message: 'done', percent: 100 })
  return result
}

export async function cleanupTemporaryTapps(): Promise<number> {
  const result = await apiRequest<number>('/api/tapps/cleanup-temporary', {
    method: 'POST',
  })
  return result
}
