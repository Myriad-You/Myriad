/** ZIP 导出与 direct 安装共用同一份包文件图。 */

import type { TappPlaygroundCode } from '../services/TappPlaygroundService'
import type { TappCodeStructure, TappManifest } from '../types'
import { generateOnDemandTailwindCSS } from '../runtime/sandbox/styles'

export type PackageFileContent = string | Uint8Array

export interface PlaygroundPackageFiles {
  manifest: TappManifest
  files: Record<string, PackageFileContent>
}

export const PLAYGROUND_CORE_ENTRY = 'core.js'
export const PLAYGROUND_STYLES = 'styles.css'
export const PLAYGROUND_PAGE_ENTRY = 'page/index.js'
export const PLAYGROUND_PAGE_TEMPLATE = 'page.html'
export const PLAYGROUND_WIDGET_ENTRY = 'widget/index.js'

export function buildPlaygroundModules(
  code: TappPlaygroundCode,
): Record<string, string> {
  const modules: Record<string, string> = {
    [PLAYGROUND_CORE_ENTRY]: code.core || '',
  }
  if (code.page?.trim()) modules[PLAYGROUND_PAGE_ENTRY] = code.page
  if (code.widget?.trim()) modules[PLAYGROUND_WIDGET_ENTRY] = code.widget
  return modules
}

export function playgroundCodeToRuntime(
  manifest: Pick<TappManifest, 'widgets'>,
  code: TappPlaygroundCode,
): TappCodeStructure {
  const modules = buildPlaygroundModules(code)
  const widgetEntries =
    Object.hasOwn(modules, PLAYGROUND_WIDGET_ENTRY)
      ? Object.fromEntries(
          (manifest.widgets || []).map((widget) => [
            widget.id,
            PLAYGROUND_WIDGET_ENTRY,
          ]),
        )
      : {}
  return {
    modules,
    coreEntry: PLAYGROUND_CORE_ENTRY,
    pageEntry: Object.hasOwn(modules, PLAYGROUND_PAGE_ENTRY)
      ? PLAYGROUND_PAGE_ENTRY
      : undefined,
    widgetEntries:
      Object.keys(widgetEntries).length > 0 ? widgetEntries : undefined,
    styles: code.styles,
    widgetHtml: code.widgetHtml,
    pageHtml: code.pageHtml,
    widgetCSS:
      code.widgetCSS ||
      generateOnDemandTailwindCSS(
        [
          code.widgetHtml || '',
          code.styles || '',
          code.core || '',
          code.widget || '',
        ].join('\n'),
      ),
    pageCSS:
      code.pageCSS ||
      generateOnDemandTailwindCSS(
        [
          code.pageHtml || '',
          code.styles || '',
          code.core || '',
          code.page || '',
        ].join('\n'),
      ),
    i18n: code.i18n,
    assets: code.assets,
  }
}

export function normalizeManifestForPackage(
  manifest: TappManifest,
  code: TappPlaygroundCode,
): TappManifest {
  const next: TappManifest = { ...manifest }

  next.core = {
    entry: PLAYGROUND_CORE_ENTRY,
    ...(code.styles ? { styles: PLAYGROUND_STYLES } : {}),
  }

  const hasUsablePageHtml = !!(code.pageHtml && code.pageHtml.trim())
  const hasPageCode = !!(code.page && code.page.trim())
  if (hasUsablePageHtml || hasPageCode) {
    next.page = {
      ...(hasPageCode ? { entry: PLAYGROUND_PAGE_ENTRY } : {}),
      ...(hasUsablePageHtml ? { template: PLAYGROUND_PAGE_TEMPLATE } : {}),
    }
  } else {
    delete next.page
  }

  if (code.widget?.trim() && next.widgets && next.widgets.length > 0) {
    next.widgets = next.widgets.map((widget) => ({
      ...widget,
      entry: PLAYGROUND_WIDGET_ENTRY,
    }))
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
    : `assets/${path.replaceAll(/^\/+/g, '')}`
}

export function decodeAssetPayload(value: string): PackageFileContent {
  const dataUrlMatch = value.match(/^data:[^;]+;base64,(.+)$/s)
  if (dataUrlMatch) {
    return base64ToBytes(dataUrlMatch[1])
  }
  if (
    /^[A-Z0-9+/=\s]+$/i.test(value) &&
    value.replaceAll(/\s/g, '').length % 4 === 0
  ) {
    try {
      return base64ToBytes(value.replaceAll(/\s/g, ''))
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

/** 固定三文件层入口：core.js / page/index.js / widget/index.js。 */
export function buildPlaygroundPackageFiles(
  manifest: TappManifest,
  code: TappPlaygroundCode,
): PlaygroundPackageFiles {
  const normalized = normalizeManifestForPackage(manifest, code)
  const files: Record<string, PackageFileContent> = {}

  files['manifest.json'] = JSON.stringify(normalized, null, 2)
  for (const [path, source] of Object.entries(buildPlaygroundModules(code))) {
    files[path] = source
  }

  if (code.styles) {
    files[PLAYGROUND_STYLES] = code.styles
  }

  if (code.pageHtml && code.pageHtml.trim()) {
    files[PLAYGROUND_PAGE_TEMPLATE] = code.pageHtml
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

  if (code.assets && Object.keys(code.assets).length > 0) {
    for (const [path, value] of Object.entries(code.assets)) {
      files[assetPackagePath(path)] = decodeAssetPayload(value)
    }
  }

  return { manifest: normalized, files }
}

export function packageFilesToDirectInstallBody(
  pkg: PlaygroundPackageFiles,
  originalAssets?: Record<string, string>,
): {
  manifest: TappManifest
  modules: Record<string, string>
  coreStyles?: string
  pageTemplate?: string
  widgetTemplates?: Record<string, Record<string, string>>
  i18n?: Record<string, unknown>
  assets?: Record<string, string>
} {
  const { manifest, files } = pkg

  const readText = (path: string | undefined): string | undefined => {
    if (!path) return undefined
    const raw = files[path]
    if (raw === undefined) return undefined
    return typeof raw === 'string' ? raw : new TextDecoder().decode(raw)
  }

  // 层入口及其依赖：包里的 .js 一并送上，安装时按 Manifest 层声明校验。
  const modules: Record<string, string> = {}
  for (const [path, content] of Object.entries(files)) {
    if (!path.endsWith('.js')) continue
    modules[path] =
      typeof content === 'string' ? content : new TextDecoder().decode(content)
  }

  const coreStyles = readText(manifest.core?.styles)
  const pageTemplate = readText(manifest.page?.template)

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
    }
  }

  const assets =
    originalAssets && Object.keys(originalAssets).length > 0
      ? originalAssets
      : undefined

  return {
    manifest,
    modules,
    coreStyles,
    pageTemplate,
    widgetTemplates,
    i18n,
    assets,
  }
}
