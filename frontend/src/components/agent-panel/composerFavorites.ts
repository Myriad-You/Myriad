/**
 * 输入行收藏：开开关关面板不该反复打 presets。
 * 收藏有增删时清掉，下次打开再读。存进常用后通知输入行立刻重读。
 */

import { agentService } from '../../services/agent'

export interface ComposerFavorite {
  id: number
  input: string
  title?: string
}

/** 中间列能横滑，不必只留三四枚。 */
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
