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

/** 公开列表须等站主布局。先画目录/本地顺序会造成一次重排。 */
export function isSiteOwnerLayoutPending(params: {
  layoutReady: boolean
  isAuthenticated: boolean
  isSiteScope: boolean
}): boolean {
  if (params.layoutReady) return false
  if (!params.isAuthenticated) return true
  return params.isSiteScope
}
