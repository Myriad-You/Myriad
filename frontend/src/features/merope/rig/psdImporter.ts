import type { Layer, Psd } from 'ag-psd'
import { currentCopy } from '../../../i18n/localeCopy'
import type { PreparedAnime25DRigImport } from './anime25dImporter'
import {
  normalizeAnime25DLayerName,
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
  return prepareAnime25DRigPsd(
    psd,
    sourceMasterAssetId,
    onStage,
    sourceGenerationFingerprint,
  )
}

export async function compositePsdToPng(file: File): Promise<File> {
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
  const canvas = document.createElement('canvas')
  canvas.width = psd.width
  canvas.height = psd.height
  const context = canvas.getContext('2d')
  if (!context) throw new Error(currentCopy().merope.psdPreviewFailed)
  paintPsdLayers(context, psd.children || [])
  const blob = await new Promise<Blob>((resolve, reject) => {
    canvas.toBlob(
      (next) =>
        next
          ? resolve(next)
          : reject(new Error(currentCopy().merope.psdPreviewFailed)),
      'image/png',
    )
  })
  return new File([blob], 'uploaded-portrait.png', { type: 'image/png' })
}

function paintPsdLayers(
  context: CanvasRenderingContext2D,
  layers: Layer[],
): void {
  for (const layer of layers) {
    if (layer.hidden) continue
    if (layer.children) {
      paintPsdLayers(context, layer.children)
      continue
    }
    const pixels = layer.imageData
    if (!pixels?.data || !pixels.width || !pixels.height) continue
    const image = context.createImageData(pixels.width, pixels.height)
    image.data.set(pixels.data)
    const scratch = document.createElement('canvas')
    scratch.width = pixels.width
    scratch.height = pixels.height
    const scratchContext = scratch.getContext('2d')
    if (!scratchContext) continue
    scratchContext.putImageData(image, 0, 0)
    context.drawImage(scratch, layer.left ?? 0, layer.top ?? 0)
  }
}

export const normalizePsdLayerName = normalizeAnime25DLayerName

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
