/**
 * Package path layout shared with Playground export / direct-install.
 * Mirrors frontend/src/tapp/utils/playgroundPackageFiles.ts path rules for
 * disk-based CLI projects (content still comes from the filesystem).
 */

export function assetPackagePath(path) {
  return path.startsWith('assets/') ? path : `assets/${path.replace(/^\/+/, '')}`
}

/** 层入口按固定顺序展开：core、page、各 widget。 */
export function manifestLayerEntries(manifest) {
  const entries = []
  if (manifest?.core?.entry) entries.push(manifest.core.entry)
  if (manifest?.page?.entry) entries.push(manifest.page.entry)
  for (const widget of manifest?.widgets || []) {
    if (widget?.entry) entries.push(widget.entry)
  }
  return entries
}

/**
 * Expected relative package paths for a disk project after validation.
 *
 * 只列 manifest 声明的层入口与层资源。入口 require 进来的文件属于包内实现，
 * 由安装时扫描登记，不需要作者在 manifest 里逐个声明。
 *
 * @param {object} options
 * @param {object} options.manifest
 * @param {string[]} [options.extraPaths] existing files already collected
 */
export function expectedPackagePaths({ manifest, extraPaths = [] }) {
  const paths = new Set(['manifest.json', ...manifestLayerEntries(manifest)])

  if (manifest?.core?.styles) paths.add(manifest.core.styles)
  if (manifest?.page?.styles) paths.add(manifest.page.styles)
  if (manifest?.page?.template) paths.add(manifest.page.template)

  for (const asset of manifest?.assets || []) {
    paths.add(assetPackagePath(asset))
  }
  for (const widget of manifest?.widgets || []) {
    if (widget?.styles) paths.add(widget.styles)
    for (const template of Object.values(widget?.templates || {})) {
      if (template) paths.add(template)
    }
  }
  for (const path of extraPaths) paths.add(path)
  return [...paths].sort()
}
