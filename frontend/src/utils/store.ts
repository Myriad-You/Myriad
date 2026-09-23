import { useSyncExternalStore } from 'react'

/**
 * Module-level state shaped for `useSyncExternalStore`. `get` and `subscribe`
 * are stable functions, and `set` publishes only when the value actually
 * changes, so a snapshot keeps its identity until there is something new.
 */
export interface Store<T> {
  readonly get: () => T
  readonly set: (next: T | ((current: T) => T)) => void
  readonly subscribe: (listener: () => void) => () => void
}

export function createStore<T>(
  initial: T,
  equals: (a: T, b: T) => boolean = Object.is,
): Store<T> {
  let value = initial
  const listeners = new Set<() => void>()
  return {
    get: () => value,
    set: (next) => {
      const resolved = typeof next === 'function'
        ? (next as (current: T) => T)(value)
        : next
      if (equals(value, resolved)) return
      value = resolved
      for (const listener of listeners) listener()
    },
    subscribe: (listener) => {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
}

/** Shallow merge for object stores; a patch that changes nothing publishes nothing. */
export function patchStore<T extends object>(store: Store<T>, patch: Partial<T>): T {
  store.set((current) => {
    for (const key of Object.keys(patch) as (keyof T)[]) {
      if (!Object.is(current[key], patch[key])) return { ...current, ...patch }
    }
    return current
  })
  return store.get()
}

export function useStore<T>(store: Store<T>): T {
  return useSyncExternalStore(store.subscribe, store.get, store.get)
}
