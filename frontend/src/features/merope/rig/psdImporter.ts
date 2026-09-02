import type { Layer, Psd } from 'ag-psd'
import type {
  Anime25DSourceReference,
  PreparedAnime25DRigImport,
} from './anime25dImporter'
import { currentCopy } from '../../../i18n/localeCopy'
import {
  prepareAnime25DRigPsd,
} from './anime25dImporter'

const MAX_PSD_BYTES = 32 * 1024 * 1024
const MAX_DOCUMENT_EDGE = 2048
const MAX_LAYER_COUNT = 64

export type PreparedRigPsdImport = PreparedAnime25DRigImport

/**
 * The production importer has one contract: a layered upper-body FaceRig.
 * Full-body articulated PSDs are deliberately not compiled by this path.
 */
export async function prepareRigPsdImport(
  file: File,
  sourceMasterAssetId: string,
  onStage?: (stage: 'validated' | 'packing') => void,
  sourceGenerationFingerprint?: string,
): Promise<PreparedRigPsdImport> {
  if (!sourceMasterAssetId) throw new Error(currentCopy().merope.psdNeedAsset)
  if (file.size <= 0 || file.size > MAX_PSD_BYTES) {
    throw new Error(currentCopy().merope.psdTooLarge)
  }
  const { readPsd } = await import('ag-psd')
  const psd = readPsd(await file.arrayBuffer(), {
    useImageData: true,
    skipCompositeImageData: true,
    skipThumbnail: true,
    skipLinkedFilesData: true,
    totalMemoryLimit: 128 * 1024 * 1024,
  })
  validateFaceRigDocument(psd)
  const sourceReference = await alignSourceMasterToSeeThroughDocument(
    sourceMasterAssetId,
    psd.width,
    psd.height,
  )
  return prepareAnime25DRigPsd(
    psd,
    sourceMasterAssetId,
    onStage,
    sourceGenerationFingerprint,
    sourceReference,
  )
}

/**
 * See-through centers a non-square input on a transparent square, then scales
 * that square to the requested PSD resolution. Repeating that transform gives
 * the importer a pixel-aligned copy of the original visible composition.
 */
async function alignSourceMasterToSeeThroughDocument(
  sourceMasterAssetId: string,
  width: number,
  height: number,
): Promise<Anime25DSourceReference | undefined> {
  if (width !== height) return undefined
  const response = await fetch(sourceMasterAssetId)
  if (!response.ok) throw new Error(currentCopy().merope.psdPreviewFailed)
  const bitmap = await createImageBitmap(await response.blob())
  try {
    const squareEdge = Math.max(bitmap.width, bitmap.height)
    const paddingX = Math.floor((squareEdge - bitmap.width) / 2)
    const paddingY = Math.floor((squareEdge - bitmap.height) / 2)
    const scaleX = width / squareEdge
    const scaleY = height / squareEdge
    const canvas = document.createElement('canvas')
    canvas.width = width
    canvas.height = height
    const context = canvas.getContext('2d')
    if (!context) throw new Error(currentCopy().merope.psdPreviewFailed)
    context.imageSmoothingEnabled = true
    context.imageSmoothingQuality = 'high'
    context.drawImage(
      bitmap,
      paddingX * scaleX,
      paddingY * scaleY,
      bitmap.width * scaleX,
      bitmap.height * scaleY,
    )
    return {
      width,
      height,
      data: context.getImageData(0, 0, width, height).data,
    }
  } finally {
    bitmap.close()
  }
}

function validateFaceRigDocument(psd: Psd): void {
  if (
    psd.width < 256 ||
    psd.height < 256 ||
    psd.width > MAX_DOCUMENT_EDGE ||
    psd.height > MAX_DOCUMENT_EDGE ||
    (psd.bitsPerChannel ?? 8) !== 8
  ) {
    throw new Error(currentCopy().merope.psdSpecInvalid)
  }
  const layerCount = countVisiblePixelLayers(psd.children || [])
  if (layerCount === 0 || layerCount > MAX_LAYER_COUNT) {
    throw new Error(currentCopy().merope.psdLayerCountInvalid)
  }
}

function countVisiblePixelLayers(layers: Layer[]): number {
  let count = 0
  for (const layer of layers) {
    if (layer.hidden) continue
    if (layer.children) count += countVisiblePixelLayers(layer.children)
    else if (layer.imageData) count += 1
  }
  return count
}
