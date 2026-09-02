/** Installed Tapp package resources, assets and export operations. */

import { API_URL } from '../../config'
import { currentCopy } from '../../i18n/localeCopy'
import { apiRequest } from './TappHttpClient'

export interface TappResources {
  /** 包内 `.js` 文件：相对路径 → 源码，只含该 mode 相关层。 */
  modules: Record<string, string>
  /** 宿主解析的 require 图；缺失表示旧后端，由前端兼容扫描。 */
  moduleResolutions?: Record<string, Record<string, string>>
  coreEntry?: string
  pageEntry?: string
  widgetEntries?: Record<string, string>
  /** 作者样式（层声明）。 */
  coreStyles?: string
  pageStyles?: string
  widgetStyles?: Record<string, string>
  /** 宿主预编译 Tailwind，与作者样式是两条通道。 */
  widgetCSS?: string
  pageCSS?: string
  widgetTemplates?: Record<string, Record<string, string>>
  pageTemplate?: string
  i18n?: Record<string, unknown>
}

interface TappResourcesRaw {
  modules: Record<string, string>
  module_resolutions?: Record<string, Record<string, string>>
  core_entry?: string
  page_entry?: string
  widget_entries?: Record<string, string>
  core_styles?: string
  page_styles?: string
  widget_styles?: Record<string, string>
  widget_css?: string
  page_css?: string
  widget_templates?: Record<string, Record<string, string>>
  page_template?: string
  i18n?: Record<string, unknown>
}

/** Projection of installed package resources. Matches backend `mode` query. */
export type TappResourceMode = 'full' | 'core' | 'widget' | 'page'

export async function getTappResources(
  tappId: string,
  options?: { mode?: TappResourceMode; widgetId?: string },
): Promise<TappResources> {
  const mode =
    options?.mode && options.mode !== 'full' ? options.mode : undefined
  const query = new URLSearchParams()
  if (mode) query.set('mode', mode)
  if (options?.widgetId) query.set('widget_id', options.widgetId)
  const params = query.size > 0 ? `?${query.toString()}` : ''
  const response = await fetch(
    `${API_URL}/api/tapps/${encodeURIComponent(tappId)}/resources${params}`,
    { method: 'GET', credentials: 'include' },
  )
  if (!response.ok) {
    // 没有旧端点回退：包结构不符合当前契约时后端返回 409，让它照常抛出，
    // 不要再换一条路把不受支持的包送进沙箱。
    throw new Error(
      `${currentCopy().tapp.loadAppFailed} (${response.status})`,
    )
  }
  const raw: TappResourcesRaw = await response.json()
  return {
    modules: raw.modules || {},
    moduleResolutions: raw.module_resolutions,
    coreEntry: raw.core_entry,
    pageEntry: raw.page_entry,
    widgetEntries: raw.widget_entries,
    coreStyles: raw.core_styles,
    pageStyles: raw.page_styles,
    widgetStyles: raw.widget_styles,
    widgetCSS: raw.widget_css,
    pageCSS: raw.page_css,
    widgetTemplates: raw.widget_templates,
    pageTemplate: raw.page_template,
    i18n: raw.i18n,
  }
}

export interface TappAssetPayload {
  path: string
  mimeType: string
  size: number
  base64: string
}

export async function getTappAsset(
  tappId: string,
  path: string,
): Promise<TappAssetPayload> {
  const params = new URLSearchParams({ path })
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/asset?${params.toString()}`,
  )
}

export async function exportTapp(tappId: string): Promise<void> {
  const response = await fetch(
    `${API_URL}/api/tapps/${encodeURIComponent(tappId)}/export`,
    { credentials: 'include' },
  )
  if (!response.ok) {
    throw new Error(
      `${currentCopy().tapp.exportFailed} (${response.status})`,
    )
  }

  const disposition = response.headers.get('Content-Disposition')
  let filename = `${tappId}.tapp`
  const match = disposition?.match(/filename="(.+)"/)
  if (match) {
    filename = match[1]
  }

  const blob = await response.blob()
  const downloadUrl = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = downloadUrl
  anchor.download = filename
  document.body.appendChild(anchor)
  anchor.click()
  document.body.removeChild(anchor)
  URL.revokeObjectURL(downloadUrl)
}
