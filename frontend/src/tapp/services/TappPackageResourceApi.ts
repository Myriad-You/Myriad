/** Installed Tapp package resources, assets and export operations. */

import { API_URL } from '../../config'
import { apiRequest } from './TappHttpClient'

export async function getTappCode(tappId: string): Promise<string> {
  const response = await fetch(
    `${API_URL}/api/tapps/${encodeURIComponent(tappId)}/code`,
    { method: 'GET', credentials: 'include' },
  )
  if (!response.ok) {
    throw new Error(`Failed to get Tapp code: ${response.status}`)
  }
  return response.text()
}

export interface TappResources {
  code: string
  styles?: string
  widgetStyles?: string
  pageStyles?: string
  widgetCSS?: string
  pageCSS?: string
  widgetTemplates?: Record<string, Record<string, string>>
  pageTemplate?: string
  cssMode?: 'unified' | 'separated'
  i18n?: Record<string, unknown>
  pageModules?: Record<string, string>
  pageModuleOrder?: string[]
}

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

export async function getTappResources(tappId: string): Promise<TappResources> {
  const response = await fetch(
    `${API_URL}/api/tapps/${encodeURIComponent(tappId)}/resources`,
    { method: 'GET', credentials: 'include' },
  )
  if (!response.ok) {
    if (response.status === 404) {
      return { code: await getTappCode(tappId) }
    }
    throw new Error(`Failed to get Tapp resources: ${response.status}`)
  }
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

export async function getTappAsset(
  tappId: string,
  path: string,
): Promise<TappAssetPayload> {
  const params = new URLSearchParams({ path })
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/asset?${params.toString()}`,
  )
}

export interface SeparatedCSSRequest {
  widgetCss?: string
  pageCss?: string
}

export async function updateSeparatedCSS(
  tappId: string,
  css: SeparatedCSSRequest,
): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/separated-css`, {
    method: 'POST',
    body: JSON.stringify(css),
  })
}

export async function exportTapp(tappId: string): Promise<void> {
  const response = await fetch(
    `${API_URL}/api/tapps/${encodeURIComponent(tappId)}/export`,
    { credentials: 'include' },
  )
  if (!response.ok) {
    throw new Error(`Export failed: ${response.status}`)
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
