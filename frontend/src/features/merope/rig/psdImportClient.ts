import type { Anime25DImportCopy } from './anime25dImportCopy'
import type { PreparedAnime25DRigImport } from './anime25dImporter'

export interface RigPsdImportRequest {
  buffer: ArrayBuffer
  sourceMasterAssetId: string
  sourceMasterUrl: string
  sourceGenerationFingerprint?: string
  copy: Anime25DImportCopy
}

export type RigPsdImportStage = 'validated' | 'packing'
export type RigPsdImportReply =
  | { stage: RigPsdImportStage }
  | { prepared: PreparedAnime25DRigImport }
  | { error: string }

/** One worker owns decoding through PNG packing; no UI-thread compile fallback. */
export function importRigPsdInWorker(
  request: RigPsdImportRequest,
  signal?: AbortSignal,
  onStage?: (stage: RigPsdImportStage) => void,
  createWorker: () => Worker = () =>
    new Worker(new URL('./psdImport.worker.ts', import.meta.url), {
      type: 'module',
    }),
): Promise<PreparedAnime25DRigImport> {
  signal?.throwIfAborted()
  return new Promise((resolve, reject) => {
    const worker = createWorker()
    let settled = false
    const finish = (error?: unknown, prepared?: PreparedAnime25DRigImport) => {
      if (settled) return
      settled = true
      clearTimeout(timeout)
      signal?.removeEventListener('abort', abort)
      worker.onmessage = null
      worker.onerror = null
      worker.onmessageerror = null
      worker.terminate()
      if (prepared) resolve(prepared)
      else reject(error)
    }
    const abort = () =>
      finish(signal?.reason ?? new DOMException('Aborted', 'AbortError'))
    const timeout = setTimeout(
      () => finish(new Error(request.copy.psdSpecInvalid)),
      60_000,
    )
    worker.onmessage = (event: MessageEvent<RigPsdImportReply>) => {
      if (settled) return
      if (signal?.aborted) {
        abort()
      } else if ('stage' in event.data) {
        try {
          onStage?.(event.data.stage)
        } catch (error) {
          finish(error)
        }
      } else if ('prepared' in event.data) {
        finish(undefined, event.data.prepared)
      } else {
        finish(new Error(event.data.error))
      }
    }
    worker.onerror = () => finish(new Error(request.copy.psdSpecInvalid))
    worker.onmessageerror = () => finish(new Error(request.copy.psdSpecInvalid))
    signal?.addEventListener('abort', abort, { once: true })
    if (signal?.aborted) {
      abort()
      return
    }
    try {
      worker.postMessage(request, [request.buffer])
    } catch (error) {
      finish(error)
    }
  })
}
