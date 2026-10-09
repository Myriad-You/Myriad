import type { SeeThroughDecomposeInput } from './api'
import { currentCopy } from '../../i18n/localeCopy'
import { ApiError, apiService } from '../../services/api'
import { fullBodyPath, MeropeApiError, meropeError, PREFIX, SEE_THROUGH_TIMEOUT_MS, seeThroughError } from './api'
import { attachTurnKeyforms, isDocumentTurnKeyforms } from './rig/turnKeyformImport'

/**
 * Where a turn-keys job stands: generating the four turned drawings,
 * decomposing all five, fitting. A standing figure's is enlarged first and
 * stitched from its tiles last (a plain figure's only that).
 */
export interface TurnKeyformsStatus {
  jobId: string
  stage: 'upscaling' | 'generating' | 'decomposing' | 'fitting' | 'stitching' | 'done' | 'failed'
  done: number
  total: number
  seconds: number
  error?: { error?: string; code?: string } | null
  /** The archive the finished job was kept as. */
  archiveId?: string | null
}

/** A finished turn-keys job kept on the server: its drawings, decompositions and keys. */
export interface TurnArchive {
  id: string
  createdAt: string
  /** null: the worn bust. */
  outfitId: string | null
  sourceMasterAssetId: string
  /** The archive this one was refitted from. */
  parent: string | null
  /** Turned drawings drawn again after the Space's check. */
  redrawn: string[]
  bytes: number
  /** Whether its portrait is still the slot's current one (only then can it be refitted and imported). */
  current: boolean
}

const ARCHIVES = `${PREFIX}/see-through/turn-archives`

const TURN_KEYFORMS_POLL_MS = 5000
/** The server lost the job (it restarted): start it again; what was already made is kept. */
const TURN_KEYFORMS_RESTARTS = 3
/** Polls that may fail in a row (a dropped connection, the server restarting) before giving up. */
const TURN_KEYFORMS_POLL_FAILURES = 24
/** Four drawings, a figure's eight decompositions and a fit: well under an hour and a half. */
const TURN_KEYFORMS_LIMIT_MS = 90 * 60 * 1000

/**
 * Splits a portrait with the head's turns keyed: the decomposition, with
 * what the turns uncover baked in, and its keys remembered on the file for
 * the importer. Polls the server's job until it finishes.
 */
export async function decomposeWithTurnKeyforms(
  input: Pick<SeeThroughDecomposeInput, 'sourceMasterAssetId' | 'sourceGenerationFingerprint'>,
  options: {
    outfitId?: string
    /** Fit this kept job again instead of drawing and decomposing anew. */
    fromArchive?: string
    /** A full-body set decomposed in tiles without turn keys (needs outfitId). */
    plain?: boolean
    onStatus?: (status: TurnKeyformsStatus) => void
    signal?: AbortSignal
  } = {},
): Promise<File> {
  const start = options.fromArchive
    ? `${ARCHIVES}/${encodeURIComponent(options.fromArchive)}/refit`
    : options.outfitId
      ? `${PREFIX}${fullBodyPath(options.outfitId)}/see-through/${options.plain ? 'figure' : 'turn-keyforms'}`
      : `${PREFIX}/see-through/turn-keyforms`
  const begin = async () => {
    const { jobId } = await apiService.post<{ jobId: string }>(start, options.fromArchive ? {} : {
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

/** The kept turn-keys jobs, newest first. */
export async function listTurnArchives(): Promise<{ archives: TurnArchive[]; keepPerMaster: number }> {
  return apiService.get(ARCHIVES)
}

/** Saves a kept job as a zip. */
export async function downloadTurnArchive(id: string): Promise<void> {
  const zip = await apiService.get<Blob>(`${ARCHIVES}/${encodeURIComponent(id)}/zip`, {
    responseType: 'blob',
    timeout: SEE_THROUGH_TIMEOUT_MS,
  })
  const objectUrl = URL.createObjectURL(zip)
  const link = document.createElement('a')
  link.href = objectUrl
  link.download = `turn-keys-${id}.zip`
  document.body.appendChild(link)
  link.click()
  link.remove()
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1_000)
}

export async function deleteTurnArchive(id: string): Promise<void> {
  await apiService.delete(`${ARCHIVES}/${encodeURIComponent(id)}`)
}
