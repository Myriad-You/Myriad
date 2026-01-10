/**
 * Tapp API 服务
 * 提供 Tapp 与后端 API 的通信功能
 */

import type { TappCodeStructure } from '../examples/tapps/types'
import type {
  AIAnalyzeRequest,
  AIAnalyzeResponse,
  AIGenerateRequest,
  AIGenerateResponse,
  NewPlatformItem,
  PlatformInfo,
  PlatformItemResult,
  RegisteredWidget,
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
  installed_at: string
  last_run_at?: string
  /** 是否为临时安装（普通用户安装的 Tapp） */
  is_temporary?: boolean
  /** 是否为管理员的 Tapp */
  is_admin_tapp?: boolean
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
async function apiRequest<T>(
  endpoint: string,
  options: RequestInit = {},
  retryOnCsrf: boolean = true,
): Promise<T> {
  // 只有非 GET 请求才需要 CSRF token
  const method = (options.method || 'GET').toUpperCase()
  const needsCsrf = method !== 'GET' && method !== 'HEAD' && method !== 'OPTIONS'
  const csrfToken = needsCsrf ? (await getCSRFToken() || '') : ''

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...options.headers as Record<string, string>,
  }

  // 只在需要时添加 CSRF token
  if (needsCsrf && csrfToken) {
    headers['X-CSRF-Token'] = csrfToken
  }

  const response = await fetch(`${API_URL}${endpoint}`, {
    ...options,
    headers,
    credentials: 'include',
  })

  // 如果 CSRF Token 无效，尝试刷新后重试一次
  if (response.status === 403 && retryOnCsrf) {
    const errorData = await response.json().catch(() => ({}))
    if (errorData.error?.includes('CSRF') || errorData.error?.includes('csrf')) {
      // 强制刷新 CSRF Token
      await getCSRFToken(true)
      // 重试请求（不再重试）
      return apiRequest(endpoint, options, false)
    }
  }

  if (!response.ok) {
    const errorData = await response.json().catch(() => ({}))
    throw new Error(errorData.message || errorData.error || `API Error: ${response.status}`)
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

// ============ Tapp 应用管理 API ============

/**
 * 获取已安装的 Tapp 列表
 */
export async function listTapps(): Promise<TappListItem[]> {
  return apiRequest('/api/tapps')
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
export async function getRecentTapps(limit: number = 10): Promise<RecentTappItem[]> {
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
  widgetTemplates?: Record<string, string>
  /** Widget 专用 CSS */
  widgetCss?: string
  /** Page 专用 CSS */
  pageCss?: string
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
): Promise<TappListItem> {
  // 合并 JS 代码（core + widget + page）
  const jsCode = [
    code.core,
    code.widget ? `\n// ========== Widget Code ==========\n${code.widget}` : '',
    code.page ? `\n// ========== Page Code ==========\n${code.page}` : '',
  ].join('')

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
    // 为每个 widget 尺寸创建模板
    const templates: Record<string, string> = {}
    for (const widget of manifest.widgets) {
      if (widget.sizes) {
        for (const size of widget.sizes) {
          templates[size] = code.widgetHtml
        }
      }
    }
    if (Object.keys(templates).length > 0) {
      requestBody.widgetTemplates = templates
    }
  }

  return apiRequest('/api/tapps/install', {
    method: 'POST',
    body: JSON.stringify(requestBody),
  })
}

/**
 * 从代码和清单安装 Tapp（用于示例 Tapp）
 * 支持完整的代码结构，包括 CSS 和 HTML 模板
 *
 * 🎯 自动生成分离式预编译 Tailwind CSS（widget.css 和 page.css）
 */
export async function installFromCode(manifest: TappManifest, code: TappCodeStructure): Promise<TappListItem> {
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
  ].join('\n')
  const pageCss = generateOnDemandTailwindCSS(pageSources)

  // 🎯 安装 Tapp
  const result = await installTapp(manifest, code, manifest.permissions)

  // 安装成功后更新分离式 CSS
  if (result && result.id) {
    try {
      await updateSeparatedCSS(result.id, { widgetCss, pageCss })
    }
    catch (cssError) {
      console.warn('Failed to update separated CSS:', cssError)
    }
  }

  return result
}

/**
 * 上传 .tapp 文件安装（multipart 文件上传）
 *
 * @param file .tapp 文件
 * @param permissions 授权的权限列表（可选）
 * @returns 安装后的 Tapp 信息
 */
export async function installTappFile(file: File, permissions?: string[]): Promise<TappListItem> {
  const formData = new FormData()
  formData.append('file', file)
  if (permissions) {
    formData.append('permissions', JSON.stringify(permissions))
  }

  const csrfToken = await getCSRFToken() || ''

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
    throw new Error(errorData.message || errorData.error || `Install failed: ${response.status}`)
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
 * 使用统一的 /install API，source 设为 "store"
 *
 * @param request 安装请求
 * @returns 安装后的 Tapp 信息
 */
export async function installFromStore(request: InstallFromStoreRequest): Promise<TappListItem> {
  return apiRequest('/api/tapps/install', {
    method: 'POST',
    body: JSON.stringify({
      source: 'store',
      storeSource: request.source,
      tappId: request.tappId,
      permissions: request.permissions,
    }),
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
  const response = await fetch(`${API_URL}/api/tapps/${encodeURIComponent(tappId)}/code`, {
    method: 'GET',
    credentials: 'include',
  })
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
  /** Widget HTML 模板（按尺寸） */
  widgetTemplates?: Record<string, string>
  /** Page HTML 模板 */
  pageTemplate?: string
  /** CSS 架构模式 */
  cssMode?: 'unified' | 'separated'
}

/** 后端原始响应格式（snake_case） */
interface TappResourcesRaw {
  code: string
  styles?: string
  widget_styles?: string
  page_styles?: string
  widget_css?: string
  page_css?: string
  widget_templates?: Record<string, string>
  page_template?: string
  css_mode?: 'unified' | 'separated'
}

/**
 * 获取 Tapp 完整资源（代码 + CSS + HTML 模板）
 * 支持混合渲染模式
 */
export async function getTappResources(tappId: string): Promise<TappResources> {
  const response = await fetch(`${API_URL}/api/tapps/${encodeURIComponent(tappId)}/resources`, {
    method: 'GET',
    credentials: 'include',
  })
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
  }
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
export async function updateSeparatedCSS(tappId: string, css: SeparatedCSSRequest): Promise<void> {
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
export async function uninstallTapp(tappId: string, options?: UninstallOptions): Promise<void> {
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
export async function updateTappFromStore(tappId: string, request: UpdateTappFromStoreRequest): Promise<TappListItem> {
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
 * 获取 Tapp 注册的小组件列表
 */
export async function listTappWidgets(tappId: string): Promise<RegisteredWidget[]> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/widgets`)
}

/**
 * 获取所有已注册的小组件
 * 通过新的 /api/tapps/widgets 接口一次性获取所有小组件
 */
export async function getAllWidgets(): Promise<RegisteredWidget[]> {
  try {
    // 使用新的单一接口获取所有小组件
    const widgets = await apiRequest<RegisteredWidget[]>('/api/tapps/widgets')
    return widgets
  }
  catch {
    // 回退到旧方式（遍历每个 Tapp）
    const tapps = await listTapps()
    const widgets: RegisteredWidget[] = []

    for (const tapp of tapps) {
      // 只要 Tapp 已安装（不是 disabled 状态），就获取其小组件
      if (tapp.status !== 'disabled') {
        try {
          const tappWidgets = await listTappWidgets(tapp.id)
          widgets.push(...tappWidgets)
        }
        catch {
          // 静默失败
        }
      }
    }

    return widgets
  }
}

/**
 * 注册小组件到后端
 */
export async function registerTappWidget(tappId: string, config: WidgetRegistration): Promise<RegisteredWidget> {
  const requestBody = {
    id: config.id,
    name: config.name,
    description: config.description,
    icon: config.icon,
    default_size: config.defaultSize,
    sizes: config.sizes,
    category: config.category,
    config: config.configSchema || {},
  }

  const result = await apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/widgets`, {
    method: 'POST',
    body: JSON.stringify(requestBody),
  })
  return result as RegisteredWidget
}

/**
 * 注销小组件
 */
export async function unregisterTappWidget(tappId: string, widgetId: string): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/widgets/${encodeURIComponent(widgetId)}`, {
    method: 'DELETE',
  })
}

// ============ Tapp 存储 API ============

/**
 * 获取存储值
 */
export async function getStorage(tappId: string, key: string): Promise<unknown> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`)
}

/**
 * 设置存储值
 */
export async function setStorage(tappId: string, key: string, value: unknown): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`, {
    method: 'POST',
    body: JSON.stringify(value),
  })
}

/**
 * 删除存储值
 */
export async function removeStorage(tappId: string, key: string): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`, {
    method: 'DELETE',
  })
}

/**
 * 获取所有存储键
 */
export async function listStorageKeys(tappId: string): Promise<string[]> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage`)
}

/**
 * 清除所有存储
 */
export async function clearStorage(tappId: string): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage`, {
    method: 'DELETE',
  })
}

// ============ 平台数据 API ============

/**
 * 获取已启用的平台列表
 */
export async function listEnabledPlatforms(): Promise<PlatformInfo[]> {
  const data = await apiRequest<{ platforms: PlatformInfo[] }>('/api/platforms')
  return data.platforms.filter(p => p.enabled)
}

/**
 * 获取平台数据
 */
export async function getPlatformData(
  platform: string,
  options?: {
    limit?: number
    offset?: number
    filter?: Record<string, unknown>
  },
): Promise<{
  items: unknown[]
  total: number
  platform: string
}> {
  const params = new URLSearchParams()
  if (options?.limit)
    params.set('limit', String(options.limit))
  if (options?.offset)
    params.set('offset', String(options.offset))
  if (options?.filter)
    params.set('filter', JSON.stringify(options.filter))

  return apiRequest(`/api/tapp/platform/${platform}/data?${params}`)
}

/**
 * 获取平台统计数据
 */
export async function getPlatformStats(platform: string): Promise<{
  platform: string
  total: number
  distribution: Record<string, number>
  recentActivity: { date: string, count: number }[]
}> {
  return apiRequest(`/api/tapp/platform/${platform}/stats`)
}

/**
 * 获取平台数据分布
 */
export async function getPlatformDistribution(
  platform: string,
  dimension: string,
): Promise<{ dimension: string, data: { label: string, value: number }[] }> {
  return apiRequest(`/api/tapp/platform/${platform}/distribution/${dimension}`)
}

/**
 * 添加新的平台数据条目
 */
export async function addPlatformItem(
  tappId: string,
  item: NewPlatformItem,
): Promise<PlatformItemResult> {
  return apiRequest('/api/tapp/platform/items', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      item,
    }),
  })
}

/**
 * 批量添加平台数据条目
 */
export async function addPlatformItems(
  tappId: string,
  items: NewPlatformItem[],
): Promise<{ success: boolean, results: PlatformItemResult[] }> {
  return apiRequest('/api/tapp/platform/items/batch', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      items,
    }),
  })
}

// ============ AI API ============

/**
 * AI 生成
 */
export async function aiGenerate(
  tappId: string,
  request: AIGenerateRequest,
): Promise<AIGenerateResponse> {
  return apiRequest('/api/tapp/ai/generate', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      ...request,
    }),
  })
}

/**
 * AI 分析
 */
export async function aiAnalyze(
  tappId: string,
  request: AIAnalyzeRequest,
): Promise<AIAnalyzeResponse> {
  return apiRequest('/api/tapp/ai/analyze', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      ...request,
    }),
  })
}

/**
 * AI 图片生成请求
 */
export interface AIImageGenerateRequest {
  prompt: string
  width?: number
  height?: number
  model?: string
  enhance?: boolean
  seed?: number
}

/**
 * AI 图片生成响应
 */
export interface AIImageGenerateResponse {
  success: boolean
  provider: 'pollinations' | 'imaginepro'
  url?: string // Pollinations 直接返回 URL
  task_id?: string // ImaginePro 返回任务 ID
  status?: string // ImaginePro 任务状态
  result?: unknown // ImaginePro 完整结果
  width: number
  height: number
  model: string
  prompt: string
  quotaRemaining: number
}

/**
 * AI 图片生成
 */
export async function aiImageGenerate(
  tappId: string,
  request: AIImageGenerateRequest,
): Promise<AIImageGenerateResponse> {
  return apiRequest('/api/tapp/ai/image', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      ...request,
    }),
  })
}

// ============ 报告 API ============

/**
 * 获取报告列表
 */
export async function listReports(): Promise<{
  reports: {
    id: string
    platform: string
    type: 'platform' | 'comprehensive'
    createdAt: string
    summary?: string
  }[]
}> {
  return apiRequest('/api/reports/list')
}

/**
 * 获取单个报告
 */
export async function getReport(reportId: string): Promise<{
  id: string
  platform?: string
  type: 'platform' | 'comprehensive'
  content: unknown
  createdAt: string
}> {
  return apiRequest(`/api/reports/${reportId}`)
}

/**
 * 获取平台报告
 */
export async function getPlatformReport(platform: string): Promise<{
  platform: string
  summary: string
  insights: string[]
  metadata: unknown
  cardVisuals: unknown
  createdAt: string
} | null> {
  try {
    return await apiRequest(`/api/reports/platform/${platform}`)
  }
  catch {
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
  updateTappFromStore,
  getTapp,
  getTappCode,
  getTappResources,
  startTapp,
  stopTapp,
  uninstallTapp,
  exportTapp,
  // Widget 管理
  listTappWidgets,
  getAllWidgets,
  registerTappWidget,
  unregisterTappWidget,
  // 存储
  getStorage,
  setStorage,
  removeStorage,
  listStorageKeys,
  clearStorage,
  // Platform
  listEnabledPlatforms,
  getPlatformData,
  getPlatformStats,
  getPlatformDistribution,
  addPlatformItem,
  addPlatformItems,
  // AI
  aiGenerate,
  aiAnalyze,
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
  // P1: AI Chat
  aiChat,
  // P1: Report CRUD
  createTappReport,
  listTappReports,
  getTappReport,
  updateTappReport,
  deleteTappReport,
  // P1: Media Control
  mediaControl,
  mediaStatus,
}

// ============ P0: Data Transform API ============

/** 数据输入源 */
export type DataInput
  = | { source: 'platform', platform: string }
    | { source: 'storage', key: string }
    | { source: 'inline', data: unknown }

/** 数据输出目标 */
export type DataOutput
  = | { target: 'platform', platform: string }
    | { target: 'storage', key: string }

/** 处理步骤 */
export type ProcessStep
  = | { type: 'filter', field: string, operator: string, value: unknown }
    | { type: 'sort', field: string, order?: 'asc' | 'desc' }
    | { type: 'limit', count: number }
    | { type: 'offset', count: number }
    | { type: 'select', fields: string[] }
    | { type: 'group', by: string }
    | { type: 'aggregate', operation: 'count' | 'sum' | 'avg' | 'min' | 'max', field?: string }
    | { type: 'dedupe', key: string }
    | { type: 'map', expression: string }

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
export async function dataTransform(request: DataTransformRequest): Promise<DataTransformResponse> {
  return apiRequest('/api/tapp/data/transform', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      input: request.input,
      pipeline: request.pipeline,
      output: request.output,
    }),
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
  avatar: string | null
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
export async function getContextApp(): Promise<AppContext> {
  return apiRequest('/api/tapp/context/app')
}

/**
 * 获取用户上下文
 */
export async function getContextUser(): Promise<UserContext> {
  return apiRequest('/api/tapp/context/user')
}

/**
 * 获取播放器上下文
 */
export async function getContextPlayer(): Promise<PlayerContext> {
  return apiRequest('/api/tapp/context/player')
}

/**
 * 获取导航上下文
 */
export async function getContextNavigation(): Promise<NavigationContext> {
  return apiRequest('/api/tapp/context/navigation')
}

/**
 * 获取系统上下文
 */
export async function getContextSystem(): Promise<SystemContext> {
  return apiRequest('/api/tapp/context/system')
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
export async function getContextGeo(): Promise<GeoContext> {
  const result = await apiRequest<{ success: boolean, data: GeoContext }>('/api/tapp/context/geo')
  if (result.success && result.data) {
    return result.data
  }
  throw new Error('Failed to get geo info')
}

// 从统一的地理位置工具重新导出，避免重复实现
export {
  type GeoLocationData,
  getClientGeoLocation,
  isUserInChinaMainland,
  resetGeoCache,
} from '../../utils/geoLocation'

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
): Promise<TappApiExecuteResponse> {
  // 不使用 apiRequest 因为它会自动解包 data 字段
  // 这里需要返回完整的 { success, data, error, cached } 响应
  const csrfToken = await getCSRFToken() || ''

  const response = await fetch(`${API_URL}/api/tapp/${encodeURIComponent(tappId)}/api/${encodeURIComponent(apiName)}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-CSRF-Token': csrfToken,
    },
    body: JSON.stringify({ params }),
    credentials: 'include',
  })

  const result = await response.json()

  // 返回完整的响应结构
  return {
    success: result.success ?? false,
    data: result.data,
    error: result.error,
    cached: result.cached,
  }
}

/**
 * 列出 Tapp 可用的 API
 *
 * @param tappId - Tapp ID
 * @returns API 列表
 */
export async function listTappApis(tappId: string): Promise<TappApiInfo[]> {
  const result = await apiRequest<{ success: boolean, apis: TappApiInfo[] }>(
    `/api/tapp/${encodeURIComponent(tappId)}/apis`,
  )
  return result.apis || []
}

// ============ P1: AI Chat API ============

/** 聊天消息 */
export interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
}

/** AI Chat 请求 */
export interface AIChatRequest {
  tappId: string
  messages: ChatMessage[]
  context?: {
    includePlatformStats?: boolean
    includeUserProfile?: boolean
    customData?: unknown
  }
  options?: {
    maxTokens?: number
    temperature?: number
    stream?: boolean
  }
}

/** AI Chat 响应 */
export interface AIChatResponse {
  success: boolean
  message: ChatMessage
  usage: {
    promptTokens: number
    completionTokens: number
    totalTokens: number
  }
  sessionId?: string
}

/**
 * AI 对话
 */
export async function aiChat(request: AIChatRequest): Promise<AIChatResponse> {
  return apiRequest('/api/tapp/ai/chat', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      messages: request.messages,
      context: request.context
        ? {
            include_platform_stats: request.context.includePlatformStats,
            include_user_profile: request.context.includeUserProfile,
            custom_data: request.context.customData,
          }
        : undefined,
      options: request.options,
    }),
  })
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
export async function createTappReport(request: CreateReportRequest): Promise<{ success: boolean, report: TappReport }> {
  return apiRequest('/api/tapp/reports', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      title: request.title,
      report_type: request.reportType,
      content: request.content,
      metadata: request.metadata,
    }),
  })
}

/**
 * 获取 Tapp 报告列表
 */
export async function listTappReports(tappId: string): Promise<{ success: boolean, reports: TappReport[] }> {
  return apiRequest(`/api/tapp/reports/tapp/${encodeURIComponent(tappId)}`)
}

/**
 * 获取报告详情
 */
export async function getTappReport(tappId: string, reportId: string): Promise<{ success: boolean, report: TappReport }> {
  return apiRequest(`/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`)
}

/**
 * 更新报告
 */
export async function updateTappReport(
  tappId: string,
  reportId: string,
  updates: { title?: string, content?: unknown, metadata?: unknown },
): Promise<{ success: boolean, report: TappReport }> {
  return apiRequest(`/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`, {
    method: 'PUT',
    body: JSON.stringify(updates),
  })
}

/**
 * 删除报告
 */
export async function deleteTappReport(tappId: string, reportId: string): Promise<{ success: boolean, deleted: string }> {
  return apiRequest(`/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`, {
    method: 'DELETE',
  })
}

// ============ P1: Media Control API ============

/** 媒体控制请求 */
export interface MediaControlRequest {
  tappId: string
  action: 'play' | 'pause' | 'next' | 'prev' | 'seek' | 'volume' | 'mode' | 'mute' | 'unmute'
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
export async function mediaControl(request: MediaControlRequest): Promise<{ success: boolean, action: string, value?: unknown }> {
  return apiRequest('/api/tapp/media/control', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      action: request.action,
      value: request.value,
    }),
  })
}

/**
 * 获取媒体状态
 */
export async function mediaStatus(): Promise<{ success: boolean, status: MediaStatus }> {
  return apiRequest('/api/tapp/media/status')
}

// ============ P2: Component Registration API ============

/** 组件类型 */
export type ComponentType = 'page' | 'theme' | 'agent'

/** 组件配置基础接口 */
export interface ComponentConfig {
  id: string
  [key: string]: unknown
}

/** Page 组件配置 */
export interface PageComponentConfig extends ComponentConfig {
  path: string
  title: string
  icon?: string
  menu?: boolean
  order?: number
}

/** Theme 组件配置 */
export interface ThemeComponentConfig extends ComponentConfig {
  name: string
  colors?: Record<string, string>
  styles?: string
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
): Promise<{ success: boolean, component: RegisteredComponent }> {
  return apiRequest('/api/tapp/components/register', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      component_type: componentType,
      config,
    }),
  })
}

/**
 * 注销组件
 */
export async function unregisterComponent(
  tappId: string,
  componentType: ComponentType,
  componentId: string,
): Promise<{ success: boolean, unregistered: { id: string, type: string, tappId: string } }> {
  return apiRequest(`/api/tapp/components/${encodeURIComponent(tappId)}/${componentType}/${encodeURIComponent(componentId)}`, {
    method: 'DELETE',
  })
}

/**
 * 列出 Tapp 的已注册组件
 */
export async function listComponents(
  tappId: string,
  type?: ComponentType,
): Promise<{ success: boolean, components: RegisteredComponent[] }> {
  const url = type
    ? `/api/tapp/components/${encodeURIComponent(tappId)}?type=${type}`
    : `/api/tapp/components/${encodeURIComponent(tappId)}`
  return apiRequest(url)
}

/**
 * 列出所有指定类型的组件
 */
export async function listAllComponentsByType(
  componentType: ComponentType,
): Promise<{ success: boolean, type: string, components: RegisteredComponent[] }> {
  return apiRequest(`/api/tapp/components/all/${componentType}`)
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
): Promise<{ success: boolean, shortcut: RegisteredShortcut }> {
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
  })
}

/**
 * 注销快捷键
 */
export async function unregisterShortcut(
  tappId: string,
  shortcutId: string,
): Promise<{ success: boolean, unregistered: string }> {
  return apiRequest(`/api/tapp/shortcuts/${encodeURIComponent(tappId)}/${encodeURIComponent(shortcutId)}`, {
    method: 'DELETE',
  })
}

/**
 * 列出快捷键
 */
export async function listShortcuts(
  tappId?: string,
): Promise<{ success: boolean, shortcuts: RegisteredShortcut[] }> {
  const url = tappId ? `/api/tapp/shortcuts?tapp_id=${encodeURIComponent(tappId)}` : '/api/tapp/shortcuts'
  return apiRequest(url)
}

// ============ P2: Event Bus API ============

/** 事件发布请求 */
export interface PublishEventRequest {
  tappId: string
  eventType: string
  payload: unknown
  target?: 'all' | 'self' | string
}

/** 发布的事件 */
export interface PublishedEvent {
  id: string
  type: string
  tappId: string
  target: string
  timestamp: string
}

/**
 * 发布事件
 */
export async function publishEvent(
  request: PublishEventRequest,
): Promise<{ success: boolean, event: PublishedEvent }> {
  return apiRequest('/api/tapp/events/publish', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      event_type: request.eventType,
      payload: request.payload,
      target: request.target,
    }),
  })
}

/**
 * 获取事件订阅
 */
export async function getEventSubscriptions(
  tappId: string,
): Promise<{ success: boolean, tappId: string, subscriptions: string[] }> {
  return apiRequest(`/api/tapp/events/subscriptions/${encodeURIComponent(tappId)}`)
}

/**
 * 更新事件订阅
 */
export async function updateEventSubscriptions(
  tappId: string,
  subscriptions: string[],
): Promise<{ success: boolean, tappId: string, subscriptions: string[] }> {
  return apiRequest(`/api/tapp/events/subscriptions/${encodeURIComponent(tappId)}`, {
    method: 'PUT',
    body: JSON.stringify({ subscriptions }),
  })
}
