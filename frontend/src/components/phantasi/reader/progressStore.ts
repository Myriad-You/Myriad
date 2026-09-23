import { createStore } from '../../../utils/store'

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
  const store = createStore(clampProgress(initial))
  return {
    get: store.get,
    set: next => store.set(clampProgress(next)),
    subscribe: store.subscribe,
  }
}
