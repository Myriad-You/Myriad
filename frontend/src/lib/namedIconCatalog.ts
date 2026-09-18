import type { IconType } from 'react-icons'

const cache = new Map<string, IconType | null>()
const pending = new Set<string>()
const listeners = new Set<() => void>()
let version = 0

function notify(): void {
  version += 1
  listeners.forEach((listener) => listener())
}

export function subscribeNamedIcons(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function namedIconVersion(): number {
  return version
}

/** `undefined` = not loaded yet; `null` = unknown name. */
export function peekNamedIcon(name: string): IconType | null | undefined {
  if (!cache.has(name)) return undefined
  return cache.get(name)
}

export function requestNamedIcon(name: string): void {
  if (!name || cache.has(name) || pending.has(name)) return
  pending.add(name)
  void import('./iconLookup').then((mod) => {
    cache.set(name, mod.getIconByName(name))
    pending.delete(name)
    notify()
  })
}
