import type { PoseCorrection } from './anime25drig/poseCorrections'
import type { AuthoredExpressionKind } from './rig/authoredExpression'
import type { DetectedSkeleton } from './rig/skeleton'
import type { MeropeRigImportSource, MeropeRigManifest } from './rig/types'
import { currentCopy } from '../../i18n/localeCopy'
import { ApiError, apiService } from '../../services/api'
import { AUTHORED_EXPRESSION_KINDS } from './rig/authoredExpression'
import { isLiveMeropeManifest, isRigManifest } from './rig/manifestValidation'

export const PREFIX = '/merope/rig'
const RIG_MUTATION_TIMEOUT_MS = 6 * 60 * 1000
/** Keep in sync with MEROPE_PROXY_TIMEOUT_MS and get_long_running_client. */
const PORTRAIT_GENERATION_TIMEOUT_MS = 15 * 60 * 1000
// A sleeping Space can take minutes to wake before the split itself starts.
export const SEE_THROUGH_TIMEOUT_MS = 15 * 60 * 1000
/** The first look also downloads the pose model on the server. */
const SKELETON_TIMEOUT_MS = 180_000

export class MeropeApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly code?: string,
  ) {
    super(message)
    this.name = 'MeropeApiError'
  }
}

export interface SiteFace {
  manifest: MeropeRigManifest | null
  portraitUrl: string | null
  generationFingerprint: string | null
  assetId: string | null
}

export interface SeeThroughStatus {
  provider: string
  tokenConfigured: boolean
  defaultResolution: number
  splitArmsAndLegs: boolean
  /** The Space decompositions run on: Myriad's own unless the owner named another. */
  space: string
  defaultSpace: string
}

const DEFAULT_SEE_THROUGH_SPACE = 'SomekawaHitomi/see-through-demo'

function readSeeThroughStatus(data: Partial<SeeThroughStatus>): SeeThroughStatus {
  const space =
    typeof data.space === 'string' && data.space
      ? data.space
      : typeof data.provider === 'string' && data.provider
        ? data.provider
        : DEFAULT_SEE_THROUGH_SPACE
  return {
    provider: space,
    tokenConfigured: data.tokenConfigured === true,
    defaultResolution:
      typeof data.defaultResolution === 'number' ? data.defaultResolution : 1280,
    splitArmsAndLegs: data.splitArmsAndLegs !== false,
    space,
    defaultSpace:
      typeof data.defaultSpace === 'string' && data.defaultSpace
        ? data.defaultSpace
        : DEFAULT_SEE_THROUGH_SPACE,
  }
}

function payloadCode(payload: Record<string, unknown>): string | undefined {
  return typeof payload.code === 'string' && payload.code.trim()
    ? payload.code
    : undefined
}

function readPortraitUrl(data: { portraitUrl?: unknown }): string | null {
  return typeof data.portraitUrl === 'string' && data.portraitUrl.trim()
    ? data.portraitUrl
    : null
}

export function meropeError(
  reason: unknown,
  fallback: string,
  status = 500,
): MeropeApiError {
  const failure = reason instanceof ApiError && reason.status > 0 ? reason : null
  const resolvedStatus = failure ? failure.status : status
  const payload =
    failure?.body && typeof failure.body === 'object'
      ? (failure.body as Record<string, unknown>)
      : {}
  return new MeropeApiError(
    typeof payload.error === 'string'
      ? payload.error
      : typeof payload.message === 'string'
        ? payload.message
        : reason instanceof Error
          ? reason.message
          : fallback,
    resolvedStatus,
    payloadCode(payload),
  )
}

let siteFaceInflight: Promise<SiteFace> | null = null

export async function getSiteFace(): Promise<SiteFace> {
  if (siteFaceInflight) return siteFaceInflight
  siteFaceInflight = loadFace(`${PREFIX}/active`).finally(() => {
    siteFaceInflight = null
  })
  return siteFaceInflight
}

export function fullBodyPath(outfitId: string): string {
  return `/full-body/${encodeURIComponent(outfitId)}`
}

/**
 * The joints the backend found on a master portrait, or null when it found
 * none or cannot look: the rig then falls back to its own proportions.
 */
async function getSkeleton(path: string): Promise<DetectedSkeleton | null> {
  try {
    const data = await apiService.get<Partial<DetectedSkeleton>>(`${PREFIX}${path}`, {
      timeout: SKELETON_TIMEOUT_MS,
    })
    const keypoints = data.keypoints
    if (
      typeof data.sourceMasterAssetId !== 'string' ||
      typeof data.model !== 'string' ||
      !(typeof data.width === 'number' && data.width > 0) ||
      !(typeof data.height === 'number' && data.height > 0) ||
      !Array.isArray(keypoints) ||
      !keypoints.every(
        (point) =>
          Array.isArray(point) &&
          point.length === 3 &&
          point.every((value) => typeof value === 'number' && Number.isFinite(value)),
      )
    ) {
      return null
    }
    return {
      sourceMasterAssetId: data.sourceMasterAssetId,
      model: data.model,
      width: data.width,
      height: data.height,
      keypoints,
    }
  } catch {
    return null
  }
}

/** The worn bust's skeleton. */
export function getSiteSkeleton(): Promise<DetectedSkeleton | null> {
  return getSkeleton('/skeleton')
}

/** A full-body set's skeleton. */
export function getFullBodySkeleton(outfitId: string): Promise<DetectedSkeleton | null> {
  return getSkeleton(`${fullBodyPath(outfitId)}/skeleton`)
}

/**
 * The picture a full-body set's tiles were cut from (its master enlarged),
 * or null while it has none (not decomposed in tiles yet).
 */
export async function getFullBodyFigurePicture(outfitId: string): Promise<Blob | null> {
  try {
    const picture = await apiService.get<Blob>(`${PREFIX}${fullBodyPath(outfitId)}/see-through/figure/picture`, {
      responseType: 'blob',
      timeout: SEE_THROUGH_TIMEOUT_MS,
    })
    return picture instanceof Blob && picture.size > 0 ? picture : null
  } catch {
    return null
  }
}

/** A full-body set's picture and package, for the owner's workbench. */
export async function getFullBodyFace(outfitId: string): Promise<SiteFace> {
  return loadFace(`${PREFIX}${fullBodyPath(outfitId)}`)
}

async function loadFace(path: string): Promise<SiteFace> {
  try {
    const data = await apiService.get<{
      manifest?: unknown
      portraitUrl?: unknown
      generationFingerprint?: unknown
      assetId?: unknown
    }>(path)
    return readFaceResponse(data)
  } catch (reason) {
    const error = meropeError(reason, currentCopy().merope.loadFailed)
    if (error.status === 404) return readFaceResponse({})
    throw error
  }
}

export async function getWardrobeFace(outfitId: string): Promise<SiteFace> {
  const id = outfitId.trim()
  if (!id) {
    return {
      manifest: null,
      portraitUrl: null,
      generationFingerprint: null,
      assetId: null,
    }
  }
  return loadFace(`/agent/wardrobe/${encodeURIComponent(id)}/face`)
}

function readFaceResponse(
  data: {
    manifest?: unknown
    portraitUrl?: unknown
    generationFingerprint?: unknown
    assetId?: unknown
  },
): SiteFace {
  const manifest = isLiveMeropeManifest(data.manifest) ? data.manifest : null
  return {
    manifest,
    portraitUrl: readPortraitUrl(data),
    generationFingerprint:
      typeof data.generationFingerprint === 'string' &&
      /^[0-9a-f]{64}$/iu.test(data.generationFingerprint)
        ? data.generationFingerprint.toLowerCase()
        : null,
    assetId: typeof data.assetId === 'string' ? data.assetId : null,
  }
}

export async function getSeeThroughStatus(): Promise<SeeThroughStatus> {
  const data = await apiService.get<Partial<SeeThroughStatus>>(
    `${PREFIX}/see-through/status`,
  )
  return readSeeThroughStatus(data)
}

export async function updateSeeThroughToken(
  token: string,
): Promise<SeeThroughStatus> {
  const data = await apiService.patch<Partial<SeeThroughStatus>>(
    `${PREFIX}/see-through/token`,
    { token },
  )
  return readSeeThroughStatus(data)
}

/**
 * Names the See-through Space (`owner/name`) to decompose with; empty goes back
 * to Myriad's own. A Space serving `decompose` fits portraits on a canvas of
 * their own shape; any other (the public demo) pads them to a square.
 */
export async function updateSeeThroughSpace(
  space: string,
): Promise<SeeThroughStatus> {
  const data = await apiService.patch<Partial<SeeThroughStatus>>(
    `${PREFIX}/see-through/space`,
    { space: space.trim() || null },
  )
  return readSeeThroughStatus(data)
}

/** The PSD endpoint's failures carry no useful HTTP status text; prefer the domain fallback. */
export function seeThroughError(status: number, body: unknown, fallback: string): MeropeApiError {
  const payload =
    body && typeof body === 'object' ? (body as Record<string, unknown>) : {}
  return new MeropeApiError(
    typeof payload.error === 'string'
      ? payload.error
      : typeof payload.message === 'string'
        ? payload.message
        : fallback,
    status,
    payloadCode(payload),
  )
}

export interface SeeThroughDecomposeInput {
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  resolution?: number
  seed?: number
  splitArmsAndLegs?: boolean
}

export async function decomposeSitePortraitWithSeeThrough(
  input: SeeThroughDecomposeInput,
): Promise<File> {
  return decomposeWithSeeThrough('/see-through/decompose', input)
}

async function decomposeWithSeeThrough(
  path: string,
  input: SeeThroughDecomposeInput,
): Promise<File> {
  try {
    const psd = await apiService.post<Blob>(
      `${PREFIX}${path}`,
      input,
      {
        responseType: 'blob',
        timeout: SEE_THROUGH_TIMEOUT_MS,
      },
    )
    if (!(psd instanceof Blob) || psd.size === 0) {
      throw new MeropeApiError(
        currentCopy().merope.motionSeeThroughUpstream,
        502,
      )
    }
    return new File([psd], 'see-through.psd', {
      type: 'image/vnd.adobe.photoshop',
    })
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    if (reason instanceof ApiError && reason.status > 0) {
      throw seeThroughError(
        reason.status,
        reason.body,
        currentCopy().merope.motionSeeThroughUpstream,
      )
    }
    throw meropeError(reason, currentCopy().merope.motionSeeThroughUpstream)
  }
}

export async function importMeropeRig(
  source: MeropeRigImportSource,
  atlas: Blob,
): Promise<MeropeRigManifest> {
  return submitMeropeRigImport('/import', source, atlas, 'import')
}

/** Previews and stores packages for one full-body set. */
export function fullBodyRigImport(outfitId: string) {
  const path = fullBodyPath(outfitId)
  return {
    preview: (
      source: MeropeRigImportSource,
      atlas: Blob,
      analysisReference: Blob,
    ): Promise<MeropeRigManifest> =>
      submitMeropeRigImport(
        `${path}/import/preview`,
        source,
        atlas,
        'preview',
        analysisReference,
      ),
    commit: (
      source: MeropeRigImportSource,
      atlas: Blob,
    ): Promise<MeropeRigManifest> =>
      submitMeropeRigImport(`${path}/import`, source, atlas, 'import'),
  }
}

export async function saveRigPoseCorrections(assetId: string, corrections: PoseCorrection[]): Promise<{ manifest: MeropeRigManifest; assetId: string }> {
  try {
    const data = await apiService.patch<{ manifest: unknown; assetId: unknown }>(`${PREFIX}/pose-corrections`, { assetId, corrections })
    if (!isRigManifest(data.manifest) || typeof data.assetId !== 'string' || !/^[0-9a-f]{64}$/u.test(data.assetId)) {
      throw new Error(currentCopy().merope.poseCorrection.failed)
    }
    return { manifest: data.manifest, assetId: data.assetId }
  } catch (reason) {
    const error = meropeError(reason, currentCopy().merope.poseCorrection.failed)
    if (error.status === 409) throw new Error(currentCopy().merope.poseCorrection.conflict)
    throw error
  }
}

/** A full-body set's corrections, saved into a new package for that set. */
export async function saveFullBodyPoseCorrections(outfitId: string, assetId: string, corrections: PoseCorrection[]): Promise<{ manifest: MeropeRigManifest; assetId: string }> {
  try {
    const data = await apiService.patch<{ manifest: unknown; assetId: unknown }>(`${PREFIX}${fullBodyPath(outfitId)}/pose-corrections`, { assetId, corrections })
    if (!isRigManifest(data.manifest) || typeof data.assetId !== 'string' || !/^[0-9a-f]{64}$/u.test(data.assetId)) {
      throw new Error(currentCopy().merope.poseCorrection.failed)
    }
    return { manifest: data.manifest, assetId: data.assetId }
  } catch (reason) {
    const error = meropeError(reason, currentCopy().merope.poseCorrection.failed)
    if (error.status === 409) throw new Error(currentCopy().merope.poseCorrection.conflict)
    throw error
  }
}

export async function previewMeropeRigImport(
  source: MeropeRigImportSource,
  atlas: Blob,
  analysisReference: Blob,
): Promise<MeropeRigManifest> {
  return submitMeropeRigImport(
    '/import/preview',
    source,
    atlas,
    'preview',
    analysisReference,
  )
}

async function submitMeropeRigImport(
  path: string,
  source: MeropeRigImportSource,
  atlas: Blob,
  action: string,
  analysisReference?: Blob,
): Promise<MeropeRigManifest> {
  const body = new FormData()
  body.append('source', JSON.stringify(source))
  body.append('atlas', atlas, 'rig-atlas.png')
  if (analysisReference) {
    body.append('analysisReference', analysisReference, 'rig-analysis.png')
  }
  let data
  try {
    data = await apiService.post<{ manifest: unknown }>(`${PREFIX}${path}`, body, {
      timeout: RIG_MUTATION_TIMEOUT_MS,
    })
  } catch (reason) {
    throw meropeError(
      reason,
      action === 'commit'
        ? currentCopy().merope.rigCommitFailed
        : currentCopy().merope.rigImportFailed,
    )
  }
  if (!isRigManifest(data.manifest)) {
    throw new Error(currentCopy().merope.rigCompileFailed)
  }
  return data.manifest
}

export async function uploadSitePortrait(image: Blob): Promise<{
  portraitUrl: string
}> {
  const body = new FormData()
  body.append(
    'image',
    image,
    image instanceof File ? image.name : 'uploaded-portrait.png',
  )
  try {
    const data = await apiService.post<{ portraitUrl?: unknown }>(
      `${PREFIX}/portrait/upload`,
      body,
      {
        timeout: RIG_MUTATION_TIMEOUT_MS,
      },
    )
    const portraitUrl = readPortraitUrl(data)
    if (!portraitUrl) {
      throw new MeropeApiError(currentCopy().merope.portraitUploadFailed, 502)
    }
    return { portraitUrl }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.portraitUploadFailed)
  }
}

/** The owner's own picture for a bust set that is not worn. */
export async function uploadOutfitPortrait(
  outfitId: string,
  image: Blob,
): Promise<{
  portraitUrl: string
}> {
  const body = new FormData()
  body.append(
    'image',
    image,
    image instanceof File ? image.name : 'uploaded-portrait.png',
  )
  try {
    const data = await apiService.post<{ portraitUrl?: unknown }>(
      `${PREFIX}/outfits/${encodeURIComponent(outfitId)}/portrait/upload`,
      body,
      { timeout: RIG_MUTATION_TIMEOUT_MS },
    )
    const portraitUrl = readPortraitUrl(data)
    if (!portraitUrl) {
      throw new MeropeApiError(currentCopy().merope.portraitUploadFailed, 502)
    }
    return { portraitUrl }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.portraitUploadFailed)
  }
}

/** The owner's own picture for a full-body set. */
export async function uploadFullBodyPortrait(
  outfitId: string,
  image: Blob,
): Promise<{
  portraitUrl: string
}> {
  const body = new FormData()
  body.append(
    'image',
    image,
    image instanceof File ? image.name : 'uploaded-full-body.png',
  )
  try {
    const data = await apiService.post<{ portraitUrl?: unknown }>(
      `${PREFIX}${fullBodyPath(outfitId)}/portrait/upload`,
      body,
      { timeout: RIG_MUTATION_TIMEOUT_MS },
    )
    const portraitUrl = readPortraitUrl(data)
    if (!portraitUrl) {
      throw new MeropeApiError(currentCopy().merope.portraitUploadFailed, 502)
    }
    return { portraitUrl }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.portraitUploadFailed)
  }
}

export async function generateStickerAvatar(): Promise<{
  avatarUrl: string | null
}> {
  try {
    const data = await apiService.post<{ avatarUrl?: unknown }>(
      `${PREFIX}/avatar`,
      {},
      { timeout: PORTRAIT_GENERATION_TIMEOUT_MS },
    )
    return {
      avatarUrl:
        typeof data.avatarUrl === 'string' &&
        data.avatarUrl.trim()
          ? data.avatarUrl
          : null,
    }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.avatarFailed)
  }
}

export type SiteExpressionUrls = Partial<Record<AuthoredExpressionKind, string>>

/** Expression redraws of the current master portrait, cut into the rig at import. */
export async function listSiteExpressions(): Promise<SiteExpressionUrls> {
  try {
    const data = await apiService.get<{ expressions?: Record<string, unknown> }>(
      `${PREFIX}/expressions`,
    )
    return readExpressionUrls(data.expressions)
  } catch (reason) {
    throw meropeError(reason, currentCopy().merope.aiExpressionsFailed)
  }
}

export async function generateSiteExpression(
  kind: AuthoredExpressionKind,
): Promise<string> {
  try {
    const data = await apiService.post<{ url?: unknown }>(
      `${PREFIX}/expressions/${kind}`,
      {},
      { timeout: PORTRAIT_GENERATION_TIMEOUT_MS },
    )
    if (typeof data.url !== 'string' || !data.url.trim()) {
      throw new MeropeApiError(currentCopy().merope.aiExpressionsFailed, 502)
    }
    return data.url
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.aiExpressionsFailed)
  }
}

function readExpressionUrls(value: Record<string, unknown> | undefined): SiteExpressionUrls {
  const urls: SiteExpressionUrls = {}
  for (const kind of AUTHORED_EXPRESSION_KINDS) {
    const url = value?.[kind]
    if (typeof url === 'string' && url.trim()) urls[kind] = url
  }
  return urls
}

/**
 * Draws a full-body set's picture: redrawn from the bust set it was made
 * from, or from its design.
 */
export async function generateFullBodyPortrait(outfitId: string): Promise<{
  portraitUrl: string | null
  generationFingerprint: string | null
}> {
  try {
    const data = await apiService.post<{
      portraitUrl?: unknown
      generationFingerprint?: unknown
    }>(`${PREFIX}${fullBodyPath(outfitId)}/portrait`, {}, {
      timeout: PORTRAIT_GENERATION_TIMEOUT_MS,
    })
    return {
      portraitUrl: readPortraitUrl(data),
      generationFingerprint:
        typeof data.generationFingerprint === 'string' &&
        /^[0-9a-f]{64}$/iu.test(data.generationFingerprint)
          ? data.generationFingerprint.toLowerCase()
          : null,
    }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.fullBody.failed)
  }
}

export async function generateSitePortrait(
  prompt?: string,
  options?: { edit?: boolean },
): Promise<{
  portraitUrl: string | null
  generationFingerprint: string | null
}> {
  try {
    const data = await apiService.post<{
      portraitUrl?: unknown
      generationFingerprint?: unknown
    }>(
      `${PREFIX}/portrait`,
      { prompt, edit: options?.edit === true },
      { timeout: PORTRAIT_GENERATION_TIMEOUT_MS },
    )
    const fingerprint =
      typeof data.generationFingerprint === 'string' &&
      /^[0-9a-f]{64}$/iu.test(data.generationFingerprint)
        ? data.generationFingerprint.toLowerCase()
        : null
    return {
      portraitUrl: readPortraitUrl(data),
      generationFingerprint: fingerprint,
    }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.visualFailed)
  }
}
