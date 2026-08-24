import type {
  ImportedRigAsset,
  RigAssetCompileEvent,
  RigAssetPreflight,
} from './compiler'
import { importMeropeRig, previewMeropeRigImport } from '../api'
import { prepareRigPsdImport } from '../rig/psdImporter'
import {
  compileRigAsset,
  persistRigAsset,
  preflightRigAsset,
} from './compiler'

export type {
  ImportedRigAsset,
  RigAssetCompileEvent,
  RigAssetPreflight,
} from './compiler'

/** PSD parsing, atlas packing, validation, and upload form one transaction. */
export async function importRigPsdAsset(
  file: File,
  sourceMasterAssetId: string,
  dependencies: {
    prepare?: typeof prepareRigPsdImport
    preview?: typeof previewMeropeRigImport
    upload?: typeof importMeropeRig
    onStage?: (event: RigAssetCompileEvent) => void
  } = {},
  sourceGenerationFingerprint?: string,
): Promise<ImportedRigAsset> {
  return compileRigAsset(
    file,
    sourceMasterAssetId,
    {
      prepare: dependencies.prepare || prepareRigPsdImport,
      preview: dependencies.preview || previewMeropeRigImport,
      upload: dependencies.upload || importMeropeRig,
    },
    dependencies.onStage,
    sourceGenerationFingerprint,
  )
}

export async function preflightRigPsdAsset(
  file: File,
  sourceMasterAssetId: string,
  onStage?: (event: RigAssetCompileEvent) => void,
  sourceGenerationFingerprint?: string,
): Promise<RigAssetPreflight> {
  return preflightRigAsset(
    file,
    sourceMasterAssetId,
    { prepare: prepareRigPsdImport, preview: previewMeropeRigImport },
    onStage,
    sourceGenerationFingerprint,
  )
}

export async function commitRigPsdAsset(
  preflight: RigAssetPreflight,
  onStage?: (event: RigAssetCompileEvent) => void,
): Promise<ImportedRigAsset> {
  return persistRigAsset(preflight, importMeropeRig, onStage)
}
