import { TappKernelError } from '../kernelErrors'

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
    return Promise.reject(new TappKernelError('runtimeModuleUnknown', { id }))
  }
  const cached = cache.get(id)
  if (cached) return cached
  const pending = fetch(THREE_RUNTIME_PATH, { credentials: 'same-origin' })
    .then((response) => {
      if (!response.ok) {
        throw new TappKernelError('runtimeModuleHttp', {
          status: response.status,
          id,
        })
      }
      return response.text()
    })
    .then((source) => source.replaceAll(/<\/script/gi, '<\\/script'))
    .catch((error) => {
      cache.delete(id)
      throw error
    })
  cache.set(id, pending)
  return pending
}
