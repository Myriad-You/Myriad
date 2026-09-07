/**
 * Client preflight for Playground .tapp export and install-from-playground.
 *
 * Mirrors production install checks as closely as practical without a server
 * dry-run: manifest field rules from `validate_tapp_manifest`, resource
 * presence from `validate_installed_resources`, asset path rules from
 * `validate_asset_path`, plus playground project constraints that affect
 * whether the built package would stage cleanly.
 */

import type { TappPlaygroundCode } from '../services/TappPlaygroundService'
import type { TappManifest, WidgetSize } from '../types'
import type { PackageFileContent, PlaygroundPackageFiles } from './playgroundPackageFiles.ts'
import {
  buildPlaygroundPackageFiles,

} from './playgroundPackageFiles.ts'
import { maxDeclaredAssets } from './tappPackageLimits.ts'

const MAX_TAPP_ID_LEN = 128
const MAX_RESOURCE_PATH_LEN = 256
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

/** Loose BCP-47 tag matching backend `valid_locale_tag`: language 2-3 letters + alnum subtags. */
const LOCALE_TAG_RE = /^[a-z]{2,3}(-[a-z0-9]{1,8})*$/i

/** Loose semver matching the backend `semver::Version::parse` happy path. */
const SEMVER_RE =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-z-][0-9a-z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-z-][0-9a-z-]*))*))?(?:\+([0-9a-z-]+(?:\.[0-9a-z-]+)*))?$/i

export type ValidatePlaygroundPackageResult =
  | { ok: true; package: PlaygroundPackageFiles }
  | { ok: false; errors: string[] }

export interface ValidatePlaygroundPackageInput {
  manifest: TappManifest
  code: TappPlaygroundCode
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

/** Tailwind `sm:`/`md:`/`lg:` prefixes. `text-sm` / `rounded-md` are allowed. */
export function unsupportedTailwindBreakpoint(source: string): string | null {
  const prefixes = ['sm:', 'md:', 'lg:', 'xl:', '2xl:'] as const
  for (const prefix of prefixes) {
    let rest = source
    while (rest.includes(prefix)) {
      const position = rest.indexOf(prefix)
      const after = rest.charAt(position + prefix.length)
      if (/[A-Za-z]/.test(after) || after === '[') return prefix
      rest = rest.slice(position + prefix.length)
    }
  }
  return null
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
  // declared page when pageHtml is empty) does not hide "page without content" errors.
  const declared = project.manifest

  // Manifest required fields (validate_tapp_manifest core)
  const idError = validateTappId(manifest.id || '')
  if (idError) push(idError)

  if (!manifest.name || !manifest.name.trim() || manifest.name.length > 255) {
    push('Tapp name must contain 1-255 characters')
  }

  if (manifest.locales) {
    const entries = Object.entries(manifest.locales)
    if (entries.length > 32) {
      push('Tapp locales must not declare more than 32 languages')
    }
    for (const [tag, entry] of entries) {
      if (!LOCALE_TAG_RE.test(tag)) {
        push(
          `Tapp locales key '${tag}' must be a BCP-47 language tag (e.g. zh-CN)`,
        )
      }
      if (
        entry.name !== undefined &&
        (!entry.name.trim() || entry.name.length > 255)
      ) {
        push(`Tapp locales['${tag}'].name must contain 1-255 characters`)
      }
      if (entry.description !== undefined && entry.description.length > 2000) {
        push(
          `Tapp locales['${tag}'].description must not exceed 2000 characters`,
        )
      }
    }
  }

  if (!manifest.version || !SEMVER_RE.test(manifest.version)) {
    push('Tapp version must be valid semantic version')
  }

  if (!manifest.category) {
    push('Tapp category is required')
  }

  // 层入口与层资源（validate_tapp_layers）
  const layerPaths: Array<[string, string | undefined, string]> = [
    ['core.entry', manifest.core?.entry, '.js'],
    ['core.styles', manifest.core?.styles, '.css'],
    ['page.entry', manifest.page?.entry, '.js'],
    ['page.template', manifest.page?.template, '.html'],
    ['page.styles', manifest.page?.styles, '.css'],
  ]
  for (const widget of manifest.widgets ?? []) {
    layerPaths.push(['widgets[].entry', widget.entry, '.js'])
    layerPaths.push(['widgets[].styles', widget.styles, '.css'])
  }
  for (const [field, path, extension] of layerPaths) {
    if (!path) continue
    const pathError = validateResourcePath(path)
    if (pathError) push(pathError)
    const extError = validateResourceExtension(path, extension, field)
    if (extError) push(extError)
  }

  const pageCode = code.page ?? ''
  const pageHtml = code.pageHtml ?? ''
  const widgetsForMode = declared.widgets ?? manifest.widgets ?? []
  const hasWidgets = widgetsForMode.length > 0
  // 用作者声明判断模式：normalize 会在内容为空时清掉 page 层，
  // 只看 normalize 结果就分不出「没打算要页面」和「声明了却没写内容」。
  const hasPage = declared.page !== undefined || manifest.page !== undefined

  const declaredBackground =
    (declared.backgroundRequirements?.length ?? 0) > 0 ||
    (manifest.backgroundRequirements?.length ?? 0) > 0
  if (declaredBackground && !declared.core?.entry && !code.core?.trim()) {
    push('Tapp declaring backgroundRequirements must declare a core layer')
  }

  // Dual mode: Page and/or Widget-only. Reject empty projects (neither).
  if (!hasPage && !hasWidgets) {
    push('Playground project requires a page layer and/or non-empty Widgets')
  }

  // Playground install/export expectations (fixed three-file layout)
  if (manifest.core && manifest.core.entry !== 'core.js') {
    push('Playground requires core entry core.js')
  }
  if (code.styles && manifest.core?.styles !== 'styles.css') {
    push('Playground requires styles.css')
  }
  if (hasPage) {
    if (!pageCode.trim() || !pageHtml.trim()) {
      push(
        'Playground project requires non-empty page code and HTML when a page layer is declared',
      )
    } else if (manifest.page?.template !== 'page.html') {
      push('Playground page layer requires template page.html')
    }
  }

  if (!hasPage && hasWidgets) {
    if (!code.widget?.trim() || !code.widgetHtml?.trim()) {
      push(
        'Widget-only Playground projects require non-empty code.widget and code.widgetHtml',
      )
    }
  }

  // Assets list on manifest — full validate_asset_path
  if (manifest.assets) {
    const maxAssets = maxDeclaredAssets(manifest)
    if (manifest.assets.length > maxAssets) {
      push(`Tapp assets accepts at most ${maxAssets} entries`)
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

  // Declared layer resources must exist in the built file map
  const requiredPaths: string[] = []
  for (const path of [
    manifest.core?.entry,
    manifest.core?.styles,
    manifest.page?.entry,
    manifest.page?.template,
    manifest.page?.styles,
  ]) {
    if (path) requiredPaths.push(path)
  }

  for (const widget of widgets) {
    if (widget.entry) requiredPaths.push(widget.entry)
    if (widget.styles) requiredPaths.push(widget.styles)
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

  // 安装契约只在后台常驻时强制 core。空 core.js 对 Page/Widget 预览无害。
  const coreEntry = manifest.core?.entry
  if (declaredBackground && coreEntry && fileExists(files, coreEntry)) {
    const source = files[coreEntry]
    if (typeof source === 'string' && source.trim().length === 0) {
      push('Tapp declaring backgroundRequirements must declare a core layer')
    }
  }

  const source = [
    code.core,
    code.page,
    code.widget,
    code.widgetHtml,
    code.pageHtml,
  ]
    .filter((value): value is string => typeof value === 'string')
    .join('\n')
  const widgetSource = [code.widget, code.widgetHtml]
    .filter((value): value is string => typeof value === 'string')
    .join('\n')

  const calls = (method: string, from = source): boolean => {
    let rest = from
    while (rest.includes(method)) {
      const position = rest.indexOf(method)
      const next = rest.charAt(position + method.length)
      if (!next || !/[A-Za-z0-9]/.test(next)) return true
      rest = rest.slice(position + 1)
    }
    return false
  }

  if (calls('Tapp.ai') && !manifest.ai) {
    push(
      'Code calls Tapp.ai but manifest.ai is missing (protocolVersion 2, operations, outputFormats)',
    )
  }
  if (calls('Tapp.game') && !manifest.game) {
    push('Code calls Tapp.game but manifest.game is missing')
  }
  if (
    (source.includes('Tapp.api(') || source.includes('Tapp.api (')) &&
    (!manifest.apis || Object.keys(manifest.apis).length === 0)
  ) {
    push('Code calls Tapp.api() but manifest.apis is missing')
  }
  if (
    (calls('Tapp.ui.openUrl') || calls('Tapp.ui.listOpenUrls')) &&
    (!manifest.openUrls || manifest.openUrls.length === 0)
  ) {
    push('Code calls Tapp.ui.openUrl but manifest.openUrls is missing')
  }
  if (widgets.length > 0) {
    if (!source.includes('Tapp.widgets')) {
      push(
        'Widget projects must assign Tapp.widgets[<id>] = { render } so the host can paint the card',
      )
    }
    const widgetPageOnly: Array<[string, string]> = [
      [
        'Tapp.widget.register',
        'Tapp.widget.register is Page-only; Widget code must assign Tapp.widgets[id] = { render }',
      ],
      ['Tapp.ui.confirm', 'Tapp.ui.confirm is not on the Widget SDK'],
      ['Tapp.ui.setTitle', 'Tapp.ui.setTitle is not on the Widget SDK'],
      [
        'Tapp.ui.requestFullscreen',
        'Tapp.ui.requestFullscreen is not on the Widget SDK',
      ],
      [
        'Tapp.ui.exitFullscreen',
        'Tapp.ui.exitFullscreen is not on the Widget SDK',
      ],
      ['Tapp.ui.fullscreen', 'Tapp.ui.fullscreen is not on the Widget SDK'],
      ['Tapp.game', 'Tapp.game is not on the Widget SDK'],
      ['Tapp.federation', 'Tapp.federation is not on the Widget SDK'],
      ['Tapp.tappList', 'Tapp.tappList is not on the Widget SDK'],
      ['Tapp.brewList', 'Tapp.brewList is not on the Widget SDK'],
      ['Tapp.component', 'Tapp.component is not on the Widget SDK'],
      ['Tapp.shortcut', 'Tapp.shortcut is not on the Widget SDK'],
      ['Tapp.dynamicContent', 'Tapp.dynamicContent is not on the Widget SDK'],
    ]
    for (const [method, message] of widgetPageOnly) {
      if (calls(method, widgetSource)) push(message)
    }
  }

  for (const [field, text] of [
    ['page', code.page],
    ['pageHtml', code.pageHtml],
    ['widget', code.widget],
    ['widgetHtml', code.widgetHtml],
    ['core', code.core],
  ] as const) {
    if (!text) continue
    const prefix = unsupportedTailwindBreakpoint(text)
    if (prefix) {
      push(
        `${field} uses unsupported Tailwind breakpoint \`${prefix}\` (sandbox on-demand compile ignores sm:/md:/lg:; use CSS media queries or container width)`,
      )
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
