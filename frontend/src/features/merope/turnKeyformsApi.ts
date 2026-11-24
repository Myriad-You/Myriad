import type { SeeThroughDecomposeInput } from './api'
import { attachTurnKeyforms, isDocumentTurnKeyforms } from '@myriad/figure/rig'
import { currentCopy } from '../../i18n/localeCopy'
import { ApiError, apiService } from '../../services/api'
import { fullBodyPath, MeropeApiError, meropeError, PREFIX, SEE_THROUGH_TIMEOUT_MS, seeThroughError } from './api'

/** What the asking page puts on a job's result beside its keys: the redrawn expressions, the gesture arms. */
export interface TurnKeyformsBeside {
  expressions: boolean
  arms: boolean
}

/**
 * What a job was asked for: the choices that decide what it draws, decomposes
 * and fits. A page picking the job up shows and uses these, not its own boxes.
 */
export interface TurnKeyformsAsked {
  /** The head's turn keyed; says nothing of a kept job used again. */
  head: boolean
  /** A standing figure's body keyed; says nothing of a kept job used again. */
  body: boolean
  /** The bust split once, no keys. */
  plain: boolean
  /** The kept job used instead of drawing and decomposing anew. */
  fromArchive: TurnArchiveUse | null
  /** Whether the asking page puts redrawn expressions on the result; null when it did not say. */
  expressions: boolean | null
  /** The same of the gesture arms. */
  arms: boolean | null
}

/**
 * Where a turn-keys job stands: generating the four turned drawings,
 * decomposing all five, fitting. A standing figure's is enlarged first and
 * stitched from its tiles last, its body's keys made alongside (`body`).
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
  /**
   * Which part of the work the stage belongs to: the worn bust's, a standing
   * figure's own or its head's, waiting on its body, a refit, a loaded archive.
   * Absent from a server before it was reported.
   */
  phase?: 'bust' | 'figure' | 'head' | 'body' | 'refit' | 'load' | 'plain'
  /** A standing figure's body keys, made alongside its head's: none until they start. */
  body?: {
    motion: 'turn' | 'weight'
    stage: 'generating' | 'decomposing' | 'fitting' | 'done' | 'failed'
    done: number
    total: number
  } | null
  /** What the job was asked for. Absent from a server that does not report it. */
  asked?: TurnKeyformsAsked
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
  /** A figure's body motions keyed beside its head; null for a bust or a figure kept before they were recorded. */
  bodyMotions?: Array<'turn' | 'weight'> | null
  /** Whether its portrait is still the slot's current one (only then can it be refitted and imported). */
  current: boolean
}

/** A kept job to use: fitted again with the Space's current fit, or loaded as it was. */
export interface TurnArchiveUse {
  id: string
  refit: boolean
}

const ARCHIVES = `${PREFIX}/see-through/turn-archives`

const TURN_KEYFORMS_POLL_MS = 5000
/**
 * The server lost the job (it restarted): start it again as it was asked;
 * what was already made is kept, so only what is missing is made.
 */
const TURN_KEYFORMS_RESTARTS = 3
/** Polls that may fail in a row (a dropped connection, the server restarting) before giving up. */
const TURN_KEYFORMS_POLL_FAILURES = 24
/** Four drawings, a figure's eight decompositions and a fit: well under an hour and a half. */
const TURN_KEYFORMS_LIMIT_MS = 90 * 60 * 1000

/** The choices a job is started with. */
interface TurnKeyformsChoices {
  /** Use this kept job (fitted again, or as it was) instead of drawing and decomposing anew. */
  fromArchive?: TurnArchiveUse
  /** A full-body set's parts to key: its head's turn, its body's turn and weight shift (both when absent). */
  parts?: { head: boolean; body: boolean }
  /** The bust split once without keys: the base its redrawn expressions go on. */
  plain?: boolean
  /** Kept by the job for a page that picks it up. */
  beside?: Partial<TurnKeyformsBeside>
}

/**
 * Splits a portrait with the head's turns keyed: the decomposition, with
 * what the turns uncover baked in, and its keys remembered on the file for
 * the importer. Polls the server's job until it finishes.
 */
export async function decomposeWithTurnKeyforms(
  input: Pick<SeeThroughDecomposeInput, 'sourceMasterAssetId' | 'sourceGenerationFingerprint'>,
  options: TurnKeyformsChoices & {
    outfitId?: string
    /**
     * Pick up this job instead of starting one (runningTurnKeyformsFor). Lost by
     * the server while it ran, it is started again only as its status says it
     * was asked, never as this page would ask by default; a status that does
     * not say leaves it lost. One picked up at its end is never started again:
     * it was done, or had failed.
     */
    resume?: TurnKeyformsStatus
    onStatus?: (status: TurnKeyformsStatus) => void
    signal?: AbortSignal
  } = {},
): Promise<File> {
  // What a job is started with: this page's choices, or those of the job it picked up
  // (as its latest status told them), and nothing for one picked up at its end.
  const ended = (status: TurnKeyformsStatus) => status.stage === 'done' || status.stage === 'failed'
  let chosen: TurnKeyformsChoices | null = !options.resume
    ? options
    : ended(options.resume) ? null : choicesOf(options.resume.asked)
  const begin = async (choices: TurnKeyformsChoices) => {
    const start = choices.fromArchive
      ? `${ARCHIVES}/${encodeURIComponent(choices.fromArchive.id)}/${choices.fromArchive.refit ? 'refit' : 'load'}`
      : options.outfitId
        ? `${PREFIX}${fullBodyPath(options.outfitId)}/see-through/turn-keyforms`
        : `${PREFIX}/see-through/turn-keyforms`
    // What the page puts on the result rides in the query: the body is what the job does.
    const said = Object.entries(choices.beside ?? {}).filter(([, value]) => typeof value === 'boolean')
    const beside = new URLSearchParams(said.map(([key, value]) => [key, String(value)])).toString()
    const { jobId } = await apiService.post<{ jobId: string }>(`${start}${beside ? `?${beside}` : ''}`, choices.fromArchive ? {} : {
      sourceMasterAssetId: input.sourceMasterAssetId,
      ...(input.sourceGenerationFingerprint ? { sourceGenerationFingerprint: input.sourceGenerationFingerprint } : {}),
      ...(options.outfitId && choices.parts ? choices.parts : {}),
      ...(choices.plain ? { plain: true } : {}),
    })
    return `${PREFIX}/see-through/turn-keyforms/${encodeURIComponent(jobId)}`
  }
  try {
    let job = options.resume
      ? `${PREFIX}/see-through/turn-keyforms/${encodeURIComponent(options.resume.jobId)}`
      : await begin(options)
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
        if (reason.status === 404 && chosen && restarts < TURN_KEYFORMS_RESTARTS) {
          // A page that left meanwhile starts nothing.
          options.signal?.throwIfAborted()
          restarts += 1
          job = await begin(chosen)
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
      // A status read just as the job was started may not say yet what it was asked: the later one does.
      if (options.resume && restarts === 0 && status.asked && !ended(status)) chosen = choicesOf(status.asked)
      if (status.stage === 'done') break
      if (status.stage === 'failed') {
        throw seeThroughError(500, status.error ?? {}, currentCopy().merope.motionSeeThroughUpstream)
      }
      if (Date.now() > deadline) throw new MeropeApiError(currentCopy().merope.motionSeeThroughUpstream, 504)
      await new Promise((resolve) => setTimeout(resolve, TURN_KEYFORMS_POLL_MS))
    }
    // Fetching counts the result as handed over: a page that left meanwhile leaves it for the next.
    options.signal?.throwIfAborted()
    const [psd, keys] = await Promise.all([
      apiService.get<Blob>(`${job}/psd`, { responseType: 'blob', timeout: SEE_THROUGH_TIMEOUT_MS }),
      apiService.get<unknown>(`${job}/keyforms`, { timeout: SEE_THROUGH_TIMEOUT_MS }),
    ])
    if (!(psd instanceof Blob) || psd.size === 0) {
      throw new MeropeApiError(currentCopy().merope.motionSeeThroughUpstream, 502)
    }
    const file = new File([psd], 'see-through.psd', { type: 'image/vnd.adobe.photoshop' })
    if (isDocumentTurnKeyforms(keys)) {
      attachTurnKeyforms(file, {
        canvas: keys.canvas,
        keyforms: keys.keyforms,
        ...(keys.bodyKeyforms ? { bodyKeyforms: keys.bodyKeyforms } : {}),
      })
    }
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

/** A picked-up job's choices as its status told them; null when it did not. */
function choicesOf(asked: TurnKeyformsAsked | undefined): TurnKeyformsChoices | null {
  if (!asked) return null
  return {
    fromArchive: asked.fromArchive ?? undefined,
    parts: { head: asked.head, body: asked.body },
    plain: asked.plain,
    beside: {
      ...(asked.expressions === null ? {} : { expressions: asked.expressions }),
      ...(asked.arms === null ? {} : { arms: asked.arms }),
    },
  }
}

/**
 * The turn-keys job the server has for this portrait and set (a bust's when
 * `outfitId` is absent) that a page opened or reloaded now picks up, if any:
 * one still running, whose progress shows again without starting another;
 * one done while no page was there, whose result is fetched as any job's; one
 * that failed with no page there, whose failure is shown. A result fetched or
 * a failure shown once is not handed out again.
 */
export async function runningTurnKeyformsFor(sourceMasterAssetId: string, outfitId?: string): Promise<TurnKeyformsStatus | null> {
  if (!sourceMasterAssetId) return null
  const { job } = await apiService.get<{ job: (TurnKeyformsStatus & { sourceMasterAssetId?: string; outfitId?: string | null }) | null }>(
    `${PREFIX}/see-through/turn-keyforms`,
  )
  if (!job || job.sourceMasterAssetId !== sourceMasterAssetId || (job.outfitId ?? undefined) !== outfitId) return null
  return job
}

/** Picks up the job there is for this portrait and set (runningTurnKeyformsFor), as decomposeWithTurnKeyforms; null when there is none. */
export async function resumeTurnKeyforms(
  input: Pick<SeeThroughDecomposeInput, 'sourceMasterAssetId' | 'sourceGenerationFingerprint'>,
  options: { outfitId?: string; onStatus?: (status: TurnKeyformsStatus) => void; signal?: AbortSignal } = {},
): Promise<File | null> {
  const running = await runningTurnKeyformsFor(input.sourceMasterAssetId, options.outfitId)
  if (!running) return null
  options.onStatus?.(running)
  return decomposeWithTurnKeyforms(input, { ...options, resume: running })
}

/** The kept turn-keys jobs, newest first. */
export async function listTurnArchives(): Promise<{ archives: TurnArchive[]; keepPerMaster: number }> {
  return apiService.get(ARCHIVES)
}

/** Pieces an uploaded zip is sent in: well under what proxies take in one request. */
const UPLOAD_PIECE_BYTES = 8 * 1024 * 1024

/**
 * Sends a downloaded zip of kept turn keys back, in pieces, to be kept for
 * the slot's current portrait (null outfit: the worn bust); it is refused
 * unless it was made from the same picture. Returns the new archive's id.
 */
export async function uploadTurnArchive(
  zip: Blob,
  outfitId: string | null,
  onProgress?: (sent: number, total: number) => void,
): Promise<string> {
  const upload = Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, '0')).join('')
  const pieces = `${PREFIX}/see-through/turn-archive-uploads/${upload}`
  for (let offset = 0; offset < zip.size; offset += UPLOAD_PIECE_BYTES) {
    const piece = zip.slice(offset, Math.min(zip.size, offset + UPLOAD_PIECE_BYTES))
    await apiService.put(`${pieces}?offset=${offset}`, piece, { timeout: SEE_THROUGH_TIMEOUT_MS })
    onProgress?.(offset + piece.size, zip.size)
  }
  const finish = outfitId
    ? `${PREFIX}${fullBodyPath(outfitId)}/see-through/turn-archive-uploads/${upload}`
    : pieces
  const { id } = await apiService.post<{ id: string }>(finish, {}, { timeout: SEE_THROUGH_TIMEOUT_MS })
  return id
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
