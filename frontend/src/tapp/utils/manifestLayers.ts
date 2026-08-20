/**
 * Manifest 层的派生读取。
 *
 * 页面存在与否由 `page` 层是否声明决定，不再是一个能和内容对不上的独立开关。
 * 这些派生刻意留在前端：把 `hasPage` 注入回透传的 manifest 会让那份 JSON 在被
 * 送回安装/更新时撞上后端的 `deny_unknown_fields`，而且 Playground 与打包校验
 * 根本不经过 catalog 透传，注入也覆盖不到它们。
 */

import type { TappManifest } from '../types'

/** 该 Tapp 是否有可打开的页面。 */
export function tappHasPage(manifest: Pick<TappManifest, 'page'>): boolean {
  return manifest.page !== undefined
}

/** 层入口的稳定顺序：core、page、各 widget。 */
export function tappLayerEntries(
  manifest: Pick<TappManifest, 'core' | 'page' | 'widgets'>,
): string[] {
  const entries: string[] = []
  if (manifest.core?.entry) entries.push(manifest.core.entry)
  if (manifest.page?.entry) entries.push(manifest.page.entry)
  for (const widget of manifest.widgets || []) {
    if (widget.entry) entries.push(widget.entry)
  }
  return entries
}
