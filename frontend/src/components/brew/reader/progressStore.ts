export interface ReadingProgress {
  get: () => number
  set: (value: number) => void
  subscribe: (onStoreChange: () => void) => () => void
}

export function clampProgress(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(0, Math.min(100, Math.round(value)))
}

export function createReadingProgress(initial = 0): ReadingProgress {
  let value = clampProgress(initial)
  const listeners = new Set<() => void>()
  return {
    get: () => value,
    set: (next) => {
      const clamped = clampProgress(next)
      if (clamped === value) return
      value = clamped
      listeners.forEach((listener) => listener())
    },
    subscribe: (onStoreChange) => {
      listeners.add(onStoreChange)
      return () => {
        listeners.delete(onStoreChange)
      }
    },
  }
}
