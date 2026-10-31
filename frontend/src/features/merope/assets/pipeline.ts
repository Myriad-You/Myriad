import type { PreparedRigPsdImport, RigPsdImportOptions } from '@myriad/figure/rig'
import type {
  ImportedRigAsset,
  RigAssetCompileEvent,
  RigAssetPreflight,
} from './compiler'
import { FigureError } from '@myriad/figure'
import { prepareRigPsdImport } from '@myriad/figure/rig'
import { API_URL } from '../../../config'
import { currentCopy, formatCurrent } from '../../../i18n/localeCopy'
import {
  fullBodyRigImport,
  getFullBodyFigurePicture,
  getFullBodySkeleton,
  getSiteSkeleton,
  importMeropeRig,
  previewMeropeRigImport,
} from '../api'
import { persistRigAsset, preflightRigAsset } from './compiler'

export type {
  ImportedRigAsset,
  RigAssetCompileEvent,
  RigAssetPreflight,
} from './compiler'

/** A figure error in the site's language; any other error passes through. */
function localizeFigureError(error: unknown): unknown {
  if (!(error instanceof FigureError)) return error
  return new Error(formatCurrent(currentCopy().merope[error.code], error.params), { cause: error })
}

/** The figure's import, against this site's API and in its language. */
async function prepareSitePsd(
  file: File,
  options: Omit<RigPsdImportOptions, 'apiUrl'>,
): Promise<PreparedRigPsdImport> {
  try {
    return await prepareRigPsdImport(file, { ...options, apiUrl: API_URL })
  } catch (error) {
    throw localizeFigureError(error)
  }
}

export async function preflightRigPsdAsset(
  file: File,
  sourceMasterAssetId: string,
  onStage?: (event: RigAssetCompileEvent) => void,
  sourceGenerationFingerprint?: string,
  signal?: AbortSignal,
  expressions?: RigPsdImportOptions['expressions'],
): Promise<RigAssetPreflight> {
  // Bound to the joints found on the worn bust, when that is this portrait.
  const skeleton = await getSiteSkeleton()
  return preflightRigAsset(
    file,
    sourceMasterAssetId,
    {
      prepare: (psd, options) =>
        prepareSitePsd(psd, { ...options, profile: 'bust', skeleton: skeleton ?? undefined }),
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
  const [skeleton, picture] = await Promise.all([
    getFullBodySkeleton(outfitId),
    getFullBodyFigurePicture(outfitId),
  ])
  // A figure decomposed in tiles was cut from its enlarged picture: checked against that.
  const referenceUrl = picture ? URL.createObjectURL(picture) : undefined
  try {
    return await preflightRigAsset(
      file,
      sourceMasterAssetId,
      {
        prepare: (psd, options) =>
          prepareSitePsd(psd, {
            ...options,
            expressions: [],
            profile: 'fullBody',
            skeleton: skeleton ?? undefined,
            referenceUrl,
          }),
        preview: fullBodyRigImport(outfitId).preview,
      },
      onStage,
      sourceGenerationFingerprint,
      signal,
    )
  } finally {
    if (referenceUrl) URL.revokeObjectURL(referenceUrl)
  }
}

export async function commitFullBodyPsdAsset(
  outfitId: string,
  preflight: RigAssetPreflight,
  onStage?: (event: RigAssetCompileEvent) => void,
): Promise<ImportedRigAsset> {
  return persistRigAsset(preflight, fullBodyRigImport(outfitId).commit, onStage)
}
