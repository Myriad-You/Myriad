/** 列表成员关系。已读/收藏走 brewItemState，不在这里改。 */

export function dropItem<T extends { id: number }>(
  items: readonly T[],
  id: number,
): T[] {
  return items.filter((item) => item.id !== id)
}

export function dropItems<T extends { id: number }>(
  items: readonly T[],
  ids: ReadonlySet<number>,
): T[] {
  return Iterator.from(items)
    .filter((item) => !ids.has(item.id))
    .toArray()
}

export function appendUniqueById<T extends { id: number }>(
  current: readonly T[],
  incoming: readonly T[],
): T[] {
  const seen = new Set(current.map((item) => item.id))
  return [
    ...current,
    ...Iterator.from(incoming)
      .filter((item) => !seen.has(item.id))
      .toArray(),
  ]
}

export function dropStarredId(
  selected: Set<number>,
  id: number,
): Set<number> {
  if (!selected.has(id)) return selected
  const next = new Set(selected)
  next.delete(id)
  return next
}
