import type { SeeThroughDecomposeInput } from './api'
import { currentCopy } from '../../i18n/localeCopy'
import { ApiError, apiService } from '../../services/api'
import { fullBodyPath, MeropeApiError, meropeError, PREFIX, SEE_THROUGH_TIMEOUT_MS, seeThroughError } from './api'
import { attachTurnKeyforms, isDocumentTurnKeyforms } from './rig/turnKeyformImport'

/** Where a turn-keys job stands: generating the four turned drawings, decomposing all five, fitting. */
export interface TurnKeyformsStatus {
  jobId: string
  stage: 'generating' | 'decomposing' | 'fitting' | 'done' | 'failed'
  done: number
  total: number
  seconds: number
  error?: { error?: string; code?: string } | null
}

const TURN_KEYFORMS_POLL_MS = 5000
/** The server lost the job (it restarted): start it again; what was already made is kept. */
const TURN_KEYFORMS_RESTARTS = 3
/** Polls that may fail in a row (a dropped connection, the server restarting) before giving up. */
const TURN_KEYFORMS_POLL_FAILURES = 24
/** Four drawings, five decompositions and a fit: well under an hour. */
const TURN_KEYFORMS_LIMIT_MS = 90 * 60 * 1000

/**
 * Splits a portrait with the head's turns keyed: the decomposition, with
 * what the turns uncover baked in, and its keys remembered on the file for
 * the importer. Polls the server's job until it finishes.
 */
export async function decomposeWithTurnKeyforms(
  input: Pick<SeeThroughDecomposeInput, 'sourceMasterAssetId' | 'sourceGenerationFingerprint'>,
  options: { outfitId?: string; onStatus?: (status: TurnKeyformsStatus) => void; signal?: AbortSignal } = {},
): Promise<File> {
  const start = options.outfitId
    ? `${PREFIX}${fullBodyPath(options.outfitId)}/see-through/turn-keyforms`
    : `${PREFIX}/see-through/turn-keyforms`
  const begin = async () => {
    const { jobId } = await apiService.post<{ jobId: string }>(start, {
      sourceMasterAssetId: input.sourceMasterAssetId,
      ...(input.sourceGenerationFingerprint ? { sourceGenerationFingerprint: input.sourceGenerationFingerprint } : {}),
    })
    return `${PREFIX}/see-through/turn-keyforms/${encodeURIComponent(jobId)}`
  }
  try {
    let job = await begin()
    let restarts = 0
    let failures = 0
    const deadline = Date.now() + TURN_KEYFORMS_LIMIT_MS
    for (;;) {
      options.signal?.throwIfAborted()
      let status: TurnKeyformsStatus
      try {
        status = await apiService.get<TurnKeyformsStatus>(job)
        failures = 0
      } catch (reason) {
        if (!(reason instanceof ApiError) || Date.now() > deadline) throw reason
        if (reason.status === 404 && restarts < TURN_KEYFORMS_RESTARTS) {
          restarts += 1
          job = await begin()
          continue
        }
        // Not reachable for a moment (a dropped connection, the server restarting): keep asking.
        if ((reason.status === 0 || reason.status >= 500) && ++failures <= TURN_KEYFORMS_POLL_FAILURES) {
          await new Promise((resolve) => setTimeout(resolve, TURN_KEYFORMS_POLL_MS))
          continue
        }
        throw reason
      }
      options.onStatus?.(status)
      if (status.stage === 'done') break
      if (status.stage === 'failed') {
        throw seeThroughError(500, status.error ?? {}, currentCopy().merope.motionSeeThroughUpstream)
      }
      if (Date.now() > deadline) throw new MeropeApiError(currentCopy().merope.motionSeeThroughUpstream, 504)
      await new Promise((resolve) => setTimeout(resolve, TURN_KEYFORMS_POLL_MS))
    }
    const [psd, keys] = await Promise.all([
      apiService.get<Blob>(`${job}/psd`, { responseType: 'blob', timeout: SEE_THROUGH_TIMEOUT_MS }),
      apiService.get<unknown>(`${job}/keyforms`, { timeout: SEE_THROUGH_TIMEOUT_MS }),
    ])
    if (!(psd instanceof Blob) || psd.size === 0) {
      throw new MeropeApiError(currentCopy().merope.motionSeeThroughUpstream, 502)
    }
    const file = new File([psd], 'see-through.psd', { type: 'image/vnd.adobe.photoshop' })
    if (isDocumentTurnKeyforms(keys)) attachTurnKeyforms(file, { canvas: keys.canvas, keyforms: keys.keyforms })
    return file
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    if (reason instanceof DOMException && reason.name === 'AbortError') throw reason
    if (reason instanceof ApiError && reason.status > 0) {
      throw seeThroughError(reason.status, reason.body, currentCopy().merope.motionSeeThroughUpstream)
    }
    throw meropeError(reason, currentCopy().merope.motionSeeThroughUpstream)
  }
}
