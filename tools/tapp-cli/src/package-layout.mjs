/**
 * Package path layout shared with Playground export / direct-install.
 * Mirrors frontend/src/tapp/utils/playgroundPackageFiles.ts path rules for
 * disk-based CLI projects (content still comes from the filesystem).
 */

export const PACKAGE_MARKERS = {
  widget: '// ========== Widget Code ==========',
  page: '// ========== Page Code ==========',
}

export function assetPackagePath(path) {
  return path.startsWith('assets/') ? path : `assets/${path.replace(/^\/+/, '')}`
}

/**
 * Normalize Manifest defaults the same way Playground packaging does when
 * only on-disk files are available (no in-memory code structure).
 */
export function normalizeManifestPaths(manifest) {
  const next = { ...manifest }
  if (!next.main || !String(next.main).trim()) next.main = 'main.js'
  if (!next.cssMode) next.cssMode = 'unified'
  if (next.hasPage === true && !next.pageTemplate && !(next.pageModules && next.pageModules.length)) {
    next.pageTemplate = 'page.html'
  }
  if (Array.isArray(next.pageModules) && next.pageModules.length > 0) {
    next.hasPage = true
  }
  return next
}

/**
 * Expected relative package paths for a disk project after validation.
 * @param {object} options
 * @param {object} options.manifest
 * @param {string[]} [options.extraPaths] existing files already collected
 */
export function expectedPackagePaths({ manifest, extraPaths = [] }) {
  const normalized = normalizeManifestPaths(manifest)
  const paths = new Set(['manifest.json', normalized.main || 'main.js'])

  if (normalized.styles) paths.add(normalized.styles)
  if (normalized.widgetStyles) paths.add(normalized.widgetStyles)
  if (normalized.pageStyles) paths.add(normalized.pageStyles)
  if (normalized.pageTemplate) paths.add(normalized.pageTemplate)

  for (const module of normalized.pageModules || []) {
    paths.add(`page/${module}`)
  }
  for (const asset of normalized.assets || []) {
    paths.add(assetPackagePath(asset))
  }
  for (const widget of normalized.widgets || []) {
    for (const template of Object.values(widget.templates || {})) {
      if (template) paths.add(template)
    }
  }
  for (const path of extraPaths) paths.add(path)
  return [...paths].sort()
}

/**
 * Compose main.js the same way Playground does for structured code.
 * Disk projects usually already ship a composed main.js; this is for build.
 */
export function buildMainJs({ core = '', widget = '', page = '', pageModules } = {}) {
  const hasPageModules = pageModules && Object.keys(pageModules).length > 0
  if (hasPageModules) {
    return [
      core || '',
      widget ? `\n${PACKAGE_MARKERS.widget}\n${widget}` : '',
    ].join('')
  }
  return [
    core || '',
    widget ? `\n${PACKAGE_MARKERS.widget}\n${widget}` : '',
    page ? `\n${PACKAGE_MARKERS.page}\n${page}` : '',
  ].join('')
}
