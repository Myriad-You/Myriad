import type { MeropeRigImportSource, MeropeRigManifest } from './rig/types'
import { currentCopy } from '../../i18n/localeCopy'
import api from '../../lib/api'
import { httpStatusMessage } from '../../utils/userFacingError'
import { isLiveMeropeManifest, isRigManifest } from './rig/types'

const PREFIX = '/api/merope/rig'
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

function assertSuccess(status: number, data: unknown, _fallback: string): void {
  if (status < 400) return
  const payload =
    data && typeof data === 'object' ? (data as Record<string, unknown>) : {}
  throw new MeropeApiError(
    typeof payload.error === 'string'
      ? payload.error
      : typeof payload.message === 'string'
        ? payload.message
        : typeof data === 'string' && data.trim()
          ? data
          : httpStatusMessage(status),
    status,
    payloadCode(payload),
  )
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
  const response = (
    reason as { response?: { status?: unknown; data?: unknown } } | null
  )?.response
  const resolvedStatus =
    typeof response?.status === 'number' ? response.status : status
  const payload =
    response?.data && typeof response.data === 'object'
      ? (response.data as Record<string, unknown>)
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
  siteFaceInflight = loadSiteFace().finally(() => {
    siteFaceInflight = null
  })
  return siteFaceInflight
}

async function loadSiteFace(): Promise<SiteFace> {
  const response = await api.get<{
    manifest?: unknown
    portraitUrl?: unknown
    generationFingerprint?: unknown
    assetId?: unknown
  }>(`${PREFIX}/active`)
  return readFaceResponse(response.status, response.data)
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
  const response = await api.get<{
    manifest?: unknown
    portraitUrl?: unknown
    generationFingerprint?: unknown
    assetId?: unknown
  }>(`/api/agent/wardrobe/${encodeURIComponent(id)}/face`)
  return readFaceResponse(response.status, response.data)
}

function readFaceResponse(
  status: number,
  data: {
    manifest?: unknown
    portraitUrl?: unknown
    generationFingerprint?: unknown
    assetId?: unknown
  },
): SiteFace {
  if (status === 404) {
    return {
      manifest: null,
      portraitUrl: null,
      generationFingerprint: null,
      assetId: null,
    }
  }
  assertSuccess(status, data, currentCopy().merope.loadFailed)
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
  const response = await api.get<Partial<SeeThroughStatus>>(
    `${PREFIX}/see-through/status`,
  )
  assertSuccess(
    response.status,
    response.data,
    currentCopy().merope.seeThroughStatusFailed,
  )
  return {
    provider:
      typeof response.data.provider === 'string'
        ? response.data.provider
        : '24yearsold/see-through-demo',
    tokenConfigured: response.data.tokenConfigured === true,
    defaultResolution:
      typeof response.data.defaultResolution === 'number'
        ? response.data.defaultResolution
        : 1280,
    splitArmsAndLegs: response.data.splitArmsAndLegs !== false,
  }
}

export async function updateSeeThroughToken(
  token: string,
): Promise<SeeThroughStatus> {
  const response = await api.patch<Partial<SeeThroughStatus>>(
    `${PREFIX}/see-through/token`,
    { token },
  )
  assertSuccess(
    response.status,
    response.data,
    currentCopy().merope.motionSeeThroughTokenFailed,
  )
  return {
    provider:
      typeof response.data.provider === 'string'
        ? response.data.provider
        : '24yearsold/see-through-demo',
    tokenConfigured: response.data.tokenConfigured === true,
    defaultResolution: 1280,
    splitArmsAndLegs: true,
  }
}

async function binaryApiError(
  status: number,
  data: unknown,
  fallback: string,
): Promise<MeropeApiError> {
  let payload: unknown = data
  if (typeof Blob !== 'undefined' && data instanceof Blob) {
    try {
      const text = await data.text()
      payload = text ? JSON.parse(text) : null
    } catch {
      payload = null
    }
  }
  const body =
    payload && typeof payload === 'object'
      ? (payload as Record<string, unknown>)
      : {}
  return new MeropeApiError(
    typeof body.error === 'string'
      ? body.error
      : typeof body.message === 'string'
        ? body.message
        : fallback,
    status,
    payloadCode(body),
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
    const response = await api.post<Blob>(
      `${PREFIX}/see-through/decompose`,
      input,
      {
        responseType: 'blob',
        timeout: SEE_THROUGH_TIMEOUT_MS,
      },
    )
    if (response.status >= 400) {
      throw await binaryApiError(
        response.status,
        response.data,
        currentCopy().merope.motionSeeThroughUpstream,
      )
    }
    if (!(response.data instanceof Blob) || response.data.size === 0) {
      throw new MeropeApiError(
        currentCopy().merope.motionSeeThroughUpstream,
        502,
      )
    }
    return new File([response.data], 'see-through.psd', {
      type: 'image/vnd.adobe.photoshop',
    })
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    const response = (
      reason as { response?: { status?: unknown; data?: unknown } } | null
    )?.response
    if (typeof response?.status === 'number') {
      throw await binaryApiError(
        response.status,
        response.data,
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
  let response
  try {
    response = await api.post<{ manifest: unknown }>(`${PREFIX}${path}`, body, {
      // The shared Axios instance defaults to application/json. Clearing it is
      // required so the browser can generate the multipart boundary.
      headers: { 'Content-Type': undefined },
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
  assertSuccess(
    response.status,
    response.data,
    action === 'commit'
      ? currentCopy().merope.rigCommitFailed
      : currentCopy().merope.rigImportFailed,
  )
  if (!isRigManifest(response.data.manifest)) {
    throw new Error(currentCopy().merope.rigCompileFailed)
  }
  return response.data.manifest
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
    const response = await api.post<{ portraitUrl?: unknown }>(
      `${PREFIX}/portrait/upload`,
      body,
      {
        headers: { 'Content-Type': undefined },
        timeout: RIG_MUTATION_TIMEOUT_MS,
      },
    )
    assertSuccess(
      response.status,
      response.data,
      currentCopy().merope.portraitUploadFailed,
    )
    const portraitUrl = readPortraitUrl(response.data)
    if (!portraitUrl) {
      throw new MeropeApiError(currentCopy().merope.portraitUploadFailed, 502)
    }
    return { portraitUrl }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.portraitUploadFailed)
  }
}

/**
 * 生成人设的 Q 版贴纸头像。身份锚是已确认的主立绘，所以没有主立绘时后端会
 * 直接拒（`portrait_required`）；换主立绘会把旧头像清掉，需要重新生成。
 */
export async function generateStickerAvatar(): Promise<{
  avatarUrl: string | null
}> {
  try {
    const response = await api.post<{ avatarUrl?: unknown }>(
      `${PREFIX}/avatar`,
      {},
      { timeout: PORTRAIT_GENERATION_TIMEOUT_MS },
    )
    assertSuccess(
      response.status,
      response.data,
      currentCopy().merope.avatarFailed,
    )
    return {
      avatarUrl:
        typeof response.data.avatarUrl === 'string' &&
        response.data.avatarUrl.trim()
          ? response.data.avatarUrl
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
    const response = await api.post<{
      portraitUrl?: unknown
      generationFingerprint?: unknown
    }>(
      `${PREFIX}/portrait`,
      { prompt, edit: options?.edit === true },
      { timeout: PORTRAIT_GENERATION_TIMEOUT_MS },
    )
    assertSuccess(
      response.status,
      response.data,
      currentCopy().merope.visualFailed,
    )
    const fingerprint =
      typeof response.data.generationFingerprint === 'string' &&
      /^[0-9a-f]{64}$/iu.test(response.data.generationFingerprint)
        ? response.data.generationFingerprint.toLowerCase()
        : null
    return {
      portraitUrl: readPortraitUrl(response.data),
      generationFingerprint: fingerprint,
    }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.visualFailed)
  }
}
