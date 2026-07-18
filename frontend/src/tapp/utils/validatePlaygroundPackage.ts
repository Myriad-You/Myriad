/**
 * Client preflight for Playground .tapp export and install-from-playground.
 *
 * Mirrors production install checks as closely as practical without a server
 * dry-run: manifest field rules from `validate_tapp_manifest`, resource
 * presence from `validate_installed_resources`, asset path rules from
 * `validate_asset_path`, plus playground project constraints that affect
 * whether the built package would stage cleanly.
 */

import type { TappCodeStructure, TappManifest, WidgetSize } from '../types'
import type { PackageFileContent, PlaygroundPackageFiles } from './playgroundPackageFiles.ts'
import {
  buildPlaygroundPackageFiles,

} from './playgroundPackageFiles.ts'

const MAX_TAPP_ID_LEN = 128
const MAX_RESOURCE_PATH_LEN = 256
const MAX_TAPP_ASSETS = 64
const MAX_WIDGETS_PER_TAPP = 64

const VALID_WIDGET_SIZES = new Set<string>([
  '1x1',
  '1x2',
  '2x1',
  '2x2',
  '2x3',
  '3x2',
  '4x1',
  '4x2',
  '2x4',
  '3x3',
  '4x4',
])

/** Loose semver matching the backend `semver::Version::parse` happy path. */
const SEMVER_RE =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-z-][0-9a-z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-z-][0-9a-z-]*))*))?(?:\+([0-9a-z-]+(?:\.[0-9a-z-]+)*))?$/i

export type ValidatePlaygroundPackageResult =
  | { ok: true; package: PlaygroundPackageFiles }
  | { ok: false; errors: string[] }

export interface ValidatePlaygroundPackageInput {
  manifest: TappManifest
  code: TappCodeStructure
}

function isSafePathComponent(value: string): boolean {
  if (!value || value.length > MAX_TAPP_ID_LEN) return false
  if (value === '.' || value === '..' || value.startsWith('.')) return false
  return /^[\w.-]+$/.test(value)
}

function validateTappId(id: string): string | null {
  if (
    !id ||
    id.length > MAX_TAPP_ID_LEN ||
    !/^[A-Z0-9]/i.test(id) ||
    !isSafePathComponent(id)
  ) {
    return 'Invalid Tapp id: use 1-128 ASCII letters, numbers, dots, underscores, or hyphens'
  }
  return null
}

function validateResourcePath(path: string): string | null {
  if (
    !path ||
    path.length > MAX_RESOURCE_PATH_LEN ||
    path.includes('\\') ||
    path.startsWith('/')
  ) {
    return `Invalid Tapp resource path: ${path}`
  }
  const parts = path.split('/')
  if (parts.length === 0 || parts.some((part) => !isSafePathComponent(part))) {
    return `Invalid Tapp resource path: ${path}`
  }
  return null
}

function validateResourceExtension(
  path: string,
  extension: string,
  field: string,
): string | null {
  if (!path.endsWith(extension)) {
    return `Tapp ${field} must reference a ${extension} file`
  }
  return null
}

/** Mirrors backend `validate_asset_path`. */
export function validateAssetPath(path: string): string | null {
  const pathError = validateResourcePath(path)
  if (pathError) return pathError
  if (!path.startsWith('assets/') || path === 'assets' || path.endsWith('/')) {
    return `Tapp asset path must be a file under assets/: ${path}`
  }
  if (path.endsWith('.js') || path.endsWith('.html')) {
    return `Tapp asset path must not be a script or HTML entry: ${path}`
  }
  return null
}

function fileExists(
  files: Record<string, PackageFileContent>,
  path: string,
): boolean {
  return Object.hasOwn(files, path)
}

function isNonEmptyText(value: PackageFileContent | undefined): boolean {
  if (value === undefined) return false
  if (typeof value === 'string') return value.trim().length > 0
  return value.byteLength > 0
}

/**
 * Validate that a playground project builds an installable package map.
 * Runs after path normalization via `buildPlaygroundPackageFiles`.
 */
export function validatePlaygroundPackage(
  project: ValidatePlaygroundPackageInput,
): ValidatePlaygroundPackageResult {
  const errors: string[] = []
  const push = (message: string) => {
    if (!errors.includes(message)) errors.push(message)
  }

  const pkg = buildPlaygroundPackageFiles(project.manifest, project.code)
  const { manifest, files } = pkg
  const code = project.code
  // Mode checks use the author-declared manifest so normalize (which may clear
  // hasPage when pageHtml is empty) does not hide "hasPage without page" errors.
  const declared = project.manifest

  // --- Manifest required fields (validate_tapp_manifest core) ---
  const idError = validateTappId(manifest.id || '')
  if (idError) push(idError)

  if (!manifest.name || !manifest.name.trim() || manifest.name.length > 255) {
    push('Tapp name must contain 1-255 characters')
  }

  if (!manifest.version || !SEMVER_RE.test(manifest.version)) {
    push('Tapp version must be valid semantic version')
  }

  if (!manifest.category) {
    push('Tapp category is required')
  }

  if (!manifest.main || !manifest.main.trim()) {
    push('Tapp main entry is required')
  } else {
    const mainPathError = validateResourcePath(manifest.main)
    if (mainPathError) push(mainPathError)
    const mainExtError = validateResourceExtension(manifest.main, '.js', 'main')
    if (mainExtError) push(mainExtError)
  }

  // Playground install/export expectations (validate_playground_project dual-mode)
  if (manifest.main !== 'main.js') {
    push('Playground requires main entry main.js')
  }
  if (manifest.styles !== 'styles.css') {
    push('Playground requires styles.css')
  }
  if (manifest.cssMode && manifest.cssMode !== 'unified') {
    push('Playground requires unified CSS mode')
  }

  const pageCode = code.page ?? ''
  const pageHtml = code.pageHtml ?? ''
  const widgetsForMode = declared.widgets ?? manifest.widgets ?? []
  const hasWidgets = widgetsForMode.length > 0
  const hasPage = declared.hasPage === true

  // Dual mode: Page and/or Widget-only. Reject empty projects (neither).
  if (!hasPage && !hasWidgets) {
    push('Playground project requires a Page (hasPage) and/or non-empty Widgets')
  }

  if (hasPage) {
    const pageTemplate =
      declared.pageTemplate ?? manifest.pageTemplate ?? undefined
    if (pageTemplate !== 'page.html') {
      push('Playground Page mode requires pageTemplate: page.html')
    }
    if (!pageCode.trim() || !pageHtml.trim()) {
      push(
        'Playground project requires non-empty page code and HTML when hasPage is true',
      )
    }
  } else {
    const pageTemplate =
      declared.pageTemplate ?? manifest.pageTemplate ?? undefined
    if (pageTemplate && pageTemplate !== 'page.html') {
      push('Playground pageTemplate must be page.html when declared')
    }
  }

  if (!hasPage && hasWidgets) {
    if (!code.widget?.trim() || !code.widgetHtml?.trim()) {
      push(
        'Widget-only Playground projects require non-empty code.widget and code.widgetHtml',
      )
    }
  }

  // Optional path fields on normalized manifest
  for (const [field, path, extension] of [
    ['styles', manifest.styles, '.css'],
    ['widgetStyles', manifest.widgetStyles, '.css'],
    ['pageStyles', manifest.pageStyles, '.css'],
    ['pageTemplate', manifest.pageTemplate, '.html'],
  ] as const) {
    if (!path) continue
    const pathError = validateResourcePath(path)
    if (pathError) push(pathError)
    const extError = validateResourceExtension(path, extension, field)
    if (extError) push(extError)
  }

  // pageModules filenames
  if (manifest.pageModules) {
    if (manifest.pageModules.length > 64) {
      push('Tapp pageModules accepts at most 64 entries')
    }
    const seen = new Set<string>()
    for (const module of manifest.pageModules) {
      if (
        !isSafePathComponent(module) ||
        !module.endsWith('.js') ||
        seen.has(module)
      ) {
        push(
          `Invalid or duplicate page module filename: ${module}; expected a .js file relative to page/`,
        )
      }
      seen.add(module)
    }
  }

  // Assets list on manifest — full validate_asset_path
  if (manifest.assets) {
    if (manifest.assets.length > MAX_TAPP_ASSETS) {
      push(`Tapp assets accepts at most ${MAX_TAPP_ASSETS} entries`)
    }
    const seen = new Set<string>()
    for (const path of manifest.assets) {
      const assetError = validateAssetPath(path)
      if (assetError) push(assetError)
      if (seen.has(path)) {
        push(`Duplicate Tapp asset path: ${path}`)
      }
      seen.add(path)
    }
  }

  // Widgets
  const widgets = manifest.widgets ?? []
  if (widgets.length > 0) {
    if (!manifest.permissions?.includes('widget:register')) {
      push('Tapp widgets require widget:register permission')
    }
    if (widgets.length > MAX_WIDGETS_PER_TAPP) {
      push(`Too many Widgets (max ${MAX_WIDGETS_PER_TAPP})`)
    }

    const widgetIds = new Set<string>()
    for (const widget of widgets) {
      if (!isSafePathComponent(widget.id) || widgetIds.has(widget.id)) {
        push(`Invalid or duplicate Widget ID: ${widget.id}`)
      }
      widgetIds.add(widget.id)

      if (!widget.name || widget.name.length > 255) {
        push(`Invalid Widget name: ${widget.id}`)
      }

      const sizes = widget.sizes ?? []
      if (
        sizes.length === 0 ||
        sizes.length > 10 ||
        sizes.some((size) => !VALID_WIDGET_SIZES.has(size)) ||
        !sizes.includes(widget.defaultSize as WidgetSize)
      ) {
        push(`Invalid Widget sizes: ${widget.id}`)
      }

      if (widget.templates) {
        for (const [size, path] of Object.entries(widget.templates)) {
          if (
            !VALID_WIDGET_SIZES.has(size) ||
            !sizes.includes(size as WidgetSize)
          ) {
            push(
              `Widget template uses an undeclared size ${size}: ${widget.id}`,
            )
          }
          if (path) {
            const pathError = validateResourcePath(path)
            if (pathError) push(pathError)
            const extError = validateResourceExtension(
              path,
              '.html',
              'Widget template',
            )
            if (extError) push(extError)
          }
        }
      }
    }

    // Export writes widget HTML only when code.widgetHtml is present.
    if (!code.widget?.trim() || !code.widgetHtml?.trim()) {
      push(
        'Manifest Widgets require non-empty code.widget and code.widgetHtml',
      )
    }
  }

  // --- Declared resources must exist in the built file map ---
  const requiredPaths: string[] = []
  if (manifest.main) requiredPaths.push(manifest.main)
  if (manifest.styles) requiredPaths.push(manifest.styles)
  if (manifest.widgetStyles) requiredPaths.push(manifest.widgetStyles)
  if (manifest.pageStyles) requiredPaths.push(manifest.pageStyles)
  if (manifest.pageTemplate) requiredPaths.push(manifest.pageTemplate)

  for (const widget of widgets) {
    if (!widget.templates) continue
    for (const path of Object.values(widget.templates)) {
      if (path) requiredPaths.push(path)
    }
  }

  for (const relative of requiredPaths) {
    if (!fileExists(files, relative)) {
      push(`Declared Tapp resource not found: ${relative}`)
      continue
    }
    if (relative.endsWith('.html') && !isNonEmptyText(files[relative])) {
      push(`Declared Tapp resource is empty: ${relative}`)
    }
  }

  if (manifest.pageModules) {
    for (const module of manifest.pageModules) {
      const relative = `page/${module}`
      if (!fileExists(files, relative)) {
        push(`Declared Tapp resource not found: ${relative}`)
      }
    }
  }

  if (manifest.assets) {
    for (const relative of manifest.assets) {
      // Skip presence check when path shape is already invalid.
      if (validateAssetPath(relative)) continue
      if (!fileExists(files, relative)) {
        push(`Declared Tapp asset not found: ${relative}`)
      }
    }
  }

  // Widgets with code must also emit template files (export writes them).
  if (widgets.length > 0 && code.widgetHtml?.trim()) {
    for (const widget of widgets) {
      const templates = widget.templates
      if (!templates || Object.keys(templates).length === 0) {
        push(
          `Widget ${widget.id} is missing HTML template paths for export/install`,
        )
        continue
      }
      for (const [size, path] of Object.entries(templates)) {
        if (!path || !fileExists(files, path)) {
          push(
            `Widget ${widget.id} template for size ${size} not found: ${path || '(empty path)'}`,
          )
        }
      }
    }
  }

  // Ensure main.js was produced with some source
  if (manifest.main && fileExists(files, manifest.main)) {
    const main = files[manifest.main]
    if (
      typeof main === 'string' &&
      main.trim().length === 0 &&
      !code.core?.trim()
    ) {
      push('Tapp main entry is empty (missing core/page/widget code)')
    }
  }

  if (errors.length > 0) {
    return { ok: false, errors }
  }
  return { ok: true, package: pkg }
}

/**
 * Format validation errors for throw / error UI (English, admin playground).
 */
export function formatPlaygroundPackageErrors(errors: string[]): string {
  if (errors.length === 1) return errors[0]
  return errors.map((error, index) => `${index + 1}. ${error}`).join('\n')
}

/** Thrown by export (and usable by install) when preflight fails. */
export class PlaygroundPackageValidationError extends Error {
  readonly errors: string[]

  constructor(errors: string[]) {
    super(formatPlaygroundPackageErrors(errors))
    this.name = 'PlaygroundPackageValidationError'
    this.errors = errors
  }
}
