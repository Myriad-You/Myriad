/**
 * Shared pure package-file map for Playground .tapp export and direct install.
 *
 * Both `exportPlaygroundTapp` (ZIP) and `buildDirectTappRequest` (install JSON)
 * must derive file paths and main.js layout from this builder so export →
 * install-file and install-from-code stay consistent.
 */

import type { TappCodeStructure, TappManifest } from '../types'

/** Package entry content: text files as string; binary assets as Uint8Array. */
export type PackageFileContent = string | Uint8Array

export interface PlaygroundPackageFiles {
  /** Manifest with default paths filled so every entry has a declared path. */
  manifest: TappManifest
  /** Relative package-root path → content (layout of a .tapp ZIP). */
  files: Record<string, PackageFileContent>
}

/**
 * Build main.js from structured playground code.
 * With pageModules: core + optional widget (page lives under page/).
 * Without: core + optional widget + optional page (monolithic merge).
 * Always includes core so headless extractCoreCode works after reinstall.
 */
export function buildPlaygroundMainJs(code: TappCodeStructure): string {
  const hasPageModules =
    !!code.pageModules && Object.keys(code.pageModules).length > 0
  if (hasPageModules) {
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
 * Fill default resource paths on the manifest so package entries and install
 * staging write to the same locations.
 */
export function normalizeManifestForPackage(
  manifest: TappManifest,
  code: TappCodeStructure,
): TappManifest {
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

  const hasUsablePageHtml = !!(code.pageHtml && code.pageHtml.trim())
  if (hasUsablePageHtml && !next.pageTemplate) {
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
  } else if (hasUsablePageHtml) {
    next.hasPage = true
  } else {
    // Widget-only / no page content: do not invent page.html or force hasPage.
    next.hasPage = false
    if (!hasUsablePageHtml) {
      delete next.pageTemplate
    }
  }

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

function assetPackagePath(path: string): string {
  return path.startsWith('assets/')
    ? path
    : `assets/${path.replace(/^\/+/, '')}`
}

/**
 * Decode data-URL / raw base64 asset payloads for ZIP binary entries.
 * Non-base64 strings are returned as-is (e.g. JSON asset text).
 */
export function decodeAssetPayload(value: string): PackageFileContent {
  const dataUrlMatch = value.match(/^data:[^;]+;base64,(.+)$/s)
  if (dataUrlMatch) {
    return base64ToBytes(dataUrlMatch[1])
  }
  if (
    /^[A-Z0-9+/=\s]+$/i.test(value) &&
    value.replace(/\s/g, '').length % 4 === 0
  ) {
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

/**
 * Pure builder: package paths and contents shared by ZIP export and direct install.
 *
 * Layout keys (when present in code/manifest):
 * - manifest.json
 * - {manifest.main} (main.js)
 * - {manifest.styles}
 * - {manifest.pageTemplate}
 * - widget template paths from manifest.widgets[].templates
 * - i18n/{lang}.json
 * - page/{filename}
 * - assets/...
 */
export function buildPlaygroundPackageFiles(
  manifest: TappManifest,
  code: TappCodeStructure,
): PlaygroundPackageFiles {
  const normalized = normalizeManifestForPackage(manifest, code)
  const files: Record<string, PackageFileContent> = {}

  files['manifest.json'] = JSON.stringify(normalized, null, 2)
  files[normalized.main] = buildPlaygroundMainJs(code)

  if (code.styles) {
    files[normalized.styles || 'styles.css'] = code.styles
  }

  // Omit page.html for widget-only packages (no usable pageHtml).
  if (code.pageHtml && code.pageHtml.trim()) {
    files[normalized.pageTemplate || 'page.html'] = code.pageHtml
  }

  if (code.widgetHtml && normalized.widgets && normalized.widgets.length > 0) {
    for (const widget of normalized.widgets) {
      if (!widget.templates) continue
      for (const path of Object.values(widget.templates)) {
        if (path) files[path] = code.widgetHtml
      }
    }
  }

  if (code.i18n && Object.keys(code.i18n).length > 0) {
    for (const [lang, data] of Object.entries(code.i18n)) {
      files[`i18n/${lang}.json`] = JSON.stringify(data, null, 2)
    }
  }

  if (code.pageModules && Object.keys(code.pageModules).length > 0) {
    for (const [filename, content] of Object.entries(code.pageModules)) {
      files[`page/${filename}`] = content
    }
  }

  if (code.assets && Object.keys(code.assets).length > 0) {
    for (const [path, value] of Object.entries(code.assets)) {
      files[assetPackagePath(path)] = decodeAssetPayload(value)
    }
  }

  return { manifest: normalized, files }
}

/**
 * Map package files into direct-install API fields (source=direct JSON body).
 * Does not include permissions or generated widgetCss/pageCss — callers add those.
 */
export function packageFilesToDirectInstallBody(
  pkg: PlaygroundPackageFiles,
  originalAssets?: Record<string, string>,
): {
  manifest: TappManifest
  code: string
  styles?: string
  pageTemplate?: string
  widgetTemplates?: Record<string, Record<string, string>>
  i18n?: Record<string, unknown>
  pageModules?: Record<string, string>
  assets?: Record<string, string>
} {
  const { manifest, files } = pkg
  const mainContent = files[manifest.main]
  const code =
    typeof mainContent === 'string'
      ? mainContent
      : new TextDecoder().decode(mainContent)

  const stylesPath = manifest.styles || 'styles.css'
  const stylesRaw = files[stylesPath]
  const styles =
    stylesRaw !== undefined
      ? typeof stylesRaw === 'string'
        ? stylesRaw
        : new TextDecoder().decode(stylesRaw)
      : undefined

  const pagePath = manifest.pageTemplate || 'page.html'
  const pageRaw = files[pagePath]
  const pageTemplate =
    pageRaw !== undefined
      ? typeof pageRaw === 'string'
        ? pageRaw
        : new TextDecoder().decode(pageRaw)
      : undefined

  let widgetTemplates: Record<string, Record<string, string>> | undefined
  if (manifest.widgets && manifest.widgets.length > 0) {
    const templates: Record<string, Record<string, string>> = {}
    for (const widget of manifest.widgets) {
      if (!widget.templates) continue
      const widgetTemplatesForId: Record<string, string> = {}
      for (const [size, path] of Object.entries(widget.templates)) {
        const content = files[path]
        if (content === undefined) continue
        widgetTemplatesForId[size] =
          typeof content === 'string'
            ? content
            : new TextDecoder().decode(content)
      }
      if (Object.keys(widgetTemplatesForId).length > 0) {
        templates[widget.id] = widgetTemplatesForId
      }
    }
    if (Object.keys(templates).length > 0) {
      widgetTemplates = templates
    }
  }

  let i18n: Record<string, unknown> | undefined
  for (const [path, content] of Object.entries(files)) {
    const match = path.match(/^i18n\/(.+)\.json$/)
    if (!match) continue
    const text =
      typeof content === 'string' ? content : new TextDecoder().decode(content)
    try {
      i18n = i18n || {}
      i18n[match[1]] = JSON.parse(text) as unknown
    } catch {
      // Skip invalid i18n JSON; install validation will surface issues.
    }
  }

  let pageModules: Record<string, string> | undefined
  for (const [path, content] of Object.entries(files)) {
    if (!path.startsWith('page/')) continue
    const filename = path.slice('page/'.length)
    if (!filename || filename.includes('/')) continue
    pageModules = pageModules || {}
    pageModules[filename] =
      typeof content === 'string' ? content : new TextDecoder().decode(content)
  }

  // Prefer original base64/data-URL asset map for the install API shape.
  const assets =
    originalAssets && Object.keys(originalAssets).length > 0
      ? originalAssets
      : undefined

  return {
    manifest,
    code,
    styles,
    pageTemplate,
    widgetTemplates,
    i18n,
    pageModules,
    assets,
  }
}
