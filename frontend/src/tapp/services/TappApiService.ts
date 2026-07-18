/**
 * Tapp API 服务
 * 提供 Tapp 与后端 API 的通信功能
 */

import type { TimelineResponse } from '../../types/federation'
import type {
  AITaskEvent,
  AITaskRequest,
  AITaskSnapshot,
  AIUsageSnapshot,
  NewPlatformItem,
  PermissionLevel,
  PlatformInfo,
  PlatformItemResult,
  RegisteredWidget,
  TappCodeStructure,
  TappManifest,
  WidgetRegistration,
} from '../types'
import { API_URL } from '../../config'
import { getCSRFToken } from '../../utils/csrf'
import { generateOnDemandTailwindCSS } from '../runtime/sandbox/styles'
import {
  buildPlaygroundPackageFiles,
  packageFilesToDirectInstallBody,
} from '../utils/playgroundPackageFiles'
import {
  dataTransform,
  executeTappApi,
  getContextApp,
  getContextGeo,
  getContextNavigation,
  getContextPlayer,
  getContextSystem,
  getContextUser,
  listTappApis,
} from './TappContextApi'
import {
  createTappReport,
  deleteTappReport,
  getTappReport,
  listTappReports,
  mediaControl,
  mediaStatus,
  updateTappReport,
} from './TappHostIntegrationApi'
import { apiRequest, streamRuntimeEvents } from './TappHttpClient'

export * from './TappContextApi'
export * from './TappHostIntegrationApi'
export * from './TappInteractionApi'

/** Tapp 列表项 */
export interface TappListItem {
  id: string
  name: string
  version: string
  description?: string
  icon?: string
  /** 内联 SVG 图标代码（优先于 icon） */
  iconSvg?: string
  status: string
  installedAt: string
  lastRunAt?: string
  /** 是否为临时安装（普通用户安装的 Tapp） */
  isTemporary?: boolean
  /** 是否为管理员的 Tapp */
  isAdminTapp?: boolean
}

/** Tapp 详情 */
export interface TappDetail {
  id: string
  name: string
  version: string
  description?: string
  author?: {
    name: string
    email?: string
    url?: string
  }
  icon?: string
  theme_color?: string
  manifest: TappManifest
  status: string
  granted_permissions: string[]
  installed_at: string
  last_run_at?: string
  /** 当前用户角色: "guest" | "user" | "admin" */
  user_role?: string
  /** 是否为临时安装 */
  is_temporary?: boolean
  /** 是否为管理员的 Tapp */
  is_admin_tapp?: boolean
}

export type RuntimeGrantKind = 'page' | 'widget' | 'headless'

export interface TappRuntimeGrantResponse {
  version: 2
  token: string
  runtimeId: string
  tappId: string
  ownerId: number
  subjectId: number
  instanceId: string
  kind: RuntimeGrantKind
  permissions: string[]
  expiresAt: string
}

export async function issueTappRuntimeGrant(
  tappId: string,
  instanceId: string,
  kind: RuntimeGrantKind,
): Promise<TappRuntimeGrantResponse> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/runtime-grants`, {
    method: 'POST',
    body: JSON.stringify({ instanceId, kind }),
  })
}

export async function revokeTappRuntimeGrant(
  tappId: string,
  runtimeId: string,
): Promise<void> {
  await apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/runtime-grants/${encodeURIComponent(runtimeId)}`,
    { method: 'DELETE' },
  )
}

export async function authorizeTappRuntimePermission(
  tappId: string,
  permission: string,
  runtimeGrant: string,
): Promise<void> {
  await apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/runtime-grants/authorize`,
    {
      method: 'POST',
      body: JSON.stringify({ permission }),
      runtimeGrant,
    },
  )
}

export interface FederationFeedResponse extends TimelineResponse {
  audience: 'public' | 'public+personal'
}

/** Role-aware federation feed. The runtime grant determines visible content. */
export async function getFederationFeed(
  runtimeGrant: string,
): Promise<FederationFeedResponse> {
  return apiRequest('/api/tapp/federation/feed', { runtimeGrant })
}

export interface PrepareDataExchangeRequest {
  targetTappId: string
  exportId: string
  params?: unknown
  purpose: string
}

export interface PreparedDataExchange {
  requestId: string
  requesterTappId: string
  requesterName: string
  providerTappId: string
  providerOwnerId: number
  providerName: string
  exportId: string
  exportDescription?: string
  params: unknown
  purpose: string
  maxBytes: number
  maxRecords?: number
  expiresAt: string
}

export interface OneShotDataAccessGrant {
  version: 1
  grantId: string
  token: string
  requestId: string
  providerTappId: string
  providerOwnerId: number
  exportId: string
  params: unknown
  purpose: string
  requestHash: string
  maxBytes: number
  maxRecords?: number
  expiresAt: string
}

export async function prepareDataExchange(
  request: PrepareDataExchangeRequest,
  runtimeGrant: string,
): Promise<PreparedDataExchange> {
  return apiRequest('/api/tapp/data-exchange/requests', {
    method: 'POST',
    body: JSON.stringify(request),
    runtimeGrant,
  })
}

export async function authorizeDataExchange(
  requestId: string,
  runtimeGrant: string,
): Promise<OneShotDataAccessGrant> {
  return apiRequest(
    `/api/tapp/data-exchange/requests/${encodeURIComponent(requestId)}/authorize`,
    { method: 'POST', runtimeGrant },
  )
}

export async function cancelDataExchange(
  requestId: string,
  runtimeGrant: string,
): Promise<void> {
  await apiRequest(
    `/api/tapp/data-exchange/requests/${encodeURIComponent(requestId)}`,
    { method: 'DELETE', runtimeGrant },
  )
}

export async function consumeDataExchange(
  grantToken: string,
  response: unknown,
  providerRuntimeGrant: string,
): Promise<unknown> {
  return apiRequest('/api/tapp/data-exchange/consume', {
    method: 'POST',
    body: JSON.stringify({ grantToken, response }),
    runtimeGrant: providerRuntimeGrant,
  })
}

/** 获取当前用户在系统配置下可使用的 Tapp 权限等级。 */
export async function getAllowedPermissionLevels(): Promise<PermissionLevel[]> {
  const response = await apiRequest<{ allowed_levels: PermissionLevel[] }>(
    '/api/config/permissions',
  )
  return response.allowed_levels
}

// ============ Tapp 应用管理 API ============

/**
 * 获取已安装的 Tapp 列表
 */
export async function listTapps(): Promise<TappListItem[]> {
  return apiRequest('/api/tapps')
}

/** 一次获取当前会话可见的全部 Tapp 详情，避免列表同步产生 N+1 请求。 */
export async function listTappDetails(): Promise<TappDetail[]> {
  return apiRequest('/api/tapps/details')
}

/** 最近使用的 Tapp 项 */
export interface RecentTappItem {
  id: string
  name: string
  icon?: string
  iconSvg?: string
  themeColor?: string
  lastRunAt: string
  runCount: number
}

/**
 * 获取当前用户最近使用的 Tapp 列表
 *
 * @param limit 返回的最大数量，默认 10
 * @returns 最近使用的 Tapp 列表（按最后运行时间降序）
 */
export async function getRecentTapps(
  limit: number = 10,
): Promise<RecentTappItem[]> {
  return apiRequest(`/api/tapps/recent?limit=${limit}`)
}

/**
 * 安装 Tapp 请求体（统一格式）
 */
interface InstallTappRequest {
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
  compiledCss?: SeparatedCSSRequest,
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
  compiledCss?: SeparatedCSSRequest,
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
 */
export interface InstallFromStoreRequest {
  /** 商店源 URL 或 ID */
  source: string
  /** Tapp ID */
  tappId: string
  /** 授权的权限列表（可选，默认全部授权） */
  permissions?: string[]
}

/**
 * 从远程应用商店安装 Tapp
 *
 * 优先走后端 `/api/tapps/install`（source=store，由服务端下载）。
 * 生产环境常见问题：backend 容器无法访问 raw.githubusercontent.com 等外网，
 * 会返回 502；此时回退为浏览器下载资源 + direct 安装（与商店列表同源）。
 *
 * @param request 安装请求
 * @returns 安装后的 Tapp 信息
 */
export async function installFromStore(
  request: InstallFromStoreRequest,
): Promise<TappListItem> {
  try {
    return await apiRequest('/api/tapps/install', {
      method: 'POST',
      body: JSON.stringify({
        source: 'store',
        storeSource: request.source,
        tappId: request.tappId,
        permissions: request.permissions,
      }),
    })
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    const shouldFallback =
      /502|BAD_GATEWAY|Failed to fetch store|cannot reach store|Failed to fetch manifest|Failed to fetch code|Failed to fetch|NetworkError|ECONNREFUSED|timeout|Load failed/i.test(
        message,
      )

    if (!shouldFallback) {
      throw error
    }

    console.warn(
      '[Tapp] Backend store install failed, falling back to client-side download:',
      message,
    )
    return installFromStoreViaClient(request)
  }
}

/**
 * 浏览器侧下载远程商店资源后，以 direct 模式安装
 */
async function installFromStoreViaClient(
  request: InstallFromStoreRequest,
): Promise<TappListItem> {
  const { default: RemoteStoreService } = await import('./RemoteStoreService')

  const sources = await RemoteStoreService.getSources()
  const source =
    sources.find(
      (s) =>
        String(s.id) === request.source ||
        s.url === request.source ||
        s.url.replace(/\/index\.json$/, '') ===
          request.source.replace(/\/index\.json$/, ''),
    ) || sources.find((s) => s.enabled)

  if (!source) {
    throw new Error('无法找到商店源，请检查商店配置')
  }

  const index = await RemoteStoreService.fetchStoreIndex(source)
  const baseUrl =
    index.base_url ||
    source.url.replace(/\/index\.json$/, '').replace(/\/$/, '')
  const storeIndex = { ...index, base_url: baseUrl }

  const app = storeIndex.apps.find((a) => a.id === request.tappId)
  if (!app) {
    throw new Error(`商店中未找到应用: ${request.tappId}`)
  }

  const pkg = await RemoteStoreService.downloadAppPackage(app, storeIndex)

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
  if (pkg.pageCss) requestBody.pageCss = pkg.pageCss
  if (pkg.i18n) requestBody.i18n = pkg.i18n
  if (pkg.pageModules) requestBody.pageModules = pkg.pageModules

  return apiRequest('/api/tapps/install', {
    method: 'POST',
    body: JSON.stringify(requestBody),
  })
}

/**
 * 获取 Tapp 详情
 */
export async function getTapp(tappId: string): Promise<TappDetail> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}`)
}

/**
 * 获取 Tapp 代码
 */
export async function getTappCode(tappId: string): Promise<string> {
  // GET 请求不需要 CSRF Token
  const response = await fetch(
    `${API_URL}/api/tapps/${encodeURIComponent(tappId)}/code`,
    {
      method: 'GET',
      credentials: 'include',
    },
  )
  if (!response.ok) {
    throw new Error(`Failed to get Tapp code: ${response.status}`)
  }
  return response.text()
}

/**
 * Tapp 完整资源响应（后端 snake_case，这里转换为 camelCase）
 */
export interface TappResources {
  /** 主代码（index.js） */
  code: string
  /** 自定义 CSS 样式（统一模式，或共享样式） */
  styles?: string
  /** Widget 专用自定义 CSS（分离模式） */
  widgetStyles?: string
  /** Page 专用自定义 CSS（分离模式） */
  pageStyles?: string
  /** Widget 专用编译后的 Tailwind CSS */
  widgetCSS?: string
  /** Page 专用编译后的 Tailwind CSS */
  pageCSS?: string
  /** Widget HTML 模板（Widget ID → 尺寸） */
  widgetTemplates?: Record<string, Record<string, string>>
  /** Page HTML 模板 */
  pageTemplate?: string
  /** CSS 架构模式 */
  cssMode?: 'unified' | 'separated'
  /** i18n 翻译数据（语言代码 → 键值对） */
  i18n?: Record<string, unknown>
  /** Page 模块文件（文件名 → 代码内容） */
  pageModules?: Record<string, string>
  /** Page 模块加载顺序 */
  pageModuleOrder?: string[]
}

/** 后端原始响应格式（snake_case） */
interface TappResourcesRaw {
  code: string
  styles?: string
  widget_styles?: string
  page_styles?: string
  widget_css?: string
  page_css?: string
  widget_templates?: Record<string, Record<string, string>>
  page_template?: string
  css_mode?: 'unified' | 'separated'
  i18n?: Record<string, unknown>
  page_modules?: Record<string, string>
  page_module_order?: string[]
}

/**
 * 获取 Tapp 完整资源（代码 + CSS + HTML 模板）
 * 支持混合渲染模式
 */
export async function getTappResources(tappId: string): Promise<TappResources> {
  const response = await fetch(
    `${API_URL}/api/tapps/${encodeURIComponent(tappId)}/resources`,
    {
      method: 'GET',
      credentials: 'include',
    },
  )
  if (!response.ok) {
    // 如果新 API 不存在，回退到只获取代码
    if (response.status === 404) {
      const code = await getTappCode(tappId)
      return { code }
    }
    throw new Error(`Failed to get Tapp resources: ${response.status}`)
  }

  // 转换 snake_case 到 camelCase
  const raw: TappResourcesRaw = await response.json()

  return {
    code: raw.code,
    styles: raw.styles,
    widgetStyles: raw.widget_styles,
    pageStyles: raw.page_styles,
    widgetCSS: raw.widget_css,
    pageCSS: raw.page_css,
    widgetTemplates: raw.widget_templates,
    pageTemplate: raw.page_template,
    cssMode: raw.css_mode,
    i18n: raw.i18n,
    pageModules: raw.page_modules,
    pageModuleOrder: raw.page_module_order,
  }
}

export interface TappAssetPayload {
  path: string
  mimeType: string
  size: number
  base64: string
}

/**
 * 读取 Manifest 声明的包内静态资源（base64）。
 * 沙箱内再转为 blob URL；宿主不跨 origin 共享 blob。
 */
export async function getTappAsset(
  tappId: string,
  path: string,
): Promise<TappAssetPayload> {
  const params = new URLSearchParams({ path })
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/asset?${params.toString()}`,
  )
}

/**
 * 分离式 CSS 更新请求
 */
export interface SeparatedCSSRequest {
  /** Widget 专用 CSS */
  widgetCss?: string
  /** Page 专用 CSS */
  pageCss?: string
}

/**
 * 更新 Tapp 的分离式 CSS
 * 分别更新 widget.css 和 page.css
 */
export async function updateSeparatedCSS(
  tappId: string,
  css: SeparatedCSSRequest,
): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/separated-css`, {
    method: 'POST',
    body: JSON.stringify(css),
  })
}

/**
 * 启动 Tapp
 */
export async function startTapp(tappId: string): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/start`, {
    method: 'POST',
  })
}

/**
 * 停止 Tapp
 */
export async function stopTapp(tappId: string): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/stop`, {
    method: 'POST',
  })
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
 * 导出 Tapp 为 .tapp 文件
 * @param tappId Tapp ID
 * @returns 下载 URL 或触发浏览器下载
 */
export async function exportTapp(tappId: string): Promise<void> {
  const url = `${API_URL}/api/tapps/${encodeURIComponent(tappId)}/export`

  // 使用 fetch 获取文件，然后触发下载
  const response = await fetch(url, {
    credentials: 'include',
  })

  if (!response.ok) {
    throw new Error(`Export failed: ${response.status}`)
  }

  // 获取文件名
  const disposition = response.headers.get('Content-Disposition')
  let filename = `${tappId}.tapp`
  if (disposition) {
    const match = disposition.match(/filename="(.+)"/)
    if (match) {
      filename = match[1]
    }
  }

  // 转换为 Blob 并下载
  const blob = await response.blob()
  const downloadUrl = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = downloadUrl
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(downloadUrl)
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

/**
 * 获取所有已注册的小组件
 * 通过 /api/tapps/widgets 一次性获取所有小组件
 */
export async function getAllWidgets(): Promise<RegisteredWidget[]> {
  return apiRequest<RegisteredWidget[]>('/api/tapps/widgets')
}

/**
 * 注册小组件到后端
 */
export async function registerTappWidget(
  tappId: string,
  config: WidgetRegistration,
  runtimeGrant: string,
): Promise<RegisteredWidget> {
  const requestBody = {
    id: config.id,
    name: config.name,
    description: config.description,
    icon: config.icon,
    default_size: config.defaultSize,
    sizes: config.sizes,
    category: config.category,
    settings: config.settings || [],
    refresh_policy: config.refreshPolicy,
  }

  const result = await apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/widgets`,
    {
      method: 'POST',
      body: JSON.stringify(requestBody),
      runtimeGrant,
    },
  )
  return result as RegisteredWidget
}

/**
 * 注销小组件
 */
export async function unregisterTappWidget(
  tappId: string,
  widgetId: string,
  runtimeGrant: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/widgets/${encodeURIComponent(widgetId)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

// ============ Tapp 存储 API ============

/** Host settings editor. Only manifest-declared keys are accepted by backend. */
export async function getTappSettings(
  tappId: string,
): Promise<Record<string, unknown>> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/settings`)
}

export async function getTappSetting(
  tappId: string,
  key: string,
): Promise<unknown> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/settings/${encodeURIComponent(key)}`,
  )
}

export async function setTappSetting(
  tappId: string,
  key: string,
  value: unknown,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/settings/${encodeURIComponent(key)}`,
    { method: 'POST', body: JSON.stringify(value) },
  )
}

/**
 * 获取存储值
 */
export async function getStorage(
  tappId: string,
  key: string,
  runtimeGrant?: string,
): Promise<unknown> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`,
    { runtimeGrant },
  )
}

/**
 * 设置存储值
 */
export async function setStorage(
  tappId: string,
  key: string,
  value: unknown,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`,
    {
      method: 'POST',
      body: JSON.stringify(value),
      runtimeGrant,
    },
  )
}

/**
 * 删除存储值
 */
export async function removeStorage(
  tappId: string,
  key: string,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

/**
 * 获取所有存储键
 */
export async function listStorageKeys(
  tappId: string,
  runtimeGrant?: string,
): Promise<string[]> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage`, {
    runtimeGrant,
  })
}

/**
 * 一次获取全部存储项。
 */
export async function listStorageEntries(
  tappId: string,
  runtimeGrant?: string,
): Promise<Record<string, unknown>> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/storage/entries`,
    { runtimeGrant },
  )
}

/**
 * 清除所有存储
 */
export async function clearStorage(
  tappId: string,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage`, {
    method: 'DELETE',
    runtimeGrant,
  })
}

export async function getStorageUsage(
  tappId: string,
  runtimeGrant?: string,
): Promise<{ used: number; quota: number }> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage/usage`, {
    runtimeGrant,
  })
}

// ============ 平台数据 API ============

/**
 * 获取已启用的平台列表
 */
export async function listEnabledPlatforms(
  runtimeGrant?: string,
): Promise<PlatformInfo[]> {
  const data = await apiRequest<{ platforms: PlatformInfo[] }>(
    '/api/platforms',
    {
      runtimeGrant,
    },
  )
  return data.platforms.filter((p) => p.enabled)
}

/**
 * 获取平台数据
 */
export async function getPlatformData(
  platform: string,
  options?: {
    limit?: number
    offset?: number
  },
  runtimeGrant?: string,
): Promise<{
  items: unknown[]
  total: number
  platform: string
}> {
  const params = new URLSearchParams()
  if (options?.limit !== undefined) params.set('limit', String(options.limit))
  if (options?.offset !== undefined)
    params.set('offset', String(options.offset))

  const query = params.size > 0 ? `?${params}` : ''
  return apiRequest(
    `/api/tapp/platform/${encodeURIComponent(platform)}/data${query}`,
    { runtimeGrant },
  )
}

/**
 * 获取平台统计数据
 */
export async function getPlatformStats(
  platform: string,
  runtimeGrant?: string,
): Promise<{
  platform: string
  total: number
  distribution: Record<string, number>
  recentActivity: { date: string; count: number }[]
}> {
  return apiRequest(`/api/tapp/platform/${platform}/stats`, { runtimeGrant })
}

/**
 * 获取平台数据分布
 */
export async function getPlatformDistribution(
  platform: string,
  dimension: string,
  runtimeGrant?: string,
): Promise<{ dimension: string; data: { label: string; value: number }[] }> {
  return apiRequest(
    `/api/tapp/platform/${platform}/distribution/${dimension}`,
    {
      runtimeGrant,
    },
  )
}

/**
 * 添加新的平台数据条目
 */
export async function addPlatformItem(
  tappId: string,
  item: NewPlatformItem,
  runtimeGrant?: string,
): Promise<PlatformItemResult> {
  return apiRequest('/api/tapp/platform/items', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      item,
    }),
    runtimeGrant,
  })
}

/**
 * 批量添加平台数据条目
 */
export async function addPlatformItems(
  tappId: string,
  items: NewPlatformItem[],
  runtimeGrant?: string,
): Promise<{ success: boolean; results: PlatformItemResult[] }> {
  return apiRequest('/api/tapp/platform/items/batch', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      items,
    }),
    runtimeGrant,
  })
}

/** 创建由服务端治理的 AI 任务。 */
export async function createAITask(
  request: AITaskRequest,
  runtimeGrant: string,
): Promise<AITaskSnapshot> {
  return apiRequest('/api/tapp/ai/v2/tasks', {
    method: 'POST',
    body: JSON.stringify(request),
    runtimeGrant,
  })
}

/** 读取当前 Tapp/subject 范围内的 AI 任务。 */
export async function getAITask(
  taskId: string,
  runtimeGrant: string,
): Promise<AITaskSnapshot> {
  return apiRequest(`/api/tapp/ai/v2/tasks/${encodeURIComponent(taskId)}`, {
    runtimeGrant,
  })
}

/** 请求取消仍在运行的 AI 任务。 */
export async function cancelAITask(
  taskId: string,
  runtimeGrant: string,
): Promise<{ success: boolean; taskId: string }> {
  return apiRequest(`/api/tapp/ai/v2/tasks/${encodeURIComponent(taskId)}`, {
    method: 'DELETE',
    runtimeGrant,
  })
}

export async function getAIUsage(
  runtimeGrant: string,
): Promise<AIUsageSnapshot> {
  const response = await apiRequest<{ usage: AIUsageSnapshot }>(
    '/api/tapp/ai/v2/usage',
    { runtimeGrant },
  )
  return response.usage
}

/**
 * Host-only SSE reader. The Runtime Grant remains in the host and parsed
 * events are forwarded through TappBridge; sandbox code never receives it.
 */
export async function streamAITaskEvents(
  taskId: string,
  runtimeGrant: string,
  onEvent: (event: AITaskEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  return streamRuntimeEvents(
    `/api/tapp/ai/v2/tasks/${encodeURIComponent(taskId)}/events`,
    runtimeGrant,
    (event, data) => onEvent({ event: event as AITaskEvent['event'], data }),
    signal,
  )
}

// ============ 报告 API ============

/**
 * 获取报告列表
 */
export async function listReports(runtimeGrant?: string): Promise<{
  reports: {
    id: string
    platform: string
    type: 'platform' | 'comprehensive'
    createdAt: string
    summary?: string
  }[]
}> {
  return apiRequest('/api/tapp/report-catalog', { runtimeGrant })
}

/**
 * 获取单个报告
 */
export async function getReport(
  reportId: string,
  runtimeGrant?: string,
): Promise<{
  id: string
  platform?: string
  type: 'platform' | 'comprehensive'
  content: unknown
  createdAt: string
}> {
  return apiRequest(
    `/api/tapp/report-catalog/${encodeURIComponent(reportId)}`,
    {
      runtimeGrant,
    },
  )
}

/**
 * 获取平台报告
 */
export async function getPlatformReport(
  platform: string,
  runtimeGrant?: string,
): Promise<{
  platform: string
  summary: string
  insights: string[]
  metadata: unknown
  cardVisuals: unknown
  createdAt: string
} | null> {
  try {
    return await apiRequest(
      `/api/tapp/report-catalog/platform/${encodeURIComponent(platform)}`,
      { runtimeGrant },
    )
  } catch {
    return null
  }
}

export default {
  // Tapp 应用管理
  listTapps,
  getRecentTapps,
  installTapp,
  installTappFile,
  installFromCode,
  installFromStore,
  updateTappFromCode,
  updateTappFromStore,
  getTapp,
  getTappCode,
  getTappResources,
  startTapp,
  stopTapp,
  uninstallTapp,
  exportTapp,
  // Widget 管理
  getAllWidgets,
  registerTappWidget,
  unregisterTappWidget,
  // 存储
  getStorage,
  setStorage,
  removeStorage,
  listStorageKeys,
  listStorageEntries,
  clearStorage,
  // Platform
  listEnabledPlatforms,
  getPlatformData,
  getPlatformStats,
  getPlatformDistribution,
  addPlatformItem,
  addPlatformItems,
  // AI Task
  createAITask,
  getAITask,
  cancelAITask,
  getAIUsage,
  streamAITaskEvents,
  // Reports
  listReports,
  getReport,
  getPlatformReport,
  // P0: Data Transform
  dataTransform,
  // P0: Context API
  getContextApp,
  getContextUser,
  getContextPlayer,
  getContextNavigation,
  getContextSystem,
  getContextGeo,
  // Tapp API 声明系统
  executeTappApi,
  listTappApis,
  // P1: Report CRUD
  createTappReport,
  listTappReports,
  getTappReport,
  updateTappReport,
  deleteTappReport,
  // P1: Media Control
  mediaControl,
  mediaStatus,
  // Package assets
  getTappAsset,
}
