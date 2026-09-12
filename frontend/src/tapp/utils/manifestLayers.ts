/** 页面是否存在由 page 层是否声明决定。不把 hasPage 写回透传 manifest（会撞 deny_unknown_fields）。 */

import type { TappManifest } from '../types'

export function tappHasPage(manifest: Pick<TappManifest, 'page'>): boolean {
  return manifest.page !== undefined
}

/** 层入口顺序：core、page、各 widget。 */
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
