import type { Anime25DImportCopy } from './anime25dImportCopy'
import type {
  PreparedLayer,
  RasterLayer,
  RigCanvasFrame,
} from './anime25dImportTypes'
import { trimRaster } from './anime25dRaster'
import { formatTemplate } from './formatTemplate'

const ATLAS_PADDING = 8
const MAX_ATLAS_EDGE = 8192
const MIN_ATLAS_EDGE = 256
/** Prefer staying under common mobile MAX_TEXTURE_SIZE when the sprites fit. */
const PREFERRED_ATLAS_EDGE = 4096

export interface AtlasSpriteSize {
  id: string
  width: number
  height: number
}

export function layoutAnime25DAtlas(
  layers: AtlasSpriteSize[],
  copy: Pick<
    Anime25DImportCopy,
    'anime25dLayerTooWide' | 'anime25dAtlasOverflow'
  >,
): { places: Array<{ x: number; y: number }>; width: number; height: number } {
  if (layers.length === 0) {
    return {
      places: [],
      width: MIN_ATLAS_EDGE,
      height: MIN_ATLAS_EDGE,
    }
  }
  let maxSprite = 0
  let area = 0
  for (const layer of layers) {
    const drawWidth = Math.max(1, layer.width)
    const drawHeight = Math.max(1, layer.height)
    if (
      drawWidth + ATLAS_PADDING * 2 > MAX_ATLAS_EDGE ||
      drawHeight + ATLAS_PADDING * 2 > MAX_ATLAS_EDGE
    ) {
      throw new Error(
        formatTemplate(copy.anime25dLayerTooWide, {
          id: layer.id,
          max: MAX_ATLAS_EDGE,
        }),
      )
    }
    maxSprite = Math.max(
      maxSprite,
      drawWidth + ATLAS_PADDING * 2,
      drawHeight + ATLAS_PADDING * 2,
    )
    area += (drawWidth + ATLAS_PADDING) * (drawHeight + ATLAS_PADDING)
  }
  const square = Math.ceil(Math.sqrt(area + ATLAS_PADDING * ATLAS_PADDING))
  let edge = Math.min(
    MAX_ATLAS_EDGE,
    Math.max(MIN_ATLAS_EDGE, maxSprite, square),
  )
  if (edge > PREFERRED_ATLAS_EDGE && maxSprite <= PREFERRED_ATLAS_EDGE) {
    edge = PREFERRED_ATLAS_EDGE
  }
  for (;;) {
    const packed = packMaxRects(layers, edge, edge)
    if (packed) {
      return {
        places: packed.places,
        width: Math.max(MIN_ATLAS_EDGE, packed.width),
        height: Math.max(MIN_ATLAS_EDGE, packed.height),
      }
    }
    if (edge >= MAX_ATLAS_EDGE) {
      throw new Error(
        formatTemplate(copy.anime25dAtlasOverflow, { max: MAX_ATLAS_EDGE }),
      )
    }
    edge = Math.min(
      MAX_ATLAS_EDGE,
      Math.max(edge + 1, Math.ceil(edge * 1.15)),
    )
  }
}

interface FreeRect {
  x: number
  y: number
  w: number
  h: number
}

function packMaxRects(
  layers: AtlasSpriteSize[],
  binWidth: number,
  binHeight: number,
): {
  places: Array<{ x: number; y: number }>
  width: number
  height: number
} | null {
  const order = layers
    .map((_, index) => index)
    .toSorted(
      (a, b) =>
        Math.max(1, layers[b].height) - Math.max(1, layers[a].height) ||
        Math.max(1, layers[b].width) - Math.max(1, layers[a].width),
    )
  const places: Array<{ x: number; y: number }> = Array.from(
    { length: layers.length },
    () => ({ x: 0, y: 0 }),
  )
  let free: FreeRect[] = [
    {
      x: ATLAS_PADDING,
      y: ATLAS_PADDING,
      w: binWidth - ATLAS_PADDING,
      h: binHeight - ATLAS_PADDING,
    },
  ]
  let packedWidth = ATLAS_PADDING
  let packedHeight = ATLAS_PADDING
  for (const index of order) {
    const drawWidth = Math.max(1, layers[index].width)
    const drawHeight = Math.max(1, layers[index].height)
    const slotW = drawWidth + ATLAS_PADDING
    const slotH = drawHeight + ATLAS_PADDING
    const node = findMaxRectsNode(free, slotW, slotH)
    if (!node) return null
    places[index] = { x: node.x, y: node.y }
    free = splitFreeRects(free, { x: node.x, y: node.y, w: slotW, h: slotH })
    packedWidth = Math.max(packedWidth, node.x + drawWidth + ATLAS_PADDING)
    packedHeight = Math.max(packedHeight, node.y + drawHeight + ATLAS_PADDING)
  }
  return { places, width: packedWidth, height: packedHeight }
}

function findMaxRectsNode(
  free: FreeRect[],
  slotW: number,
  slotH: number,
): { x: number; y: number } | null {
  let best: { x: number; y: number } | null = null
  let bestShort = Number.POSITIVE_INFINITY
  let bestLong = Number.POSITIVE_INFINITY
  for (const rect of free) {
    if (rect.w < slotW || rect.h < slotH) continue
    const leftoverW = rect.w - slotW
    const leftoverH = rect.h - slotH
    const shortSide = Math.min(leftoverW, leftoverH)
    const longSide = Math.max(leftoverW, leftoverH)
    if (
      shortSide < bestShort ||
      (shortSide === bestShort && longSide < bestLong)
    ) {
      bestShort = shortSide
      bestLong = longSide
      best = { x: rect.x, y: rect.y }
    }
  }
  return best
}

function splitFreeRects(free: FreeRect[], used: FreeRect): FreeRect[] {
  const next: FreeRect[] = []
  for (const rect of free) {
    if (!rectsOverlap(rect, used)) {
      next.push(rect)
      continue
    }
    if (used.x > rect.x) {
      next.push({ x: rect.x, y: rect.y, w: used.x - rect.x, h: rect.h })
    }
    if (used.x + used.w < rect.x + rect.w) {
      next.push({
        x: used.x + used.w,
        y: rect.y,
        w: rect.x + rect.w - (used.x + used.w),
        h: rect.h,
      })
    }
    if (used.y > rect.y) {
      next.push({ x: rect.x, y: rect.y, w: rect.w, h: used.y - rect.y })
    }
    if (used.y + used.h < rect.y + rect.h) {
      next.push({
        x: rect.x,
        y: used.y + used.h,
        w: rect.w,
        h: rect.y + rect.h - (used.y + used.h),
      })
    }
  }
  return pruneContainedRects(next.filter((rect) => rect.w > 0 && rect.h > 0))
}

function rectsOverlap(a: FreeRect, b: FreeRect): boolean {
  return (
    a.x < b.x + b.w &&
    a.x + a.w > b.x &&
    a.y < b.y + b.h &&
    a.y + a.h > b.y
  )
}

function pruneContainedRects(rects: FreeRect[]): FreeRect[] {
  return rects.filter(
    (rect, index) =>
      !rects.some(
        (other, otherIndex) =>
          otherIndex !== index && rectContains(other, rect),
      ),
  )
}

function rectContains(outer: FreeRect, inner: FreeRect): boolean {
  return (
    outer.x <= inner.x &&
    outer.y <= inner.y &&
    outer.x + outer.w >= inner.x + inner.w &&
    outer.y + outer.h >= inner.y + inner.h
  )
}

export async function packAnime25DAtlas(
  frame: RigCanvasFrame,
  layers: RasterLayer[],
  copy: Anime25DImportCopy,
): Promise<{
  atlas: Blob
  analysisReference: Blob
  layers: PreparedLayer[]
  width: number
  height: number
}> {
  const trimmed = layers.map(trimRaster)
  const { places, width: packedWidth, height: packedHeight } =
    layoutAnime25DAtlas(trimmed, copy)
  const createCanvas = () =>
    typeof document === 'undefined'
      ? new OffscreenCanvas(1, 1)
      : document.createElement('canvas')
  const requiredContext = (canvas: HTMLCanvasElement | OffscreenCanvas) => {
    const context = canvas.getContext('2d', { willReadFrequently: true }) as
      CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null
    if (!context) throw new Error(copy.canvasUnsupported)
    return context
  }
  const atlas = createCanvas()
  atlas.width = packedWidth
  atlas.height = packedHeight
  const context = requiredContext(atlas)
  const analysisCanvas = createCanvas()
  analysisCanvas.width = Math.max(1, Math.round(frame.width))
  analysisCanvas.height = Math.max(1, Math.round(frame.height))
  const analysisContext = requiredContext(analysisCanvas)
  const analysisScaleX = analysisCanvas.width / Math.max(1, frame.width)
  const analysisScaleY = analysisCanvas.height / Math.max(1, frame.height)
  const layerCanvas = createCanvas()
  const prepared: PreparedLayer[] = []
  for (const [index, layer] of trimmed.entries()) {
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
          ? (layer.documentStrands ?? []).map((strand) => ({
              x: (strand.x - frame.x) / frame.width,
              rootY: (strand.rootY - frame.y) / frame.width,
              tipY: (strand.tipY - frame.y) / frame.width,
            }))
          : [],
    })
  }
  const [blob, analysisReference] = await Promise.all([
    canvasPng(atlas, copy.rigAtlasFailed),
    canvasPng(analysisCanvas, copy.rigAtlasFailed),
  ])
  return {
    atlas: blob,
    analysisReference,
    layers: prepared,
    width: packedWidth,
    height: packedHeight,
  }
}

/** Layers visible when the character rests with eyes open and mouth closed. */
export function visibleInAnalysisReference(layer: RasterLayer): boolean {
  if (
    layer.role === 'maniac-eye-shadow' ||
    layer.role === 'maniac-mouth-shadow' ||
    layer.role === 'iris-silly' ||
    layer.role === 'lovestruck-heart' ||
    layer.role === 'lovestruck-face-effect' ||
    layer.role === 'lovestruck-drool' ||
    layer.role === 'anger-mark' ||
    layer.role === 'speechless-sweat' ||
    layer.role.startsWith('accent-')
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

function canvasPng(
  canvas: HTMLCanvasElement | OffscreenCanvas,
  failure: string,
): Promise<Blob> {
  if ('convertToBlob' in canvas)
    return canvas.convertToBlob({ type: 'image/png' })
  return new Promise<Blob>((resolve, reject) =>
    canvas.toBlob(
      (value) => (value ? resolve(value) : reject(new Error(failure))),
      'image/png',
    ),
  )
}
