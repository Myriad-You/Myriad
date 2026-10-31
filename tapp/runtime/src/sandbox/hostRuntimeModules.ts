import { TappKernelError } from '../kernelErrors'

export const THREE_RUNTIME_ID = 'three'
export const THREE_RUNTIME_PATH = '/tapp-runtime/three.0.170.iife.js'

/** The part of a fetch response the kernel reads. */
export interface RuntimeModuleResponse {
  ok: boolean
  status: number
  text: () => Promise<string>
}

/** How the host fetches a runtime module file it serves at `path`. */
export type RuntimeModuleSource = (path: string) => Promise<RuntimeModuleResponse>

export function manifestRequestsRuntimeModule(
  runtimeModules: unknown,
  id: string,
): boolean {
  return Array.isArray(runtimeModules) && runtimeModules.includes(id)
}

/** 宿主注入的库，一页取一次，取失败下次重试。一页一份，由 TappKernel 持有。 */
export class HostRuntimeModules {
  private readonly cache = new Map<string, Promise<string>>()

  constructor(private readonly fetchModule: RuntimeModuleSource) {}

  load(id: string): Promise<string> {
    if (id !== THREE_RUNTIME_ID) {
      return Promise.reject(new TappKernelError('runtimeModuleUnknown', { id }))
    }
    const cached = this.cache.get(id)
    if (cached) return cached
    const pending = this.fetchModule(THREE_RUNTIME_PATH)
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
        this.cache.delete(id)
        throw error
      })
    this.cache.set(id, pending)
    return pending
  }
}
