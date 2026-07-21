/** Tapp installation, update, uninstall and temporary cleanup operations. */

import type { TappCodeStructure, TappManifest } from '../types'
import type { TappListItem } from './TappLifecycleApi'
import { API_URL } from '../../config'
import { getCSRFToken } from '../../utils/csrf'
import { generateOnDemandTailwindCSS } from '../runtime/sandbox/styles'
import {
  buildPlaygroundPackageFiles,
  packageFilesToDirectInstallBody,
} from '../utils/playgroundPackageFiles'
import { apiRequest } from './TappHttpClient'

interface CompiledCssPayload {
  widgetCss?: string
  pageCss?: string
}

/**
 * 安装 Tapp 请求体（统一格式）
 */
export interface InstallTappRequest {
  source: 'direct' | 'store'
  // direct 模式
  manifest?: TappManifest
  code?: string
  styles?: string
  pageTemplate?: string
  widgetTemplates?: Record<string, Record<string, string>>
  /** Widget 专用 CSS */
  widgetCss?: string
  /** Page 专用 CSS */
  pageCss?: string
  /** i18n 翻译数据 (lang_code → JSON) */
  i18n?: Record<string, unknown>
  /** Page 模块文件 (filename → code) */
  pageModules?: Record<string, string>
  /** Package assets (path → base64) */
  assets?: Record<string, string>
  // store 模式
  storeSource?: string
  tappId?: string
  // 通用
  permissions?: string[]
}

/**
 * Direct-install package payload (share / peer install).
 * Matches POST /api/tapps/install source=direct body minus `source`.
 */
export type DirectInstallPackage = Omit<InstallTappRequest, 'source' | 'storeSource' | 'tappId'> & {
  manifest: TappManifest
  code: string
}

/**
 * 安装 Tapp（统一接口，发送 JSON）
 *
 * @param manifest - Tapp 清单
 * @param code - Tapp 代码结构
 * @param permissions - 授权的权限
 */
export async function installTapp(
  manifest: TappManifest,
  code: TappCodeStructure,
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

/**
 * Map structured code → direct-install JSON body using the same package file
 * map as Playground .tapp export (`buildPlaygroundPackageFiles`).
 * Install API shape is unchanged; only the source of path/content mapping is shared.
 */
function buildDirectTappRequest(
  manifest: TappManifest,
  code: TappCodeStructure,
  permissions?: string[],
  compiledCss?: CompiledCssPayload,
): InstallTappRequest {
  const pkg = buildPlaygroundPackageFiles(manifest, code)
  const mapped = packageFilesToDirectInstallBody(pkg, code.assets)

  const requestBody: InstallTappRequest = {
    source: 'direct',
    manifest: mapped.manifest,
    code: mapped.code,
    permissions,
  }

  if (mapped.styles !== undefined) {
    requestBody.styles = mapped.styles
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
  if (mapped.pageModules) {
    requestBody.pageModules = mapped.pageModules
  }
  if (mapped.assets) {
    requestBody.assets = mapped.assets
  }
  // Generated Tailwind CSS is part of the installation generation. Include
  // empty strings as well so an update can remove previously generated CSS.
  if (compiledCss?.widgetCss !== undefined) {
    requestBody.widgetCss = compiledCss.widgetCss
  }
  if (compiledCss?.pageCss !== undefined) {
    requestBody.pageCss = compiledCss.pageCss
  }

  return requestBody
}

/**
 * 从代码和清单安装 Tapp（用于示例 Tapp）
 * 支持完整的代码结构，包括 CSS 和 HTML 模板
 *
 * 🎯 自动生成分离式预编译 Tailwind CSS（widget.css 和 page.css）
 */
export async function installFromCode(
  manifest: TappManifest,
  code: TappCodeStructure,
): Promise<TappListItem> {
  // 🎯 生成 Widget 专用 CSS
  const widgetSources = [
    code.widgetHtml || '',
    code.styles || '',
    code.core || '',
    code.widget || '',
  ].join('\n')
  const widgetCss = generateOnDemandTailwindCSS(widgetSources)

  // 🎯 生成 Page 专用 CSS
  const pageSources = [
    code.pageHtml || '',
    code.styles || '',
    code.core || '',
    code.page || '',
    ...Object.values(code.pageModules || {}),
  ].join('\n')
  const pageCss = generateOnDemandTailwindCSS(pageSources)

  // CSS and source resources enter the same backend staging generation.
  return installTapp(manifest, code, manifest.permissions, {
    widgetCss,
    pageCss,
  })
}

/**
 * 从代码和清单更新 Tapp（用于内置示例 Tapp）。
 *
 * 保留后端存储数据，仅覆盖 manifest、代码和资源。
 */
export async function updateTappFromCode(
  manifest: TappManifest,
  code: TappCodeStructure,
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
    ...Object.values(code.pageModules || {}),
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

/**
 * 上传 .tapp 文件安装（multipart 文件上传）
 *
 * @param file .tapp 文件
 * @param permissions 授权的权限列表（可选）
 * @returns 安装后的 Tapp 信息
 */
export async function installTappFile(
  file: File,
  permissions?: string[],
): Promise<TappListItem> {
  const formData = new FormData()
  formData.append('file', file)
  if (permissions) {
    formData.append('permissions', JSON.stringify(permissions))
  }

  const csrfToken = (await getCSRFToken()) || ''

  const response = await fetch(`${API_URL}/api/tapps/install-file`, {
    method: 'POST',
    headers: {
      'X-CSRF-Token': csrfToken,
    },
    body: formData,
    credentials: 'include',
  })

  if (!response.ok) {
    const errorData = await response.json().catch(() => ({}))
    throw new Error(
      errorData.message ||
        errorData.error ||
        `Install failed: ${response.status}`,
    )
  }

  const result = await response.json()
  if (result.success === false) {
    throw new Error(result.error || 'Install failed')
  }
  return result.data || result
}

/**
 * 从远程应用商店安装 Tapp 的请求参数
 *
 * `source` is the **catalog URL or local store-source id**, NOT the install mode
 * string `"store"`. Callers must never pass `"store"` / `"direct"` here.
 */
export interface InstallFromStoreRequest {
  /** 商店源 URL（跨实例优先）或本机 store source ID */
  source: string
  /** Tapp ID */
  tappId: string
  /** 授权的权限列表（可选，默认全部授权） */
  permissions?: string[]
}

/** Official Myriad catalog URL (portable across instances; never use local DB id). */
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

/**
 * Normalize catalog URL for matching (strip trailing slash / index.json).
 */
export function normalizeStoreCatalogUrl(url: string): string {
  return url
    .trim()
    .replace(/\/+$/, '')
    .replace(/\/index\.json$/i, '')
}

/**
 * Resolve which remote catalog contains `tappId`.
 * Always returns a **URL** (portable across instances), never a local DB id.
 */
export async function resolveStoreSourceForTapp(tappId: string): Promise<{
  storeSource: string
  sourceName?: string
  matchedApp: boolean
}> {
  const { default: RemoteStoreService, OFFICIAL_STORE } = await import(
    './RemoteStoreService',
  )
  const sources = await RemoteStoreService.getEnabledSources()
  const ordered = [...sources].sort((a, b) => {
    if (a.official && !b.official) return -1
    if (!a.official && b.official) return 1
    return 0
  })

  for (const source of ordered) {
    try {
      const index = await RemoteStoreService.fetchStoreIndex(source)
      if (index.apps?.some((app) => app.id === tappId)) {
        // Prefer full index.json URL for peer install + backend lookup.
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

  // Fallback: official catalog URL (peer can resolve if they have official source).
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
  /** Progress for large packages (download + register). */
  onProgress?: import('../utils/tappInstallProgress').TappInstallProgressCallback
  /** Catalog `size` in bytes; ≥1 MiB uses client download for measurable progress. */
  estimatedBytes?: number
}

/**
 * 从远程应用商店安装 Tapp
 *
 * 优先走后端 `/api/tapps/install`（source=store，由服务端下载）。
 * 生产环境常见问题：backend 容器无法访问 raw.githubusercontent.com 等外网，
 * 会返回 502；此时回退为浏览器下载资源 + direct 安装（与商店列表同源）。
 *
 * Packages with estimatedBytes ≥ 1 MiB always use client download so the UI can
 * show real progress while fetching text + binary assets.
 *
 * @param request 安装请求 — `source` must be catalog URL/id, never `"store"`
 * @returns 安装后的 Tapp 信息
 */
export async function installFromStore(
  request: InstallFromStoreRequest,
  options?: InstallFromStoreOptions,
): Promise<TappListItem> {
  if (isInstallModePlaceholder(request.source)) {
    throw new Error(
      'Invalid storeSource: expected catalog URL or store source id, not install mode "store"',
    )
  }

  const { isLargeTappInstall, clampInstallPercent } = await import(
    '../utils/tappInstallProgress',
  )
  const report = options?.onProgress
  const large = isLargeTappInstall(options?.estimatedBytes)

  // Large packages: client path for download progress (assets dominate size).
  if (large) {
    report?.({
      phase: 'prepare',
      message: 'prepare',
      percent: 0,
    })
    return installFromStoreViaClient(request, options)
  }

  try {
    report?.({
      phase: 'install',
      message: 'server',
      percent: 30,
    })
    const result = await apiRequest('/api/tapps/install', {
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
    return result
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    // Also fall back when peer has no matching DB row for the shared catalog URL —
    // browser can still download if the URL is public.
    const shouldFallback =
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

/**
 * Install from a peer-shared / playground direct package (source=direct).
 * Used by chat share install and custom/example tapps not in the store.
 */
export async function installDirect(
  packagePayload: DirectInstallPackage,
): Promise<TappListItem> {
  if (!packagePayload?.manifest || typeof packagePayload.code !== 'string') {
    throw new Error('Direct install requires manifest and code')
  }
  const requestBody: InstallTappRequest = {
    source: 'direct',
    manifest: packagePayload.manifest,
    code: packagePayload.code,
    permissions:
      packagePayload.permissions ?? packagePayload.manifest.permissions,
  }
  if (packagePayload.styles !== undefined) requestBody.styles = packagePayload.styles
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
  if (packagePayload.pageModules) {
    requestBody.pageModules = packagePayload.pageModules
  }
  if (packagePayload.assets) requestBody.assets = packagePayload.assets

  return apiRequest('/api/tapps/install', {
    method: 'POST',
    body: JSON.stringify(requestBody),
  })
}

/**
 * Build a direct-install package from an installed Tapp (for peer share).
 * Omits heavy assets when the package JSON would exceed `maxBytes` (if set).
 */
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
    code: resources.code || '',
    permissions: detail.granted_permissions?.length
      ? detail.granted_permissions
      : detail.manifest.permissions,
  }
  if (resources.styles) pkg.styles = resources.styles
  if (resources.pageTemplate) pkg.pageTemplate = resources.pageTemplate
  if (resources.widgetTemplates) pkg.widgetTemplates = resources.widgetTemplates
  // Prefer generated widget/page CSS when present (install API field names).
  if (resources.widgetCSS) pkg.widgetCss = resources.widgetCSS
  else if (resources.widgetStyles) pkg.widgetCss = resources.widgetStyles
  if (resources.pageCSS) pkg.pageCss = resources.pageCSS
  else if (resources.pageStyles) pkg.pageCss = resources.pageStyles
  if (resources.i18n) pkg.i18n = resources.i18n
  if (resources.pageModules) pkg.pageModules = resources.pageModules

  if (!pkg.code) {
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
    // Drop optional heavy fields and retry once.
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

/**
 * 浏览器侧下载远程商店资源后，以 direct 模式安装
 *
 * Accepts catalog URL even when the peer has not added that source to their DB
 * (share-install path). Never treats `"store"` as a source id.
 */
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

  // Shared catalog URL not in local sources — still fetch by absolute URL.
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

  const index = await RemoteStoreService.fetchStoreIndex(source)
  const baseUrl =
    index.base_url ||
    source.url.replace(/\/index\.json$/, '').replace(/\/$/, '')
  const storeIndex = { ...index, base_url: baseUrl }

  const app = storeIndex.apps.find((a) => a.id === request.tappId)
  if (!app) {
    throw new Error(`商店中未找到应用: ${request.tappId}`)
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

  const requestBody: InstallTappRequest = {
    source: 'direct',
    manifest: pkg.manifest,
    code: pkg.code,
    permissions: request.permissions ?? pkg.manifest.permissions,
  }

  if (pkg.styles) requestBody.styles = pkg.styles
  if (pkg.pageTemplate) requestBody.pageTemplate = pkg.pageTemplate
  if (pkg.widgetTemplates) requestBody.widgetTemplates = pkg.widgetTemplates
  if (pkg.widgetCss) requestBody.widgetCss = pkg.widgetCss
  // Always send pageCss when present — required for cssMode=separated (pageStyles → page.css)
  if (pkg.pageCss != null && pkg.pageCss !== '') {
    requestBody.pageCss = pkg.pageCss
  } else if (pkg.manifest.pageStyles) {
    throw new Error(
      `Client install package is missing pageCss for manifest.pageStyles=${pkg.manifest.pageStyles}`,
    )
  }
  if (pkg.i18n) requestBody.i18n = pkg.i18n
  if (pkg.pageModules) requestBody.pageModules = pkg.pageModules
  // Binary package assets (manifest.assets) so Tapp.assets works after store install
  if (pkg.assets && Object.keys(pkg.assets).length > 0) {
    requestBody.assets = pkg.assets
  }

  const result = await apiRequest('/api/tapps/install', {
    method: 'POST',
    body: JSON.stringify(requestBody),
  })

  report?.({
    phase: 'done',
    message: 'done',
    percent: 100,
  })
  return result
}

/**
 * 卸载选项
 */
export interface UninstallOptions {
  /** 是否保留应用数据（存储和设置），以便再次安装时恢复 */
  keepData?: boolean
}

/**
 * 卸载 Tapp
 * @param tappId Tapp ID
 * @param options 卸载选项
 */
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

/**
 * 更新 Tapp 的请求参数（从远程商店更新）
 */
export interface UpdateTappFromStoreRequest {
  /** 商店源 URL 或 ID */
  source: string
  /** 授权的权限列表（可选，保留原有权限） */
  permissions?: string[]
}

/**
 * 更新 Tapp（从远程商店获取最新版本）
 *
 * 保留用户数据，仅更新代码和资源
 *
 * @param tappId - 要更新的 Tapp ID
 * @param request - 更新请求参数
 * @returns 更新后的 Tapp 信息
 */
export async function updateTappFromStore(
  tappId: string,
  request: UpdateTappFromStoreRequest,
): Promise<TappListItem> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/update`, {
    method: 'POST',
    body: JSON.stringify({
      source: 'store',
      storeSource: request.source,
      permissions: request.permissions,
    }),
  })
}

/**
 * 清理用户的临时 Tapp（登出时调用）
 * @returns 删除的临时 Tapp 数量
 */
export async function cleanupTemporaryTapps(): Promise<number> {
  const result = await apiRequest<number>('/api/tapps/cleanup-temporary', {
    method: 'POST',
  })
  return result
}
