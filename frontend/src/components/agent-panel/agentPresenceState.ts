/**
 * 进出场记账：DOM 不能说卸就卸，退场那段动画还得留着这行。
 *
 * 纯函数，时间从外面喂，方便测。真正的卸载时机由 hook 用 until 来掐。
 */

export const AGENT_ROW_MS = 480
export const AGENT_SWAP_MS = 320

export type PresencePhase = 'in' | 'out'

export interface PresenceEntry<T> {
  key: string
  item: T
  phase: PresencePhase
  until: number
}

export function readPresenceDuration(durationMs = AGENT_ROW_MS): number {
  if (typeof document === 'undefined') return durationMs
  if (document.documentElement.dataset.perfMode === 'exlight') return 0
  if (
    typeof window !== 'undefined' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  ) {
    return 0
  }
  return durationMs
}

export function presenceEqual<T>(
  a: readonly PresenceEntry<T>[],
  b: readonly PresenceEntry<T>[],
): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i += 1) {
    if (
      a[i].key !== b[i].key ||
      a[i].phase !== b[i].phase ||
      a[i].until !== b[i].until ||
      a[i].item !== b[i].item
    ) {
      return false
    }
  }
  return true
}

export function presenceNextDrop(
  entries: readonly PresenceEntry<unknown>[],
  now: number,
): number | null {
  let soon: number | null = null
  for (const entry of entries) {
    if (entry.phase !== 'out') continue
    if (soon === null || entry.until < soon) soon = entry.until
  }
  if (soon === null) return null
  return Math.max(0, soon - now)
}

export function reconcilePresence<T>(
  prev: readonly PresenceEntry<T>[],
  items: readonly T[],
  keyOf: (item: T) => string,
  now: number,
  durationMs: number,
): PresenceEntry<T>[] {
  if (durationMs <= 0) {
    return items.map((item) => ({
      key: keyOf(item),
      item,
      phase: 'in',
      until: 0,
    }))
  }

  const incoming = new Map<string, T>()
  const incomingKeys: string[] = []
  for (const item of items) {
    const key = keyOf(item)
    incoming.set(key, item)
    incomingKeys.push(key)
  }

  const result: PresenceEntry<T>[] = []
  const placed = new Set<string>()

  const keep = (key: string) => {
    const item = incoming.get(key)
    if (!item || placed.has(key)) return
    placed.add(key)
    result.push({
      key,
      item,
      phase: 'in',
      until: 0,
    })
  }

  for (const entry of prev) {
    if (incoming.has(entry.key)) {
      keep(entry.key)
      continue
    }
    if (placed.has(entry.key)) continue
    const until = entry.phase === 'out' ? entry.until : now + durationMs
    if (until <= now) continue
    placed.add(entry.key)
    result.push({
      key: entry.key,
      item: entry.item,
      phase: 'out',
      until,
    })
  }

  for (let i = 0; i < incomingKeys.length; i += 1) {
    const key = incomingKeys[i]
    if (placed.has(key)) continue
    let at = result.length
    for (let j = i - 1; j >= 0; j -= 1) {
      const idx = result.findIndex((entry) => entry.key === incomingKeys[j])
      if (idx >= 0) {
        at = idx + 1
        break
      }
    }
    const item = incoming.get(key)
    if (!item) continue
    placed.add(key)
    result.splice(at, 0, { key, item, phase: 'in', until: 0 })
  }

  return result
}
