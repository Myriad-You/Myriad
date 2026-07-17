/**
 * Tapp API 服务
 * 提供 Tapp 与后端 API 的通信功能
 */

import type { TimelineResponse } from '../../types/federation'
import type {
  AgentInteractionV2,
  AITaskEvent,
  AITaskRequest,
  AITaskSnapshot,
  AIUsageSnapshot,
  NewPlatformItem,
  PermissionLevel,
  PlatformInfo,
  PlatformItemResult,
  PublishEventRequest,
  RegisteredWidget,
  TappCodeStructure,
  TappEvent,
  TappManifest,
  WidgetRegistration,
} from '../types'
import { API_URL } from '../../config'
import { getCSRFToken } from '../../utils/csrf'
import { generateOnDemandTailwindCSS } from '../runtime/sandbox/styles'

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

/**
 * 通用 API 请求函数
 *
 * 对于 GET 请求，不需要 CSRF token（只读操作）
 * 对于 POST/PUT/DELETE 等修改请求，需要 CSRF token
 */
interface ApiRequestOptions extends RequestInit {
  /** Host-only runtime identity; never exposed to sandbox code. */
  runtimeGrant?: string
}

async function apiRequest<T>(
  endpoint: string,
  options: ApiRequestOptions = {},
  retryOnCsrf: boolean = true,
  retryOnRuntimeGrant: boolean = true,
): Promise<T> {
  const { runtimeGrant, ...fetchOptions } = options
  // 只有非 GET 请求才需要 CSRF token
  const method = (options.method || 'GET').toUpperCase()
  const needsCsrf =
    method !== 'GET' && method !== 'HEAD' && method !== 'OPTIONS'
  const csrfToken = needsCsrf ? (await getCSRFToken()) || '' : ''

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }

  if (runtimeGrant) {
    headers['X-Tapp-Runtime-Grant'] = runtimeGrant
  }

  // 只在需要时添加 CSRF token
  if (needsCsrf && csrfToken) {
    headers['X-CSRF-Token'] = csrfToken
  }

  const response = await fetch(`${API_URL}${endpoint}`, {
    ...fetchOptions,
    headers,
    credentials: 'include',
  })

  if (!response.ok) {
    const errorData = await response.json().catch(() => ({}))
    if (
      response.status === 403 &&
      retryOnCsrf &&
      (errorData.error?.includes('CSRF') || errorData.error?.includes('csrf'))
    ) {
      // 强制刷新 CSRF Token
      await getCSRFToken(true)
      // 重试请求（不再重试）
      return apiRequest(endpoint, options, false, retryOnRuntimeGrant)
    }

    if (
      response.status === 401 &&
      retryOnRuntimeGrant &&
      runtimeGrant &&
      errorData.code === 'INVALID_RUNTIME_GRANT'
    ) {
      const { TappRuntimeGrant } = await import('../runtime/TappRuntimeGrant')
      const replacement =
        await TappRuntimeGrant.recoverRejectedToken(runtimeGrant)
      if (replacement) {
        return apiRequest(
          endpoint,
          { ...options, runtimeGrant: replacement },
          retryOnCsrf,
          false,
        )
      }
    }

    const detail =
      errorData.message || errorData.error || response.statusText || 'unknown'
    throw new Error(`API Error: ${response.status} ${detail}`)
  }

  const result = await response.json()

  // 处理 API 响应格式
  // 格式1: { success: true, data: {...} } - 标准包装格式
  // 格式2: { success: true, result: "...", ... } - AI API 等直接返回格式
  if (typeof result === 'object' && result !== null && 'success' in result) {
    if (!result.success) {
      throw new Error(result.error || 'Unknown error')
    }
    // 如果有 data 字段，返回 data；否则返回整个 result（去掉 success 字段外的所有内容）
    if ('data' in result) {
      return result.data as T
    }
    // 对于 AI API 等没有 data 包装的响应，直接返回整个结果
    return result as T
  }

  return result as T
}

/** Shared host-only SSE parser used by bounded runtime streams. */
async function streamRuntimeEvents(
  endpoint: string,
  runtimeGrant: string,
  onEvent: (event: string, data: unknown) => void,
  signal?: AbortSignal,
  retryOnRuntimeGrant: boolean = true,
): Promise<void> {
  const response = await fetch(`${API_URL}${endpoint}`, {
    headers: {
      Accept: 'text/event-stream',
      'X-Tapp-Runtime-Grant': runtimeGrant,
    },
    credentials: 'include',
    signal,
  })
  if (!response.ok) {
    const error = await response.json().catch(() => ({}))
    if (
      response.status === 401 &&
      retryOnRuntimeGrant &&
      error.code === 'INVALID_RUNTIME_GRANT'
    ) {
      const { TappRuntimeGrant } = await import('../runtime/TappRuntimeGrant')
      const replacement =
        await TappRuntimeGrant.recoverRejectedToken(runtimeGrant)
      if (replacement) {
        return streamRuntimeEvents(
          endpoint,
          replacement,
          onEvent,
          signal,
          false,
        )
      }
    }
    throw new Error(
      error.message ||
        error.error ||
        `Runtime event stream failed (${response.status})`,
    )
  }
  if (!response.body)
    throw new Error('Runtime event stream has no response body')

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  while (true) {
    const { done, value } = await reader.read()
    buffer += decoder.decode(value, { stream: !done }).replace(/\r\n/g, '\n')
    let boundary = buffer.indexOf('\n\n')
    while (boundary >= 0) {
      const block = buffer.slice(0, boundary)
      buffer = buffer.slice(boundary + 2)
      let eventName = 'message'
      const data: string[] = []
      for (const line of block.split('\n')) {
        if (line.startsWith('event:')) eventName = line.slice(6).trim()
        if (line.startsWith('data:')) data.push(line.slice(5).trimStart())
      }
      if (data.length > 0) {
        const raw = data.join('\n')
        let parsed: unknown = raw
        try {
          parsed = JSON.parse(raw)
        } catch {
          // Control events may intentionally contain short plain text.
        }
        onEvent(eventName, parsed)
      }
      boundary = buffer.indexOf('\n\n')
    }
    if (done) break
  }
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

function buildDirectTappRequest(
  manifest: TappManifest,
  code: TappCodeStructure,
  permissions?: string[],
  compiledCss?: SeparatedCSSRequest,
): InstallTappRequest {
  // 有 pageModules 时，main.js 只存 widget 相关代码（模块化的页面代码已在 pageModules 中）
  // 无 pageModules 时，main.js 存完整合并代码（core + widget + page 单体回退）
  const hasPageModules =
    code.pageModules && Object.keys(code.pageModules).length > 0
  let jsCode: string
  if (hasPageModules) {
    // 模块化模式：main.js 仅保留 widget 部分（如果有的话）
    jsCode = [
      code.widget ? `// ========== Widget Code ==========\n${code.widget}` : '',
    ]
      .filter(Boolean)
      .join('\n')
  } else {
    // 单体模式：合并所有代码
    jsCode = [
      code.core,
      code.widget
        ? `\n// ========== Widget Code ==========\n${code.widget}`
        : '',
      code.page ? `\n// ========== Page Code ==========\n${code.page}` : '',
    ].join('')
  }

  const requestBody: InstallTappRequest = {
    source: 'direct',
    manifest,
    code: jsCode,
    permissions,
  }

  // 添加可选资源
  if (code.styles) {
    requestBody.styles = code.styles
  }
  if (code.pageHtml) {
    requestBody.pageTemplate = code.pageHtml
  }
  if (code.widgetHtml && manifest.widgets && manifest.widgets.length > 0) {
    const templates: Record<string, Record<string, string>> = {}
    for (const widget of manifest.widgets) {
      if (widget.templates) {
        const widgetTemplates: Record<string, string> = {}
        for (const size of Object.keys(widget.templates)) {
          widgetTemplates[size] = code.widgetHtml
        }
        if (Object.keys(widgetTemplates).length > 0) {
          templates[widget.id] = widgetTemplates
        }
      }
    }
    if (Object.keys(templates).length > 0) {
      requestBody.widgetTemplates = templates
    }
  }

  // 添加 i18n 和 pageModules
  if (code.i18n && Object.keys(code.i18n).length > 0) {
    requestBody.i18n = code.i18n
  }
  if (code.pageModules && Object.keys(code.pageModules).length > 0) {
    requestBody.pageModules = code.pageModules
  }
  if (code.assets && Object.keys(code.assets).length > 0) {
    requestBody.assets = code.assets
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
 * One-time compatibility for store packages published before every HTTP API
 * required `network:fetch`. Client store fallback installs as `direct`, so the
 * backend store migration never runs on this path.
 */
function migrateLegacyStoreHttpApiPermissions(
  manifest: TappManifest,
): TappManifest {
  const apis = manifest.apis
  if (!apis) {
    return manifest
  }
  const hasHttpApi = Object.values(apis).some(
    (api) => (api.type ?? 'http') === 'http',
  )
  if (!hasHttpApi) {
    return manifest
  }
  const permissions = manifest.permissions ?? []
  if (permissions.includes('network:fetch')) {
    return manifest
  }
  if (permissions.length >= 64) {
    return manifest
  }
  return {
    ...manifest,
    permissions: [...permissions, 'network:fetch'],
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

  // Compatibility for store packages published before every HTTP API required
  // network:fetch. Client-side store fallback installs as `direct`, so it never
  // hits backend fetch_from_store migration — backfill here to keep validation
  // and approved permissions aligned with runtime NetworkFetch checks.
  const manifest = migrateLegacyStoreHttpApiPermissions(pkg.manifest)
  let permissions =
    !request.permissions || request.permissions.length === 0
      ? manifest.permissions
      : [...request.permissions]
  if (
    manifest.permissions.includes('network:fetch') &&
    !permissions.includes('network:fetch')
  ) {
    permissions = [...permissions, 'network:fetch']
  }

  const requestBody: InstallTappRequest = {
    source: 'direct',
    manifest,
    code: pkg.code,
    permissions,
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

// ============ P0: Data Transform API ============

/** 数据输入源 */
export type DataInput =
  | { source: 'platform'; platform: string }
  | { source: 'storage'; key: string }
  | { source: 'inline'; data: unknown }

/** 数据输出目标 */
export type DataOutput =
  { target: 'platform'; platform: string } | { target: 'storage'; key: string }

/** 处理步骤 */
export type ProcessStep =
  | { type: 'filter'; field: string; operator: string; value: unknown }
  | { type: 'sort'; field: string; order?: 'asc' | 'desc' }
  | { type: 'limit'; count: number }
  | { type: 'offset'; count: number }
  | { type: 'select'; fields: string[] }
  | { type: 'group'; by: string }
  | {
      type: 'aggregate'
      operation: 'count' | 'sum' | 'avg' | 'min' | 'max'
      field?: string
    }
  | { type: 'dedupe'; key: string }
  | { type: 'map'; operations: MapOperation[] }

export type MapOperation =
  | { op: 'rename'; from: string; to: string }
  | { op: 'remove'; field: string }
  | { op: 'set'; field: string; value: unknown }
  | { op: 'copy'; from: string; to: string }
  | { op: 'template'; field: string; template: string }
  | { op: 'lower' | 'upper' | 'to_string' | 'to_number'; field: string }
  | { op: 'default'; field: string; value: unknown }
  | { op: 'concat'; fields: string[]; separator?: string; to: string }
  | { op: 'coalesce'; fields: string[]; to: string }

/** 数据转换请求 */
export interface DataTransformRequest {
  tappId: string
  input: DataInput
  pipeline: ProcessStep[]
  output?: DataOutput
}

/** 数据转换响应 */
export interface DataTransformResponse {
  success: boolean
  count: number
  data: unknown[]
}

/**
 * 执行数据转换管道
 */
export async function dataTransform(
  request: DataTransformRequest,
  runtimeGrant?: string,
): Promise<DataTransformResponse> {
  return apiRequest('/api/tapp/data/transform', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      input: request.input,
      pipeline: request.pipeline,
      output: request.output,
    }),
    runtimeGrant,
  })
}

// ============ P0: Context API ============

/** 应用上下文 */
export interface AppContext {
  version: string
  locale: string
  theme: string
  features: {
    aiEnabled: boolean
    platforms: string[]
  }
}

/** 用户上下文 */
export interface UserContext {
  id: string
  username: string
  display_name?: string | null
  avatar: string | null
  avatar_url?: string | null
  /** 是否为管理员 */
  isAdmin: boolean
  /** 用户角色: "guest" | "user" | "admin" */
  role: 'guest' | 'user' | 'admin'
  connectedPlatforms: string[]
  preferences: {
    language: string
    timezone: string
  }
}

/** 播放器上下文 */
export interface PlayerContext {
  isPlaying: boolean
  isPaused: boolean
  currentTrack: {
    id: string
    title: string
    artist: string
    album?: string
    cover?: string
    duration: number
    source: string
  } | null
  progress: {
    current: number
    duration: number
    percentage: number
  }
  playlist: {
    id: string
    name: string
    tracks: number
  } | null
  mode: 'sequence' | 'loop' | 'shuffle' | 'single'
  volume: number
  muted: boolean
}

/** 导航上下文 */
export interface NavigationContext {
  currentPath: string
  previousPath: string | null
  history: string[]
  availableRoutes: {
    path: string
    name: string
    icon: string
  }[]
  tappPages: {
    id: string
    path: string
    name: string
    tappId: string
  }[]
}

/** 系统上下文 */
export interface SystemContext {
  online: boolean
  serverConnected: boolean
  version: string
  backgroundTasks: {
    id: string
    type: string
    status: 'running' | 'completed' | 'failed'
    progress?: number
  }[]
  lastFetch: Record<string, string | null>
}

/**
 * 获取应用上下文
 */
export async function getContextApp(
  runtimeGrant?: string,
): Promise<AppContext> {
  return apiRequest('/api/tapp/context/app', { runtimeGrant })
}

/**
 * 获取用户上下文
 */
export async function getContextUser(
  runtimeGrant?: string,
): Promise<UserContext> {
  return apiRequest('/api/tapp/context/user', { runtimeGrant })
}

/**
 * 获取播放器上下文
 */
export async function getContextPlayer(
  runtimeGrant?: string,
): Promise<PlayerContext> {
  return apiRequest('/api/tapp/context/player', { runtimeGrant })
}

/**
 * 获取导航上下文
 */
export async function getContextNavigation(
  runtimeGrant?: string,
): Promise<NavigationContext> {
  return apiRequest('/api/tapp/context/navigation', { runtimeGrant })
}

/**
 * 获取系统上下文
 */
export async function getContextSystem(
  runtimeGrant?: string,
): Promise<SystemContext> {
  return apiRequest('/api/tapp/context/system', { runtimeGrant })
}

// ============ 地理位置 API ============

/** 地理位置信息 */
export interface GeoContext {
  lat: number
  lon: number
  city: string
  region: string
  country: string
  /** 国家代码（如 CN, US） */
  countryCode?: string
}

/**
 * 获取客户端地理位置信息
 * 这是一个公开 API，所有用户（包括游客）都可以调用
 */
export async function getContextGeo(
  runtimeGrant?: string,
): Promise<GeoContext> {
  const result = await apiRequest<{ success: boolean; data: GeoContext }>(
    '/api/tapp/context/geo',
    { runtimeGrant },
  )
  if (result.success && result.data) {
    return result.data
  }
  throw new Error('Failed to get geo info')
}

// ============ Tapp API 声明系统 ============

/** Tapp API 执行请求 */
export interface TappApiExecuteRequest {
  tappId: string
  apiName: string
  params?: Record<string, unknown>
}

/** Tapp API 执行响应 */
export interface TappApiExecuteResponse {
  success: boolean
  data?: unknown
  error?: string
  cached?: boolean
}

/** Tapp API 定义 */
export interface TappApiInfo {
  name: string
  access: 'public' | 'protected'
  type: 'http' | 'builtin'
  description?: string
  cacheTtl?: number
}

/**
 * 执行 Tapp 声明的 API
 *
 * @param tappId - Tapp ID
 * @param apiName - API 名称（在 manifest.apis 中定义的 key）
 * @param params - 可选参数
 * @returns API 执行结果
 */
export async function executeTappApi(
  tappId: string,
  apiName: string,
  params?: Record<string, unknown>,
  runtimeGrant?: string,
): Promise<TappApiExecuteResponse> {
  const execute = async (
    grant: string | undefined,
    retryOnRuntimeGrant: boolean,
  ): Promise<TappApiExecuteResponse> => {
    const csrfToken = (await getCSRFToken()) || ''
    const response = await fetch(
      `${API_URL}/api/tapp/${encodeURIComponent(tappId)}/api/${encodeURIComponent(apiName)}`,
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
          ...(grant ? { 'X-Tapp-Runtime-Grant': grant } : {}),
        },
        body: JSON.stringify({ params }),
        credentials: 'include',
      },
    )

    const result = await response.json().catch(() => ({}))
    if (
      response.status === 401 &&
      retryOnRuntimeGrant &&
      grant &&
      result.code === 'INVALID_RUNTIME_GRANT'
    ) {
      const { TappRuntimeGrant } = await import('../runtime/TappRuntimeGrant')
      const replacement = await TappRuntimeGrant.recoverRejectedToken(grant)
      if (replacement) return execute(replacement, false)
    }

    if (!response.ok) {
      return {
        success: false,
        error:
          result.message ||
          result.error ||
          `Declared API request failed (${response.status})`,
      }
    }

    return {
      success: result.success ?? false,
      data: result.data,
      error: result.error,
      cached: result.cached,
    }
  }

  return execute(runtimeGrant, true)
}

/**
 * 列出 Tapp 可用的 API
 *
 * @param tappId - Tapp ID
 * @returns API 列表
 */
export async function listTappApis(
  tappId: string,
  runtimeGrant?: string,
): Promise<TappApiInfo[]> {
  const result = await apiRequest<{ success: boolean; apis: TappApiInfo[] }>(
    `/api/tapp/${encodeURIComponent(tappId)}/apis`,
    { runtimeGrant },
  )
  return result.apis || []
}

// ============ P1: Report CRUD API ============

/** 创建报告请求 */
export interface CreateReportRequest {
  tappId: string
  title: string
  reportType: 'platform' | 'comprehensive' | 'custom'
  content: unknown
  metadata?: unknown
}

/** 报告数据 */
export interface TappReport {
  id: string
  title: string
  type: string
  content: unknown
  metadata?: unknown
  createdAt: string
  updatedAt: string
}

/**
 * 创建报告
 */
export async function createTappReport(
  request: CreateReportRequest,
  runtimeGrant?: string,
): Promise<{ success: boolean; report: TappReport }> {
  return apiRequest('/api/tapp/reports', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      title: request.title,
      report_type: request.reportType,
      content: request.content,
      metadata: request.metadata,
    }),
    runtimeGrant,
  })
}

/**
 * 获取 Tapp 报告列表
 */
export async function listTappReports(
  tappId: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; reports: TappReport[] }> {
  return apiRequest(`/api/tapp/reports/tapp/${encodeURIComponent(tappId)}`, {
    runtimeGrant,
  })
}

/**
 * 获取报告详情
 */
export async function getTappReport(
  tappId: string,
  reportId: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; report: TappReport }> {
  return apiRequest(
    `/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`,
    { runtimeGrant },
  )
}

/**
 * 更新报告
 */
export async function updateTappReport(
  tappId: string,
  reportId: string,
  updates: { title?: string; content?: unknown; metadata?: unknown },
  runtimeGrant?: string,
): Promise<{ success: boolean; report: TappReport }> {
  return apiRequest(
    `/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`,
    {
      method: 'PUT',
      body: JSON.stringify(updates),
      runtimeGrant,
    },
  )
}

/**
 * 删除报告
 */
export async function deleteTappReport(
  tappId: string,
  reportId: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; deleted: string }> {
  return apiRequest(
    `/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

// ============ P1: Media Control API ============

/** 媒体控制请求 */
export interface MediaControlRequest {
  tappId: string
  action:
    | 'play'
    | 'pause'
    | 'next'
    | 'prev'
    | 'seek'
    | 'volume'
    | 'mode'
    | 'mute'
    | 'unmute'
  value?: unknown
}

/** 媒体状态 */
export interface MediaStatus {
  isPlaying: boolean
  isPaused: boolean
  currentTrack: {
    id: string
    title: string
    artist: string
    album?: string
    cover?: string
    duration: number
    source: string
  } | null
  progress: {
    current: number
    duration: number
    percentage: number
  }
  playlist: {
    id: string
    name: string
    tracks: number
  } | null
  mode: 'sequence' | 'loop' | 'shuffle' | 'single'
  volume: number
  muted: boolean
}

/**
 * 媒体控制
 */
export async function mediaControl(
  request: MediaControlRequest,
  runtimeGrant?: string,
): Promise<{ success: boolean; action: string; value?: unknown }> {
  return apiRequest('/api/tapp/media/control', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      action: request.action,
      value: request.value,
    }),
    runtimeGrant,
  })
}

/**
 * 获取媒体状态
 */
export async function mediaStatus(runtimeGrant?: string): Promise<{
  success: boolean
  status: MediaStatus
}> {
  return apiRequest('/api/tapp/media/status', { runtimeGrant })
}

export async function createTappNotification(
  request: {
    tappId: string
    title?: string
    message: string
    notificationType?: 'success' | 'info' | 'warning' | 'error'
  },
  runtimeGrant?: string,
): Promise<string> {
  const response = await apiRequest<{
    success: boolean
    notification_id: string
  }>('/api/tapp/notifications', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      title: request.title,
      message: request.message,
      notification_type: request.notificationType,
    }),
    runtimeGrant,
  })
  return response.notification_id
}

// ============ P2: Component Registration API ============

/** 组件类型 */
export type ComponentType = 'theme' | 'agent'

/** 组件配置基础接口 */
export interface ComponentConfig {
  id: string
  [key: string]: unknown
}

/** Theme 组件配置 */
export interface ThemeComponentConfig extends ComponentConfig {
  name: string
  /**
   * 小组件表面样式（受约束枚举）：'glass' | 'solid' | 'flat' | 'outline'
   * 宿主仅消费此白名单值，见 useTappThemes 的校验。
   */
  surface?: string
  /**
   * 小组件光晕模式（受约束枚举）：'identity' | 'primary' | 'none'
   */
  glow?: string
}

/** Agent 组件配置 */
export interface AgentComponentConfig extends ComponentConfig {
  name: string
  description?: string
  capabilities: string[]
}

/** 已注册组件 */
export interface RegisteredComponent {
  id: string
  type: ComponentType
  tappId: string
  config: ComponentConfig
  registeredAt: string
  enabled: boolean
}

/**
 * 注册组件
 */
export async function registerComponent(
  tappId: string,
  componentType: ComponentType,
  config: ComponentConfig,
  runtimeGrant?: string,
): Promise<{ success: boolean; component: RegisteredComponent }> {
  return apiRequest('/api/tapp/components/register', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      component_type: componentType,
      config,
    }),
    runtimeGrant,
  })
}

/**
 * 注销组件
 */
export async function unregisterComponent(
  tappId: string,
  componentType: ComponentType,
  componentId: string,
  runtimeGrant?: string,
): Promise<{
  success: boolean
  unregistered: { id: string; type: string; tappId: string }
}> {
  return apiRequest(
    `/api/tapp/components/${encodeURIComponent(tappId)}/${componentType}/${encodeURIComponent(componentId)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

/**
 * 列出 Tapp 的已注册组件
 */
export async function listComponents(
  tappId: string,
  type?: ComponentType,
  runtimeGrant?: string,
): Promise<{ success: boolean; components: RegisteredComponent[] }> {
  const url = type
    ? `/api/tapp/components/${encodeURIComponent(tappId)}?type=${type}`
    : `/api/tapp/components/${encodeURIComponent(tappId)}`
  return apiRequest(url, { runtimeGrant })
}

/**
 * 列出所有指定类型的组件
 */
export async function listAllComponentsByType(
  componentType: ComponentType,
  runtimeGrant?: string,
): Promise<{
  success: boolean
  type: string
  components: RegisteredComponent[]
}> {
  return apiRequest(`/api/tapp/components/all/${componentType}`, {
    runtimeGrant,
  })
}

// ============ P2: Shortcut Registration API ============

/** 快捷键配置 */
export interface ShortcutConfig {
  id: string
  keys: string
  description: string
  action: string
  scope?: 'global' | 'tapp' | 'editor'
}

/** 已注册快捷键 */
export interface RegisteredShortcut {
  id: string
  tappId: string
  keys: string
  description: string
  action: string
  scope: string
  registeredAt: string
  enabled: boolean
}

/**
 * 注册快捷键
 */
export async function registerShortcut(
  tappId: string,
  config: ShortcutConfig,
  runtimeGrant?: string,
): Promise<{ success: boolean; shortcut: RegisteredShortcut }> {
  return apiRequest('/api/tapp/shortcuts/register', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      shortcut_id: config.id,
      keys: config.keys,
      description: config.description,
      action: config.action,
      scope: config.scope,
    }),
    runtimeGrant,
  })
}

/**
 * 注销快捷键
 */
export async function unregisterShortcut(
  tappId: string,
  shortcutId: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; unregistered: string }> {
  return apiRequest(
    `/api/tapp/shortcuts/${encodeURIComponent(tappId)}/${encodeURIComponent(shortcutId)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

/**
 * 列出快捷键
 */
export async function listShortcuts(
  tappId?: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; shortcuts: RegisteredShortcut[] }> {
  const url = tappId
    ? `/api/tapp/shortcuts?tapp_id=${encodeURIComponent(tappId)}`
    : '/api/tapp/shortcuts'
  return apiRequest(url, { runtimeGrant })
}

// ============ Event Broker API ============

export async function publishEvent(
  request: PublishEventRequest,
  runtimeGrant: string,
): Promise<{
  accepted: boolean
  deduplicated: boolean
  delivered: number
  event: TappEvent
}> {
  return apiRequest('/api/tapp/events/publish', {
    method: 'POST',
    body: JSON.stringify(request),
    runtimeGrant,
  })
}

export async function streamEvents(
  runtimeGrant: string,
  onEvent: (event: string, data: unknown) => void,
  signal?: AbortSignal,
): Promise<void> {
  return streamRuntimeEvents(
    '/api/tapp/events/stream',
    runtimeGrant,
    onEvent,
    signal,
  )
}

export async function streamAgentInteractions(
  runtimeGrant: string,
  onInteraction: (interaction: AgentInteractionV2) => void,
  signal?: AbortSignal,
): Promise<void> {
  return streamRuntimeEvents(
    '/api/tapp/agent/v2/interactions/stream',
    runtimeGrant,
    (event, data) => {
      if (event === 'interaction' && data && typeof data === 'object') {
        onInteraction(data as AgentInteractionV2)
      }
    },
    signal,
  )
}

export async function getAgentInteraction(
  interactionId: string,
  runtimeGrant: string,
): Promise<AgentInteractionV2> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}`,
    { runtimeGrant },
  )
}

export async function acceptAgentInteraction(
  interactionId: string,
  runtimeGrant: string,
): Promise<AgentInteractionV2> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}/accept`,
    { method: 'POST', runtimeGrant },
  )
}

export async function submitAgentInteractionResult(
  interactionId: string,
  result: { data: unknown; summary?: string; idempotencyKey: string },
  runtimeGrant: string,
): Promise<AgentInteractionV2> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}/result`,
    { method: 'POST', body: JSON.stringify(result), runtimeGrant },
  )
}

export async function rejectAgentInteraction(
  interactionId: string,
  reason: string,
  runtimeGrant: string,
): Promise<AgentInteractionV2> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}/reject`,
    { method: 'POST', body: JSON.stringify({ reason }), runtimeGrant },
  )
}

export async function requestAgentIntent(
  interactionId: string,
  request: { type: string; params?: unknown; reason: string },
  runtimeGrant: string,
): Promise<unknown> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}/intents`,
    {
      method: 'POST',
      body: JSON.stringify({ ...request, hostConfirmed: true }),
      runtimeGrant,
    },
  )
}
