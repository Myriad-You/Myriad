import type { importMeropeRig, previewMeropeRigImport } from '../api'
import type {
  PreparedRigPsdImport,
  prepareRigPsdImport,
} from '../rig/psdImporter'
import type { MeropeRigManifest } from '../rig/types'
import { diagnoseRig } from '../rig/diagnostics'

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
}

export interface RigAssetCompilerDependencies {
  prepare: typeof prepareRigPsdImport
  preview: typeof previewMeropeRigImport
  upload: typeof importMeropeRig
}

/**
 * Executes the PSD → atlas → persisted manifest DAG with observable stages.
 * A failed stage is terminal, so callers never mistake a packed atlas for a
 * successfully stored character asset.
 */
export async function compileRigAsset(
  file: File,
  sourceMasterAssetId: string,
  dependencies: RigAssetCompilerDependencies,
  onStage?: (event: RigAssetCompileEvent) => void,
  sourceGenerationFingerprint?: string,
): Promise<ImportedRigAsset> {
  const preflight = await preflightRigAsset(
    file,
    sourceMasterAssetId,
    dependencies,
    onStage,
    sourceGenerationFingerprint,
  )
  return persistRigAsset(preflight, dependencies.upload, onStage)
}

/** Parses, packs, server-compiles, migrates and diagnoses without persistence. */
export async function preflightRigAsset(
  file: File,
  sourceMasterAssetId: string,
  dependencies: Pick<RigAssetCompilerDependencies, 'prepare' | 'preview'>,
  onStage?: (event: RigAssetCompileEvent) => void,
  sourceGenerationFingerprint?: string,
): Promise<RigAssetPreflight> {
  emit(onStage, 'validate-source', 'started')
  let prepared: Awaited<ReturnType<typeof prepareRigPsdImport>>
  let activeStage: RigAssetCompileStage = 'validate-source'
  try {
    prepared = await dependencies.prepare(
      file,
      sourceMasterAssetId,
      (stage) => {
        if (stage === 'validated') {
          emit(onStage, 'validate-source', 'completed')
        } else if (stage === 'packing') {
          activeStage = 'pack-atlas'
          emit(onStage, 'pack-atlas', 'started')
        }
      },
      sourceGenerationFingerprint,
    )
    if (activeStage === 'validate-source') {
      emit(onStage, 'validate-source', 'completed')
      emit(onStage, 'pack-atlas', 'started')
    }
    emit(onStage, 'pack-atlas', 'completed')
  } catch (error) {
    const message = errorMessage(error)
    emit(onStage, activeStage, 'failed', message)
    throw error
  }

  emit(onStage, 'compile-preview', 'started')
  try {
    const manifest = await dependencies.preview(
      prepared.source,
      prepared.atlas,
      prepared.analysisReference,
    )
    copyPreviewChestProfile(prepared.source, manifest)
    emit(onStage, 'compile-preview', 'completed')
    emit(onStage, 'analyze-capabilities', 'started')
    const report = diagnoseRig(manifest)
    emit(onStage, 'analyze-capabilities', 'completed')
    return { manifest, partCount: prepared.partCount, prepared, report }
  } catch (error) {
    emit(onStage, 'compile-preview', 'failed', errorMessage(error))
    throw error
  }
}

/** Preserve the one-shot preview analysis so persistence never calls AI again. */
function copyPreviewChestProfile(
  source: PreparedRigPsdImport['source'],
  manifest: MeropeRigManifest,
): void {
  const profile = manifest.anime25dPlayback?.chestProfile
  if (!profile || !source.anime25dPlayback) return
  source.anime25dPlayback.chestProfile = { ...profile }
}

/** Commits the exact source and atlas that passed preflight. */
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
  return error instanceof Error ? error.message : String(error)
}
