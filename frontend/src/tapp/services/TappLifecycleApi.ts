/** Installed Tapp discovery, installation and lifecycle operations. */

import type { TappCodeStructure, TappManifest } from '../types'
import type { SeparatedCSSRequest } from './TappPackageResourceApi'
import { API_URL } from '../../config'
import { getCSRFToken } from '../../utils/csrf'
import { generateOnDemandTailwindCSS } from '../runtime/sandbox/styles'
import {
  buildPlaygroundPackageFiles,
  packageFilesToDirectInstallBody,
} from '../utils/playgroundPackageFiles'
import { apiRequest } from './TappHttpClient'

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
 * 清理用户的临时 Tapp（登出时调用）
 * @returns 删除的临时 Tapp 数量
 */
export async function cleanupTemporaryTapps(): Promise<number> {
  const result = await apiRequest<number>('/api/tapps/cleanup-temporary', {
    method: 'POST',
  })
  return result
}
