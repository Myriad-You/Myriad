/**
 * Client-side .tapp export from Playground project state.
 * Mirrors installFromCode / buildDirectTappRequest file mapping so the zip
 * can be reinstalled via install-file without a prior server install.
 */

import type { TappCodeStructure, TappManifest } from '../types'
import type { TappPlaygroundProject } from '../services/TappPlaygroundService'

function buildMainJs(code: TappCodeStructure): string {
  // Match install path: with pageModules, page lives in page/*; main holds
  // core + optional widget. Without pageModules, monorepo-style merge includes page.
  const hasPageModules =
    !!code.pageModules && Object.keys(code.pageModules).length > 0
  if (hasPageModules) {
    // Include core (required for extractCoreCode / headless). buildDirectTappRequest
    // currently omits core in this branch; export keeps a reinstallable package.
    return [
      code.core || '',
      code.widget
        ? `\n// ========== Widget Code ==========\n${code.widget}`
        : '',
    ].join('')
  }
  return [
    code.core || '',
    code.widget
      ? `\n// ========== Widget Code ==========\n${code.widget}`
      : '',
    code.page ? `\n// ========== Page Code ==========\n${code.page}` : '',
  ].join('')
}

/**
 * Ensure manifest paths exist for every file we put in the zip.
 */
function normalizeManifestForExport(
  project: TappPlaygroundProject,
): TappManifest {
  const { manifest, code } = project
  const next: TappManifest = { ...manifest }

  if (!next.main || !next.main.trim()) {
    next.main = 'main.js'
  }

  if (code.styles && !next.styles) {
    next.styles = 'styles.css'
  }
  if (!next.cssMode) {
    next.cssMode = 'unified'
  }

  if (code.pageHtml && !next.pageTemplate) {
    next.pageTemplate = 'page.html'
  }

  const moduleNames = Object.keys(code.pageModules || {})
  if (moduleNames.length > 0) {
    const order =
      code.pageModuleOrder && code.pageModuleOrder.length > 0
        ? code.pageModuleOrder.filter((name) => moduleNames.includes(name))
        : next.pageModules?.filter((name) => moduleNames.includes(name)) || []
    const remaining = moduleNames
      .filter((name) => !order.includes(name))
      .sort((a, b) => {
        if (a === 'index.js') return 1
        if (b === 'index.js') return -1
        return a.localeCompare(b)
      })
    next.pageModules = [...order, ...remaining]
    next.hasPage = true
  }

  // Ensure widget template paths when we will write widgetHtml
  if (code.widgetHtml && next.widgets && next.widgets.length > 0) {
    next.widgets = next.widgets.map((widget) => {
      if (widget.templates && Object.keys(widget.templates).length > 0) {
        return widget
      }
      const sizes =
        widget.sizes && widget.sizes.length > 0
          ? widget.sizes
          : [widget.defaultSize || '2x2']
      const templates: Record<string, string> = {}
      for (const size of sizes) {
        templates[size] =
          sizes.length === 1
            ? `templates/${widget.id}.html`
            : `templates/${widget.id}-${size}.html`
      }
      return { ...widget, templates }
    })
  }

  return next
}

function sanitizeFilename(id: string): string {
  const cleaned = id
    .trim()
    .replace(/[^\w.-]+/g, '_')
    .replace(/_+/g, '_')
    .replace(/^[_.]+|[_.]+$/g, '')
  return cleaned || 'tapp'
}

function decodeAssetPayload(value: string): Uint8Array | string {
  const dataUrlMatch = value.match(/^data:[^;]+;base64,(.+)$/s)
  if (dataUrlMatch) {
    return base64ToBytes(dataUrlMatch[1])
  }
  // Pure base64 (no data-URL prefix)
  if (/^[A-Za-z0-9+/=\s]+$/.test(value) && value.replace(/\s/g, '').length % 4 === 0) {
    try {
      return base64ToBytes(value.replace(/\s/g, ''))
    } catch {
      return value
    }
  }
  return value
}

function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes
}

function triggerDownload(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

/**
 * Build a .tapp ZIP from the current playground project and download it.
 */
export async function exportPlaygroundProjectAsTapp(
  project: TappPlaygroundProject,
): Promise<string> {
  const JSZip = (await import('jszip')).default
  const zip = new JSZip()
  const manifest = normalizeManifestForExport(project)
  const { code } = project

  zip.file('manifest.json', JSON.stringify(manifest, null, 2))
  zip.file(manifest.main, buildMainJs(code))

  if (code.styles) {
    const stylesPath = manifest.styles || 'styles.css'
    zip.file(stylesPath, code.styles)
  }

  if (code.pageHtml) {
    const pagePath = manifest.pageTemplate || 'page.html'
    zip.file(pagePath, code.pageHtml)
  }

  // Widget HTML templates (same mapping as buildDirectTappRequest)
  if (code.widgetHtml && manifest.widgets && manifest.widgets.length > 0) {
    for (const widget of manifest.widgets) {
      if (!widget.templates) continue
      for (const path of Object.values(widget.templates)) {
        if (path) zip.file(path, code.widgetHtml)
      }
    }
  }

  if (code.i18n && Object.keys(code.i18n).length > 0) {
    for (const [lang, data] of Object.entries(code.i18n)) {
      zip.file(
        `i18n/${lang}.json`,
        JSON.stringify(data, null, 2),
      )
    }
  }

  if (code.pageModules && Object.keys(code.pageModules).length > 0) {
    for (const [filename, content] of Object.entries(code.pageModules)) {
      zip.file(`page/${filename}`, content)
    }
  }

  if (code.assets && Object.keys(code.assets).length > 0) {
    for (const [path, value] of Object.entries(code.assets)) {
      const safePath = path.startsWith('assets/')
        ? path
        : `assets/${path.replace(/^\/+/, '')}`
      zip.file(safePath, decodeAssetPayload(value))
    }
  }

  const blob = await zip.generateAsync({
    type: 'blob',
    compression: 'DEFLATE',
    compressionOptions: { level: 6 },
  })
  const filename = `${sanitizeFilename(manifest.id)}.tapp`
  triggerDownload(blob, filename)
  return filename
}
