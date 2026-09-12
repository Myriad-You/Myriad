import { API_URL } from '../../config'
import { hostLocaleHeaders } from '../../i18n/hostLocaleHeaders'
import { currentCopy } from '../../i18n/localeCopy'
import { apiRequest } from './TappHttpClient'

export interface TappResources {
  modules: Record<string, string>
  moduleResolutions?: Record<string, Record<string, string>>
  coreEntry?: string
  pageEntry?: string
  widgetEntries?: Record<string, string>
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
    {
      method: 'GET',
      credentials: 'include',
      headers: hostLocaleHeaders(),
    },
  )
  if (!response.ok) {
    // 契约不符时 409 照抛，不换路送进沙箱。
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
    { credentials: 'include', headers: hostLocaleHeaders() },
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
