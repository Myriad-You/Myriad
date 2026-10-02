import type { Psd } from 'ag-psd'
import type { Anime25DLayerRole } from './anime25d'
import type { Anime25DImportCopy } from './anime25dImportCopy'
import type { Anime25DImportProfile } from './anime25dImportProfile'
import type { AnimeAnchors, EyeSide, PreparedLayer, RasterLayer, RigCanvasFrame } from './anime25dImportTypes'
import type { MeropeRigImportSource, RigPoint } from './types'
import { anime25DImportProfile } from './anime25dImportProfile'
import { anime25DLayerAffectsFraming } from './anime25dLayerSemantics'
import { formatTemplate } from './formatTemplate'

/** Removes model letterboxing, then pads (never stretches) into the profile's stage. */
export function contentFrame(
  psd: Psd,
  layers: readonly RasterLayer[],
  profile: Anime25DImportProfile = anime25DImportProfile('bust'),
): RigCanvasFrame {
  const documentArea = psd.width * psd.height
  const framingLayers = layers.filter((layer) =>
    anime25DLayerAffectsFraming(layer, documentArea, profile.lowerBodyFrames),
  )
  const candidates = framingLayers.length > 0 ? framingLayers : layers
  let left = psd.width
  let top = psd.height
  let right = 0
  let bottom = 0
  for (const layer of candidates) {
    left = Math.min(left, layer.left)
    top = Math.min(top, layer.top)
    right = Math.max(right, layer.left + layer.width)
    bottom = Math.max(bottom, layer.top + layer.height)
  }
  if (right <= left || bottom <= top) {
    return { x: 0, y: 0, width: psd.width, height: psd.height }
  }
  const horizontalPadding = Math.max(4, Math.round((right - left) * 0.04))
  const verticalPadding = Math.max(4, Math.round((bottom - top) * 0.025))
  left = Math.max(0, Math.floor(left - horizontalPadding))
  top = Math.max(0, Math.floor(top - verticalPadding))
  right = Math.min(psd.width, Math.ceil(right + horizontalPadding))
  bottom = Math.min(psd.height, Math.ceil(bottom + verticalPadding))
  let width = Math.max(1, right - left)
  let height = Math.max(1, bottom - top)
  const targetAspect = profile.canvas.width / profile.canvas.height
  if (width / height > targetAspect) {
    const targetHeight = width / targetAspect
    top -= (targetHeight - height) / 2
    height = targetHeight
  } else {
    const targetWidth = height * targetAspect
    left -= (targetWidth - width) / 2
    width = targetWidth
  }
  return { x: left, y: top, width, height }
}

/**
 * A standing figure's hips: the top of its legs, between them. The upper body
 * leans from here while the legs and feet stay planted.
 */
export function hipPivot(layers: readonly RasterLayer[]): RigPoint | null {
  const legs = layers.filter((layer) => layer.role === 'legwear')
  if (legs.length === 0) return null
  const left = Math.min(...legs.map((layer) => layer.left))
  const right = Math.max(...legs.map((layer) => layer.left + layer.width))
  return {
    x: (left + right) / 2,
    y: Math.min(...legs.map((layer) => layer.top)),
  }
}

export function deriveAnchors(
  frame: RigCanvasFrame,
  layers: PreparedLayer[],
  rawFaceCenter: RigPoint,
  copy: Anime25DImportCopy,
): AnimeAnchors {
  const face = requiredLayer(layers, 'face', copy).bounds
  const center = (layer: PreparedLayer | undefined): RigPoint | undefined =>
    layer
      ? {
          x: layer.bounds.x + layer.bounds.width / 2,
          y: layer.bounds.y + layer.bounds.height / 2,
        }
      : undefined
  const eyes: Partial<Record<EyeSide, RigPoint>> = {}
  const irises: Partial<Record<EyeSide, RigPoint>> = {}
  for (const side of ['left', 'right'] as const) {
    const eye = layers.find(
      (layer) => layer.role === 'eyewhite' && layer.side === side,
    )
    const iris = layers.find(
      (layer) => layer.role === 'irides' && layer.side === side,
    )
    const fallback = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const eyeCenter = center(eye ?? fallback)
    if (eyeCenter) eyes[side] = eyeCenter
    const irisCenter = center(iris)
    if (irisCenter) irises[side] = irisCenter
  }
  const neckLayer = layers.find((layer) => layer.role === 'neck')
  const topwear = layers.find((layer) => layer.role === 'topwear')
  const bottomwear = layers.find((layer) => layer.role === 'bottomwear')
  const bodyReference = topwear ?? bottomwear ?? neckLayer
  const neck = neckLayer
    ? {
        x: neckLayer.bounds.x + neckLayer.bounds.width / 2,
        y: neckLayer.bounds.y + neckLayer.bounds.height * 0.85,
      }
    : {
        x: face.x + face.width / 2,
        y: face.y + face.height + 20 / frame.width,
      }
  const bodyBottom = bodyReference
    ? {
        x: bodyReference.bounds.x + bodyReference.bounds.width / 2,
        y: bodyReference.bounds.y + bodyReference.bounds.height,
      }
    : { x: 0.5, y: frame.height / frame.width }
  const mouth = center(
    layers.find((layer) => layer.role === 'mouth-open') ??
      layers.find((layer) => layer.role === 'mouth-close'),
  )
  return {
    face,
    faceCenter: {
      x: (rawFaceCenter.x - frame.x) / frame.width,
      y: (rawFaceCenter.y - frame.y) / frame.width,
    },
    neck,
    bodyBottom,
    eyes,
    irises,
    mouth: mouth ?? null,
  }
}

export function semanticAnchors(
  anchors: AnimeAnchors,
): MeropeRigImportSource['semanticAnchors'] {
  const relative = (boneId: string, pivot: RigPoint, point: RigPoint) => ({
    boneId,
    offset: { x: point.x - pivot.x, y: point.y - pivot.y },
  })
  return {
    forehead: relative('head', anchors.neck, {
      x: anchors.face.x + anchors.face.width * 0.5,
      y: anchors.face.y + anchors.face.height * 0.18,
    }),
    'temple-right': relative('head', anchors.neck, {
      x: anchors.face.x + anchors.face.width * 0.28,
      y: anchors.face.y + anchors.face.height * 0.3,
    }),
    chin: relative('head', anchors.neck, {
      x: anchors.face.x + anchors.face.width * 0.5,
      y: anchors.face.y + anchors.face.height * 0.88,
    }),
    chest: relative('body', anchors.neck, {
      x: anchors.neck.x,
      y: anchors.neck.y + anchors.face.height * 0.42,
    }),
  }
}

function requiredLayer(
  layers: PreparedLayer[],
  role: Anime25DLayerRole,
  copy: Anime25DImportCopy,
): PreparedLayer {
  const layer = layers.find((candidate) => candidate.role === role)
  if (!layer) {
    throw new Error(formatTemplate(copy.anime25dMissingLayer, { role }))
  }
  return layer
}
