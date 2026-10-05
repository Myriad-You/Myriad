import type {
  ImportedRigAsset,
  RigAssetCompileEvent,
  RigAssetPreflight,
} from './compiler'
import {
  fullBodyRigImport,
  getFullBodySkeleton,
  getSiteSkeleton,
  importMeropeRig,
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
  // Bound to the joints found on the worn bust, when that is this portrait.
  const skeleton = await getSiteSkeleton()
  return preflightRigAsset(
    file,
    sourceMasterAssetId,
    {
      prepare: (psd, master, onPrepareStage, fingerprint, prepareSignal, references) =>
        prepareRigPsdImport(
          psd,
          master,
          onPrepareStage,
          fingerprint,
          prepareSignal,
          references,
          'bust',
          skeleton ?? undefined,
        ),
      preview: previewMeropeRigImport,
    },
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

/** A full-body set's picture, imported into that set. */
export async function preflightFullBodyPsdAsset(
  outfitId: string,
  file: File,
  sourceMasterAssetId: string,
  sourceGenerationFingerprint?: string,
  onStage?: (event: RigAssetCompileEvent) => void,
  signal?: AbortSignal,
): Promise<RigAssetPreflight> {
  const skeleton = await getFullBodySkeleton(outfitId)
  return preflightRigAsset(
    file,
    sourceMasterAssetId,
    {
      prepare: (psd, master, onStage, fingerprint, signal) =>
        prepareRigPsdImport(
          psd,
          master,
          onStage,
          fingerprint,
          signal,
          [],
          'fullBody',
          skeleton ?? undefined,
        ),
      preview: fullBodyRigImport(outfitId).preview,
    },
    onStage,
    sourceGenerationFingerprint,
    signal,
  )
}

export async function commitFullBodyPsdAsset(
  outfitId: string,
  preflight: RigAssetPreflight,
  onStage?: (event: RigAssetCompileEvent) => void,
): Promise<ImportedRigAsset> {
  return persistRigAsset(preflight, fullBodyRigImport(outfitId).commit, onStage)
}
