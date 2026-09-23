import type { PoseCorrection } from './anime25drig/poseCorrections'
import type { MeropeRigImportSource, MeropeRigManifest } from './rig/types'
import { currentCopy } from '../../i18n/localeCopy'
import { ApiError, apiService } from '../../services/api'
import { isLiveMeropeManifest, isRigManifest } from './rig/types'

const PREFIX = '/merope/rig'
const RIG_MUTATION_TIMEOUT_MS = 6 * 60 * 1000
/** Keep in sync with MEROPE_PROXY_TIMEOUT_MS and get_long_running_client. */
const PORTRAIT_GENERATION_TIMEOUT_MS = 15 * 60 * 1000
const SEE_THROUGH_TIMEOUT_MS = 360_000

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

function meropeError(
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
  return {
    provider:
      typeof data.provider === 'string'
        ? data.provider
        : '24yearsold/see-through-demo',
    tokenConfigured: data.tokenConfigured === true,
    defaultResolution:
      typeof data.defaultResolution === 'number'
        ? data.defaultResolution
        : 1280,
    splitArmsAndLegs: data.splitArmsAndLegs !== false,
  }
}

export async function updateSeeThroughToken(
  token: string,
): Promise<SeeThroughStatus> {
  const data = await apiService.patch<Partial<SeeThroughStatus>>(
    `${PREFIX}/see-through/token`,
    { token },
  )
  return {
    provider:
      typeof data.provider === 'string'
        ? data.provider
        : '24yearsold/see-through-demo',
    tokenConfigured: data.tokenConfigured === true,
    defaultResolution: 1280,
    splitArmsAndLegs: true,
  }
}

/** The PSD endpoint's failures carry no useful HTTP status text; prefer the domain fallback. */
function seeThroughError(status: number, body: unknown, fallback: string): MeropeApiError {
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

export async function decomposeSitePortraitWithSeeThrough(input: {
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  resolution?: number
  seed?: number
  splitArmsAndLegs?: boolean
}): Promise<File> {
  try {
    const psd = await apiService.post<Blob>(
      `${PREFIX}/see-through/decompose`,
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
