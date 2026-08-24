import type {
  MeropeRigImportSource,
  MeropeRigManifest,
} from './rig/types'
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

export function isMeropeNotFound(error: unknown): boolean {
  return error instanceof MeropeApiError && error.status === 404
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

export async function getSiteFace(): Promise<SiteFace> {
  const response = await api.get<{
    manifest?: unknown
    portraitUrl?: unknown
    generationFingerprint?: unknown
    assetId?: unknown
  }>(`${PREFIX}/active`)
  if (response.status === 404) {
    return {
      manifest: null,
      portraitUrl: null,
      generationFingerprint: null,
      assetId: null,
    }
  }
  assertSuccess(response.status, response.data, 'Could not load site face')
  const manifest = isLiveMeropeManifest(response.data.manifest)
    ? response.data.manifest
    : null
  return {
    manifest,
    portraitUrl: readPortraitUrl(response.data),
    generationFingerprint:
      typeof response.data.generationFingerprint === 'string' &&
      /^[0-9a-f]{64}$/iu.test(response.data.generationFingerprint)
        ? response.data.generationFingerprint.toLowerCase()
        : null,
    assetId:
      typeof response.data.assetId === 'string' ? response.data.assetId : null,
  }
}

export async function getMeropeRig(): Promise<MeropeRigManifest | null> {
  return (await getSiteFace()).manifest
}

export async function getSeeThroughStatus(): Promise<SeeThroughStatus> {
  const response = await api.get<Partial<SeeThroughStatus>>(
    `${PREFIX}/see-through/status`,
  )
  assertSuccess(
    response.status,
    response.data,
    'Could not load See-through status',
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
        : 768,
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
    'Could not save Hugging Face token',
  )
  return {
    provider:
      typeof response.data.provider === 'string'
        ? response.data.provider
        : '24yearsold/see-through-demo',
    tokenConfigured: response.data.tokenConfigured === true,
    defaultResolution: 768,
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
        'See-through decomposition failed',
      )
    }
    if (!(response.data instanceof Blob) || response.data.size === 0) {
      throw new MeropeApiError('See-through returned an empty PSD', 502)
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
        'See-through decomposition failed',
      )
    }
    throw meropeError(reason, 'See-through decomposition failed')
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
    throw meropeError(reason, `Could not ${action} persona rig`)
  }
  assertSuccess(
    response.status,
    response.data,
    `Could not ${action} persona rig`,
  )
  if (!isRigManifest(response.data.manifest)) {
    throw new Error(`Persona rig ${action} manifest is invalid`)
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
    assertSuccess(response.status, response.data, 'Could not upload portrait')
    const portraitUrl = readPortraitUrl(response.data)
    if (!portraitUrl) {
      throw new MeropeApiError('Portrait upload did not return a URL', 502)
    }
    return { portraitUrl }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, 'Could not upload portrait')
  }
}

export async function generateSitePortrait(
  prompt?: string,
  options?: { edit?: boolean },
): Promise<{
  portraitUrl: string | null
}> {
  try {
    const response = await api.post<{ portraitUrl?: unknown }>(
      `${PREFIX}/portrait`,
      { prompt, edit: options?.edit === true },
      { timeout: PORTRAIT_GENERATION_TIMEOUT_MS },
    )
    assertSuccess(
      response.status,
      response.data,
      'Could not generate site portrait',
    )
    return { portraitUrl: readPortraitUrl(response.data) }
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, 'Could not generate site portrait')
  }
}
