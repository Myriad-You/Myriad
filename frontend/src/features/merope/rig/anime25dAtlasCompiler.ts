import type {
  PreparedLayer,
  RasterLayer,
  RigCanvasFrame,
} from './anime25dImportTypes'
import { currentCopy } from '../../../i18n/localeCopy'

const ATLAS_PADDING = 8
const MAX_ATLAS_EDGE = 8192
const MIN_ATLAS_EDGE = 256

export async function packAnime25DAtlas(
  frame: RigCanvasFrame,
  layers: RasterLayer[],
): Promise<{
  atlas: Blob
  analysisReference: Blob
  layers: PreparedLayer[]
  width: number
  height: number
}> {
  const places: Array<{ x: number; y: number }> = []
  let cursorX = ATLAS_PADDING
  let cursorY = ATLAS_PADDING
  let rowHeight = 0
  let packedWidth = ATLAS_PADDING
  let packedHeight = ATLAS_PADDING
  for (const layer of layers) {
    const drawWidth = Math.max(1, layer.width)
    const drawHeight = Math.max(1, layer.height)
    if (drawWidth + ATLAS_PADDING * 2 > MAX_ATLAS_EDGE) {
      throw new Error(
        currentCopy()
          .merope.anime25dLayerTooWide.replace('{id}', layer.id)
          .replace('{max}', String(MAX_ATLAS_EDGE)),
      )
    }
    if (cursorX + drawWidth + ATLAS_PADDING > MAX_ATLAS_EDGE) {
      cursorX = ATLAS_PADDING
      cursorY += rowHeight + ATLAS_PADDING
      rowHeight = 0
    }
    if (cursorY + drawHeight + ATLAS_PADDING > MAX_ATLAS_EDGE) {
      throw new Error(
        currentCopy().merope.anime25dAtlasOverflow.replace(
          '{max}',
          String(MAX_ATLAS_EDGE),
        ),
      )
    }
    places.push({ x: cursorX, y: cursorY })
    cursorX += drawWidth + ATLAS_PADDING
    rowHeight = Math.max(rowHeight, drawHeight)
    packedWidth = Math.max(packedWidth, cursorX)
    packedHeight = Math.max(packedHeight, cursorY + drawHeight + ATLAS_PADDING)
  }
  packedWidth = Math.max(MIN_ATLAS_EDGE, packedWidth)
  packedHeight = Math.max(MIN_ATLAS_EDGE, packedHeight)
  const atlas = document.createElement('canvas')
  atlas.width = packedWidth
  atlas.height = packedHeight
  const context = requiredContext(atlas)
  const analysisCanvas = document.createElement('canvas')
  analysisCanvas.width = Math.max(1, Math.round(frame.width))
  analysisCanvas.height = Math.max(1, Math.round(frame.height))
  const analysisContext = requiredContext(analysisCanvas)
  const analysisScaleX = analysisCanvas.width / Math.max(1, frame.width)
  const analysisScaleY = analysisCanvas.height / Math.max(1, frame.height)
  const layerCanvas = document.createElement('canvas')
  const prepared: PreparedLayer[] = []
  for (const [index, layer] of layers.entries()) {
    const drawX = places[index].x
    const drawY = places[index].y
    layerCanvas.width = layer.width
    layerCanvas.height = layer.height
    const imageBytes = new Uint8ClampedArray(layer.data.length)
    imageBytes.set(layer.data)
    requiredContext(layerCanvas).putImageData(
      new ImageData(imageBytes, layer.width, layer.height),
      0,
      0,
    )
    context.drawImage(layerCanvas, drawX, drawY)
    if (visibleInAnalysisReference(layer)) {
      analysisContext.drawImage(
        layerCanvas,
        (layer.left - frame.x) * analysisScaleX,
        (layer.top - frame.y) * analysisScaleY,
        layer.width * analysisScaleX,
        layer.height * analysisScaleY,
      )
    }
    const bounds = {
      x: (layer.left - frame.x) / frame.width,
      y: (layer.top - frame.y) / frame.width,
      width: layer.width / frame.width,
      height: layer.height / frame.width,
    }
    prepared.push({
      ...layer,
      bounds,
      textureBounds: {
        x: drawX / packedWidth,
        y: drawY / packedHeight,
        width: layer.width / packedWidth,
        height: layer.height / packedHeight,
      },
      strands:
        layer.role === 'front-hair' || layer.role === 'back-hair'
          ? (layer.documentStrands || []).map((strand) => ({
              x: (strand.x - frame.x) / frame.width,
              rootY: (strand.rootY - frame.y) / frame.width,
              tipY: (strand.tipY - frame.y) / frame.width,
            }))
          : [],
    })
  }
  const [blob, analysisReference] = await Promise.all([
    canvasPng(atlas),
    canvasPng(analysisCanvas),
  ])
  return {
    atlas: blob,
    analysisReference,
    layers: prepared,
    width: packedWidth,
    height: packedHeight,
  }
}

function visibleInAnalysisReference(layer: RasterLayer): boolean {
  if (
    layer.role === 'maniac-eye-shadow' ||
    layer.role === 'maniac-mouth-shadow' ||
    layer.role === 'iris-silly' ||
    layer.role === 'lovestruck-heart' ||
    layer.role === 'lovestruck-face-effect' ||
    layer.role === 'lovestruck-drool' ||
    layer.role === 'anger-mark' ||
    layer.role === 'speechless-sweat'
  ) {
    return false
  }
  if (
    (layer.slot === 'eye-left' || layer.slot === 'eye-right') &&
    layer.variant !== 'open'
  ) {
    return false
  }
  return layer.slot !== 'mouth' || layer.variant === 'closed'
}

function canvasPng(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise<Blob>((resolve, reject) =>
    canvas.toBlob(
      (value) =>
        value
          ? resolve(value)
          : reject(new Error(currentCopy().merope.rigAtlasFailed)),
      'image/png',
    ),
  )
}

function requiredContext(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const context = canvas.getContext('2d', { willReadFrequently: true })
  if (!context) throw new Error(currentCopy().merope.canvasUnsupported)
  return context
}
