/** Pinned host runtime libraries injected into the Page sandbox with a nonce. */

import { currentCopy } from '../../../i18n/localeCopy'

export const THREE_RUNTIME_ID = 'three'
export const THREE_RUNTIME_PATH = '/tapp-runtime/three.0.170.iife.js'

const cache = new Map<string, Promise<string>>()

export function manifestRequestsRuntimeModule(
  runtimeModules: unknown,
  id: string,
): boolean {
  return Array.isArray(runtimeModules) && runtimeModules.includes(id)
}

export function loadHostRuntimeModule(id: string): Promise<string> {
  if (id !== THREE_RUNTIME_ID) {
    return Promise.reject(new Error(`Unknown runtime module: ${id}`))
  }
  const cached = cache.get(id)
  if (cached) return cached
  const pending = fetch(THREE_RUNTIME_PATH, { credentials: 'same-origin' })
    .then((response) => {
      if (!response.ok) {
        throw new Error(
          `${currentCopy().errors.httpStatus.replace(
            '{status}',
            String(response.status),
          )} (${id})`,
        )
      }
      return response.text()
    })
    .then((source) => source.replace(/<\/script/gi, '<\\/script'))
    .catch((error) => {
      cache.delete(id)
      throw error
    })
  cache.set(id, pending)
  return pending
}
