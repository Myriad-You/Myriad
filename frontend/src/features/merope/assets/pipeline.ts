import type {
  ImportedRigAsset,
  RigAssetCompileEvent,
  RigAssetPreflight,
} from './compiler'
import { importMeropeRig, previewMeropeRigImport } from '../api'
import { prepareRigPsdImport } from '../rig/psdImporter'
import { persistRigAsset, preflightRigAsset } from './compiler'

export type {
  ImportedRigAsset,
  RigAssetCompileEvent,
  RigAssetPreflight,
} from './compiler'

export async function preflightRigPsdAsset(
  file: File,
  sourceMasterAssetId: string,
  onStage?: (event: RigAssetCompileEvent) => void,
  sourceGenerationFingerprint?: string,
  signal?: AbortSignal,
): Promise<RigAssetPreflight> {
  return preflightRigAsset(
    file,
    sourceMasterAssetId,
    { prepare: prepareRigPsdImport, preview: previewMeropeRigImport },
    onStage,
    sourceGenerationFingerprint,
    signal,
  )
}

export async function commitRigPsdAsset(
  preflight: RigAssetPreflight,
  onStage?: (event: RigAssetCompileEvent) => void,
): Promise<ImportedRigAsset> {
  return persistRigAsset(preflight, importMeropeRig, onStage)
}
