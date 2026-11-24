import type { MeropeRigManifest, PreparedRigPsdImport, RigPsdImportOptions } from '@myriad/figure/rig'
import type { importMeropeRig, previewMeropeRigImport } from '../api'
import { diagnoseRig } from '@myriad/figure/rig'
import { userFacingError } from '../../../utils/userFacingError'

export type RigAssetCompileStage =
  | 'validate-source'
  | 'pack-atlas'
  | 'compile-preview'
  | 'persist-manifest'
  | 'analyze-capabilities'

export interface RigAssetCompileEvent {
  stage: RigAssetCompileStage
  status: 'started' | 'completed' | 'failed'
  error?: string
}

export interface ImportedRigAsset {
  manifest: MeropeRigManifest
  partCount: number
  report: ReturnType<typeof diagnoseRig>
}

export interface RigAssetPreflight extends ImportedRigAsset {
  prepared: PreparedRigPsdImport
  /** The search for its moving parts the server began with the preview; null when none was begun. */
  partSearch: string | null
}

/** Sends again the preview of what the Worker already made, when only that request failed. */
export type RigPreviewRetry = (
  onStage?: (event: RigAssetCompileEvent) => void,
  signal?: AbortSignal,
) => Promise<RigAssetPreflight>

/**
 * A preflight whose preview request failed keeps what the Worker made, by
 * the error it threw: asking again re-sends only the request.
 */
const previewRetries = new WeakMap<object, RigPreviewRetry>()

/**
 * Whether sending the same preview again can succeed: the request did not get
 * through, timed out, or the server failed. A refusal of what was sent (it
 * does not compile, the portrait changed meanwhile) would be refused again,
 * and each try pays for another chest estimate.
 */
/**
 * The most an atlas may weigh for the server to store it (its own limit on the
 * import's atlas field). The preview goes without the atlas, so the page holds
 * it to this before asking: a package that could never be stored is turned
 * away before anything is spent on analysing it.
 */
const MAX_ATLAS_BYTES = 20 * 1024 * 1024

function worthSendingAgain(error: object): boolean {
  const { status } = error as { status?: unknown }
  return typeof status === 'number' && (status === 0 || status === 408 || status >= 500)
}

/** The way to ask again for the preview this error ended, if the Worker's part was done and asking again can help. */
export function rigPreviewRetryOf(error: unknown): RigPreviewRetry | undefined {
  return typeof error === 'object' && error !== null ? previewRetries.get(error) : undefined
}

/** What preflight decides about a PSD import; the caller adds where and for which figure. */
export type RigPsdPrepareOptions = Pick<
  RigPsdImportOptions,
  'sourceMasterAssetId' | 'onStage' | 'sourceGenerationFingerprint' | 'signal' | 'expressions'
>

interface RigAssetPreflightDependencies {
  prepare: (file: File, options: RigPsdPrepareOptions) => Promise<PreparedRigPsdImport>
  preview: typeof previewMeropeRigImport
}

/** Parses, packs, server-compiles, migrates and diagnoses without persistence. */
export async function preflightRigAsset(
  file: File,
  sourceMasterAssetId: string,
  dependencies: RigAssetPreflightDependencies,
  onStage?: (event: RigAssetCompileEvent) => void,
  sourceGenerationFingerprint?: string,
  signal?: AbortSignal,
  expressions?: RigPsdImportOptions['expressions'],
): Promise<RigAssetPreflight> {
  signal?.throwIfAborted()
  emit(onStage, 'validate-source', 'started')
  let prepared: PreparedRigPsdImport
  let activeStage: RigAssetCompileStage = 'validate-source'
  try {
    prepared = await dependencies.prepare(file, {
      sourceMasterAssetId,
      onStage: (stage) => {
        if (stage === 'validated') {
          emit(onStage, 'validate-source', 'completed')
        } else if (stage === 'packing') {
          activeStage = 'pack-atlas'
          emit(onStage, 'pack-atlas', 'started')
        }
      },
      sourceGenerationFingerprint,
      signal,
      expressions,
    })
    signal?.throwIfAborted()
    if (activeStage === 'validate-source') {
      emit(onStage, 'validate-source', 'completed')
      emit(onStage, 'pack-atlas', 'started')
    }
    emit(onStage, 'pack-atlas', 'completed')
  } catch (error) {
    signal?.throwIfAborted()
    const message = errorMessage(error)
    emit(onStage, activeStage, 'failed', message)
    throw error
  }

  return previewPreparedRigAsset(prepared, dependencies.preview, onStage, signal)
}

/** Server-compiles and diagnoses what the Worker made; a failure here keeps it for another try. */
async function previewPreparedRigAsset(
  prepared: PreparedRigPsdImport,
  preview: RigAssetPreflightDependencies['preview'],
  onStage?: (event: RigAssetCompileEvent) => void,
  signal?: AbortSignal,
): Promise<RigAssetPreflight> {
  signal?.throwIfAborted()
  emit(onStage, 'compile-preview', 'started')
  try {
    if (prepared.atlas.size > MAX_ATLAS_BYTES) throw new Error('Rig atlas exceeds 20 MB')
    const { manifest, partSearch } = await preview(prepared.source, prepared.analysisReference)
    signal?.throwIfAborted()
    copyPreviewPlaybackProfiles(prepared.source, manifest)
    emit(onStage, 'compile-preview', 'completed')
    emit(onStage, 'analyze-capabilities', 'started')
    const report = diagnoseRig(manifest, prepared.motionExposure)
    emit(onStage, 'analyze-capabilities', 'completed')
    return { manifest, partCount: prepared.partCount, prepared, report, partSearch }
  } catch (error) {
    signal?.throwIfAborted()
    emit(onStage, 'compile-preview', 'failed', errorMessage(error))
    if (typeof error === 'object' && error !== null && worthSendingAgain(error)) {
      previewRetries.set(error, (retryStage, retrySignal) =>
        previewPreparedRigAsset(prepared, preview, retryStage, retrySignal))
    }
    throw error
  }
}

/** Preserve the one-shot preview analysis so persistence never calls AI again. */
function copyPreviewPlaybackProfiles(
  source: PreparedRigPsdImport['source'],
  manifest: MeropeRigManifest,
): void {
  const playback = manifest.anime25dPlayback
  if (!playback || !source.anime25dPlayback) return
  source.anime25dPlayback.chestProfile = { ...playback.chestProfile }
  source.anime25dPlayback.shellProfile = structuredClone(playback.shellProfile)
  if (playback.partProfile) source.anime25dPlayback.partProfile = structuredClone(playback.partProfile)
}

export async function persistRigAsset(
  preflight: RigAssetPreflight,
  upload: typeof importMeropeRig,
  onStage?: (event: RigAssetCompileEvent) => void,
): Promise<ImportedRigAsset> {
  emit(onStage, 'persist-manifest', 'started')
  try {
    const manifest = await upload(
      preflight.prepared.source,
      preflight.prepared.atlas,
      preflight.prepared.analysisReference,
    )
    emit(onStage, 'persist-manifest', 'completed')
    const report = diagnoseRig(manifest)
    return { manifest, partCount: preflight.partCount, report }
  } catch (error) {
    emit(onStage, 'persist-manifest', 'failed', errorMessage(error))
    throw error
  }
}

function emit(
  listener: ((event: RigAssetCompileEvent) => void) | undefined,
  stage: RigAssetCompileStage,
  status: RigAssetCompileEvent['status'],
  error?: string,
): void {
  listener?.({ stage, status, ...(error ? { error } : {}) })
}

function errorMessage(error: unknown): string {
  const fallback = error instanceof Error ? error.message : String(error)
  return userFacingError(error, fallback)
}
