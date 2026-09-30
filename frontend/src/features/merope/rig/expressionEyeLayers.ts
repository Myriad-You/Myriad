import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DFaceFrame } from '../expressionShapes/faceFrame'
import type { RasterLayer } from './anime25dImportTypes'
import { createCryEyeBitmap, cryEyeGeneratedSize } from '../expressionShapes/cryEye'
import { createDizzyEyeBitmap, dizzyEyeGeneratedSize, sampleDizzyEyeTint } from '../expressionShapes/dizzyEye'
import { faceFramePoint, placeFaceBitmap } from '../expressionShapes/faceFrame'
import { createManiacEyeShadowBitmap, maniacEyeShadowGeneratedSize } from '../expressionShapes/maniacEyeShadow'
import { createSillyEyeWhiteBitmap, createSillyIrisBitmap, createSillyIrisFromArtwork, sampleSillyEyePalette, SILLY_IRIS_REST_SHARE, sillyEyeGeneratedSize, sillyIrisTravelRoom } from '../expressionShapes/sillyEye'
import { createSqueezeEyeBitmap, squeezeEyeGeneratedSize } from '../expressionShapes/squeezeEye'
import { uniquePartId } from './anime25dRaster'

export function synthesizeMissingDizzyEyes(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
): RasterLayer[] {
  const generated: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  for (const side of ['left', 'right'] as const) {
    if (
      layers.some((layer) => layer.role === 'eye-dizzy' && layer.side === side)
    ) {
      continue
    }
    const eye = side === 'left' ? anchors.eyeL : anchors.eyeR
    if (!eye) continue
    const eyelash = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const size = dizzyEyeGeneratedSize(eye)
    const bitmap = createDizzyEyeBitmap(
      size,
      sampleDizzyEyeTint(eyelash?.data),
      side,
    )
    generated.push({
      id: uniquePartId(`eye-dizzy-${side}`, usedIds),
      role: 'eye-dizzy',
      sourceName: `eye-dizzy-${side}`,
      order: 0,
      side,
      group: 'head',
      ...placeFaceBitmap(
        bitmap,
        bitmap.width / 2,
        bitmap.height / 2,
        { x: eye.icx, y: eye.icy },
        frame.roll,
      ),
    })
  }
  if (generated.length === 0) return layers
  const output = Iterator.from(layers).toArray()
  let insertAt = -1
  for (let index = 0; index < output.length; index += 1) {
    if (
      output[index].role === 'eye-close' ||
      output[index].role === 'eyelash'
    ) {
      insertAt = index
    }
  }
  return output.toSpliced(insertAt + 1, 0, ...generated)
}

export function synthesizeMissingSqueezeEyes(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
): RasterLayer[] {
  const generated: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  for (const side of ['left', 'right'] as const) {
    if (
      layers.some(
        (layer) => layer.role === 'eye-squeeze' && layer.side === side,
      )
    ) {
      continue
    }
    const eye = side === 'left' ? anchors.eyeL : anchors.eyeR
    if (!eye) continue
    const eyelash = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const bitmap = createSqueezeEyeBitmap(
      squeezeEyeGeneratedSize(eye),
      sampleDizzyEyeTint(eyelash?.data),
      side,
    )
    const center = faceFramePoint(
      frame,
      eye.icx,
      eye.icy,
      0,
      (eye.closeY - eye.icy) * 0.45,
    )
    generated.push({
      id: uniquePartId(`eye-squeeze-${side}`, usedIds),
      role: 'eye-squeeze',
      sourceName: `eye-squeeze-${side}`,
      order: 0,
      side,
      group: 'head',
      ...placeFaceBitmap(
        bitmap,
        bitmap.width / 2,
        bitmap.height / 2,
        center,
        frame.roll,
      ),
    })
  }
  if (generated.length === 0) return layers
  const output = Iterator.from(layers).toArray()
  let insertAt = -1
  for (let index = 0; index < output.length; index += 1) {
    if (
      output[index].role === 'eye-close' ||
      output[index].role === 'eye-dizzy' ||
      output[index].role === 'eyelash'
    ) {
      insertAt = index
    }
  }
  return output.toSpliced(insertAt + 1, 0, ...generated)
}

export function synthesizeMissingCryEyes(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
): RasterLayer[] {
  const generated: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  for (const side of ['left', 'right'] as const) {
    if (
      layers.some((layer) => layer.role === 'eye-cry' && layer.side === side)
    ) {
      continue
    }
    const eye = side === 'left' ? anchors.eyeL : anchors.eyeR
    if (!eye) continue
    const eyelash = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const bitmap = createCryEyeBitmap(
      cryEyeGeneratedSize(eye),
      sampleDizzyEyeTint(eyelash?.data),
      side,
    )
    const eyeMarkCenter = faceFramePoint(
      frame,
      eye.icx,
      eye.icy,
      0,
      (eye.closeY - eye.icy) * 0.45,
    )
    generated.push({
      id: uniquePartId(`eye-cry-${side}`, usedIds),
      role: 'eye-cry',
      sourceName: `eye-cry-${side}`,
      order: 0,
      side,
      group: 'head',
      ...placeFaceBitmap(
        bitmap,
        bitmap.width / 2,
        bitmap.width * 0.305,
        eyeMarkCenter,
        frame.roll,
      ),
    })
  }
  if (generated.length === 0) return layers
  const output = Iterator.from(layers).toArray()
  let insertAt = -1
  for (let index = 0; index < output.length; index += 1) {
    if (
      output[index].role === 'eye-close' ||
      output[index].role === 'eye-dizzy' ||
      output[index].role === 'eye-squeeze' ||
      output[index].role === 'eyelash'
    ) {
      insertAt = index
    }
  }
  return output.toSpliced(insertAt + 1, 0, ...generated)
}

export function synthesizeMissingSillyEyes(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
): RasterLayer[] {
  const generated: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  for (const side of ['left', 'right'] as const) {
    const hasWhite = layers.some(
      (layer) => layer.role === 'eye-silly-white' && layer.side === side,
    )
    const hasIris = layers.some(
      (layer) => layer.role === 'iris-silly' && layer.side === side,
    )
    if (hasWhite && hasIris) continue
    const eye = side === 'left' ? anchors.eyeL : anchors.eyeR
    if (!eye) continue
    const eyelash = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const irides = layers.find(
      (layer) => layer.role === 'irides' && layer.side === side,
    )
    const eyewhite = layers.find(
      (layer) => layer.role === 'eyewhite' && layer.side === side,
    )
    const size = sillyEyeGeneratedSize(eye)
    const palette = sampleSillyEyePalette(
      sampleDizzyEyeTint(eyelash?.data),
      irides?.data,
      eyewhite?.data,
    )
    const centerX = eye.icx
    const centerY = eye.icy
    if (!hasWhite) {
      const bitmap = createSillyEyeWhiteBitmap(size, palette, side)
      generated.push({
        id: uniquePartId(`eye-silly-white-${side}`, usedIds),
        role: 'eye-silly-white',
        sourceName: `eye-silly-white-${side}`,
        order: 0,
        side,
        group: 'head',
        ...placeFaceBitmap(
          bitmap,
          bitmap.width / 2,
          bitmap.height / 2,
          { x: centerX, y: centerY },
          frame.roll,
        ),
        synthetic: true,
      })
    }
    if (!hasIris) {
      const bitmap =
        (irides && createSillyIrisFromArtwork(irides, size.iris)) ??
        createSillyIrisBitmap(size.iris, palette, side)
      const room = sillyIrisTravelRoom(size)
      const divergentX =
        (side === 'left' ? -1 : 1) * room.x * SILLY_IRIS_REST_SHARE
      const divergentY =
        (side === 'left' ? 1 : -1) * room.y * SILLY_IRIS_REST_SHARE
      // The iris keeps its drawn orientation; only its resting offset tilts.
      const irisCenter = faceFramePoint(
        frame,
        centerX,
        centerY,
        divergentX,
        divergentY,
      )
      generated.push({
        id: uniquePartId(`iris-silly-${side}`, usedIds),
        role: 'iris-silly',
        sourceName: `iris-silly-${side}`,
        order: 0,
        side,
        group: 'head',
        ...placeFaceBitmap(
          bitmap,
          bitmap.width / 2,
          bitmap.height / 2,
          irisCenter,
          0,
        ),
        synthetic: true,
      })
    }
  }
  if (generated.length === 0) return layers
  const output = Iterator.from(layers).toArray()
  let insertAt = -1
  for (let index = 0; index < output.length; index += 1) {
    if (
      output[index].role === 'eye-close' ||
      output[index].role === 'eye-dizzy' ||
      output[index].role === 'eye-squeeze' ||
      output[index].role === 'eye-cry' ||
      output[index].role === 'eyelash'
    ) {
      insertAt = index
    }
  }
  return output.toSpliced(insertAt + 1, 0, ...generated)
}

export function synthesizeMissingManiacEyeShadows(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
): RasterLayer[] {
  const generated: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  for (const side of ['left', 'right'] as const) {
    if (
      layers.some(
        (layer) => layer.role === 'maniac-eye-shadow' && layer.side === side,
      )
    ) {
      continue
    }
    const eye = side === 'left' ? anchors.eyeL : anchors.eyeR
    if (!eye) continue
    const eyelash = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const bitmap = createManiacEyeShadowBitmap(
      maniacEyeShadowGeneratedSize(eye),
      sampleDizzyEyeTint(eyelash?.data),
      side,
    )
    const eyeHeight = Math.max(1, eye.y1 - eye.y0)
    const eyeWidth = Math.max(1, eye.x1 - eye.x0)
    const inwardOffset = (side === 'left' ? 1 : -1) * eyeWidth * 0.12
    generated.push({
      id: uniquePartId(`maniac-eye-shadow-${side}`, usedIds),
      role: 'maniac-eye-shadow',
      sourceName: `maniac-eye-shadow-${side}`,
      order: 0,
      side,
      group: 'head',
      ...placeFaceBitmap(
        bitmap,
        bitmap.width / 2,
        0,
        faceFramePoint(
          frame,
          eye.icx,
          eye.icy,
          inwardOffset,
          eye.y1 - eyeHeight * 0.22 - eye.icy,
        ),
        frame.roll,
      ),
      synthetic: true,
    })
  }
  if (generated.length === 0) return layers

  const output = Iterator.from(layers).toArray()
  const firstEyeLayer = output.findIndex(
    (layer) =>
      layer.role === 'eyewhite' ||
      layer.role === 'irides' ||
      layer.role === 'eyelash' ||
      layer.role === 'eye-close',
  )
  return firstEyeLayer >= 0
    ? output.toSpliced(firstEyeLayer, 0, ...generated)
    : output.concat(generated)
}
