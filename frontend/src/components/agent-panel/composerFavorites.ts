import { agentService } from '../../services/agent'

export interface ComposerFavorite {
  id: number
  input: string
  title?: string
}

const FAVORITE_CAP = 12

let cache: ComposerFavorite[] | null = null
let inflight: Promise<ComposerFavorite[]> | null = null
const listeners = new Set<() => void>()

function notifyComposerFavorites(): void {
  for (const listener of listeners) listener()
}

export function subscribeComposerFavorites(onChange: () => void): () => void {
  listeners.add(onChange)
  return () => {
    listeners.delete(onChange)
  }
}

export function loadComposerFavorites(): Promise<ComposerFavorite[]> {
  if (cache) return Promise.resolve(cache)
  if (!inflight) {
    inflight = agentService
      .getPresets()
      .then((response) => {
        cache = response.favorites.slice(0, FAVORITE_CAP)
        inflight = null
        return cache
      })
      .catch((error: unknown) => {
        inflight = null
        throw error
      })
  }
  return inflight
}

export function forgetComposerFavorite(id: number): void {
  if (cache) cache = cache.filter((item) => item.id !== id)
}

export function invalidateComposerFavorites(): void {
  cache = null
  inflight = null
  notifyComposerFavorites()
}
