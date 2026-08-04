/**
 * Pure helpers for TApp list card order.
 * Kept free of React/CSS so unit tests can import without DOM loaders.
 */

/** Apply preferred order to a list of ids (unknown ids keep relative catalog order). */
export function applyTappAppCardOrder<T extends { id: string }>(
  items: T[],
  order: string[],
): T[] {
  if (items.length <= 1 || order.length === 0) return items
  const byId = new Map(items.map((item) => [item.id, item]))
  const out: T[] = []
  const used = new Set<string>()
  for (const id of order) {
    const item = byId.get(id)
    if (!item || used.has(id)) continue
    out.push(item)
    used.add(id)
  }
  for (const item of items) {
    if (used.has(item.id)) continue
    out.push(item)
  }
  return out
}

/**
 * Whether the public TApp list must wait for remote site-owner layout hydrate.
 * Guest primary view and regular-user **site** scope use site-owner order;
 * painting catalog/localStorage order first causes a visible one-time reorder.
 * Personal **mine** scope may use local cache immediately.
 */
export function isSiteOwnerLayoutPending(params: {
  layoutReady: boolean
  isAuthenticated: boolean
  isSiteScope: boolean
}): boolean {
  if (params.layoutReady) return false
  if (!params.isAuthenticated) return true
  return params.isSiteScope
}
