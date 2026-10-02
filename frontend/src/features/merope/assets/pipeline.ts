import type {
  ImportedRigAsset,
  RigAssetCompileEvent,
  RigAssetPreflight,
} from './compiler'
import {
  importFullBodyRig,
  importMeropeRig,
  previewFullBodyRigImport,
  previewMeropeRigImport,
} from '../api'
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
  expressions?: Parameters<typeof prepareRigPsdImport>[5],
): Promise<RigAssetPreflight> {
  return preflightRigAsset(
    file,
    sourceMasterAssetId,
    { prepare: prepareRigPsdImport, preview: previewMeropeRigImport },
    onStage,
    sourceGenerationFingerprint,
    signal,
    expressions,
  )
}

export async function commitRigPsdAsset(
  preflight: RigAssetPreflight,
  onStage?: (event: RigAssetCompileEvent) => void,
): Promise<ImportedRigAsset> {
  return persistRigAsset(preflight, importMeropeRig, onStage)
}

/** The worn outfit's full figure, imported beside its bust and stored in its own slot. */
export async function preflightFullBodyPsdAsset(
  file: File,
  sourceMasterAssetId: string,
  sourceGenerationFingerprint?: string,
): Promise<RigAssetPreflight> {
  return preflightRigAsset(
    file,
    sourceMasterAssetId,
    {
      prepare: (psd, master, onStage, fingerprint, signal) =>
        prepareRigPsdImport(psd, master, onStage, fingerprint, signal, [], 'fullBody'),
      preview: previewFullBodyRigImport,
    },
    undefined,
    sourceGenerationFingerprint,
  )
}

export async function commitFullBodyPsdAsset(
  preflight: RigAssetPreflight,
): Promise<ImportedRigAsset> {
  return persistRigAsset(preflight, importFullBodyRig)
}
