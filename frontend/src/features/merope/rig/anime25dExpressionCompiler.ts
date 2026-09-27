import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DEyeAnchor } from '../anime25drig/types'
import type { RasterLayer } from './anime25dImportTypes'
import type { Anime25DFaceFrame } from './faceFrame'
import type { MouthExpressionKind } from './mouthExpression'
import { uniquePartId } from './anime25dRaster'
import { createCryEyeBitmap, cryEyeGeneratedSize } from './cryEye'
import {
  createDizzyEyeBitmap,
  dizzyEyeGeneratedSize,
  sampleDizzyEyeTint,
} from './dizzyEye'
import {
  createAngerMarkBitmap,
  createSpeechlessSweatBitmap,
  expressionSymbolGeneratedSizes,
} from './expressionSymbols'
import {
  faceContourAlongEyeLine,
  faceFramePoint,
  placeFaceBitmap,
  resolveAnime25DFaceFrame,
} from './faceFrame'
import { createLipMouthBitmap, detectPaintedLips, LIP_MOUTH_KINDS, lipMouthSize } from './lipMouth'
import {
  createLovestruckDroolBitmap,
  createLovestruckFaceEffectBitmap,
  createLovestruckHeartBitmap,
  LOVESTRUCK_CHEEK_ROW,
  lovestruckDroolGeneratedSize,
  lovestruckFaceEffectGeneratedSize,
  lovestruckHeartGeneratedSize,
} from './lovestruckExpression'
import {
  createManiacEyeShadowBitmap,
  maniacEyeShadowGeneratedSize,
} from './maniacEyeShadow'
import {
  createManiacMouthShadowBitmap,
  createMouthExpressionBitmap,
  mouthExpressionGeneratedSizes,
  sampleMouthExpressionPalette,
} from './mouthExpression'
import {
  createSillyEyeWhiteBitmap,
  createSillyIrisBitmap,
  createSillyIrisFromArtwork,
  sampleSillyEyePalette,
  SILLY_IRIS_REST_SHARE,
  sillyEyeGeneratedSize,
  sillyIrisTravelRoom,
} from './sillyEye'
import { createSqueezeEyeBitmap, squeezeEyeGeneratedSize } from './squeezeEye'

export function compileAnime25DExpressionLayers(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
): RasterLayer[] {
  const frame = resolveAnime25DFaceFrame(anchors)
  let output = synthesizeMissingDizzyEyes(layers, anchors, frame)
  output = synthesizeMissingSqueezeEyes(output, anchors, frame)
  output = synthesizeMissingCryEyes(output, anchors, frame)
  output = synthesizeMissingSillyEyes(output, anchors, frame)
  output = synthesizeMissingLovestruckEffects(output, anchors, frame)
  output = synthesizeMissingManiacEyeShadows(output, anchors, frame)
  output = synthesizeMissingMouthExpressions(output, anchors, frame)
  return synthesizeMissingExpressionSymbols(output, anchors, frame)
}

function synthesizeMissingDizzyEyes(
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

function synthesizeMissingSqueezeEyes(
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

function synthesizeMissingCryEyes(
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

function synthesizeMissingSillyEyes(
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

function synthesizeMissingLovestruckEffects(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
): RasterLayer[] {
  const usedIds = new Set(layers.map((layer) => layer.id))
  const generated: RasterLayer[] = []
  const mouthReference =
    layers.find((layer) => layer.role === 'mouth-close') ??
    layers.find((layer) => layer.role === 'mouth-open')
  const pink = sampleMouthExpressionPalette(mouthReference?.data).fill

  for (const side of ['left', 'right'] as const) {
    if (
      layers.some(
        (layer) => layer.role === 'lovestruck-heart' && layer.side === side,
      )
    ) {
      continue
    }
    const eye = side === 'left' ? anchors.eyeL : anchors.eyeR
    if (!eye) continue
    const bitmap = createLovestruckHeartBitmap(
      lovestruckHeartGeneratedSize(eye),
      pink,
    )
    generated.push({
      id: uniquePartId(`lovestruck-heart-${side}`, usedIds),
      role: 'lovestruck-heart',
      sourceName: `lovestruck-heart-${side}`,
      order: 0,
      side,
      group: 'head',
      ...placeFaceBitmap(
        bitmap,
        bitmap.width / 2,
        bitmap.height * 0.48,
        { x: eye.icx, y: eye.icy },
        frame.roll,
      ),
      synthetic: true,
    })
  }

  if (!layers.some((layer) => layer.role === 'lovestruck-face-effect')) {
    const bitmap = createLovestruckFaceEffectBitmap(
      lovestruckFaceEffectGeneratedSize(anchors.face),
      pink,
    )
    const placed =
      frame.landmarks && anchors.eyeL && anchors.eyeR
        ? placeFaceBitmap(
            bitmap,
            bitmap.width / 2,
            bitmap.height * LOVESTRUCK_CHEEK_ROW,
            faceFramePoint(
              frame,
              frame.originX,
              frame.originY,
              0,
              cheekDrop(anchors.eyeL, anchors.eyeR),
            ),
            frame.roll,
          )
        : placeFaceBitmap(
            bitmap,
            bitmap.width / 2,
            0,
            {
              x: anchors.face.cx,
              y:
                anchors.face.y0 +
                Math.max(1, anchors.face.y1 - anchors.face.y0) * 0.12,
            },
            0,
          )
    const faceLayer = layers.find((layer) => layer.role === 'face')
    if (faceLayer) {
      clipBitmapAlphaToLayer(
        placed.data,
        placed.width,
        placed.height,
        placed.left,
        placed.top,
        faceLayer,
      )
    }
    generated.push({
      id: uniquePartId('lovestruck-face-effect', usedIds),
      role: 'lovestruck-face-effect',
      sourceName: 'lovestruck-face-effect',
      order: 0,
      side: null,
      group: 'head',
      ...placed,
      synthetic: true,
    })
  }

  if (!layers.some((layer) => layer.role === 'lovestruck-drool')) {
    const bitmap = createLovestruckDroolBitmap(
      lovestruckDroolGeneratedSize(anchors.mouth, anchors.face),
    )
    const mouthWidth = Math.max(1, anchors.mouth.x1 - anchors.mouth.x0)
    const mouthHeight = Math.max(1, anchors.mouth.y1 - anchors.mouth.y0)
    generated.push({
      id: uniquePartId('lovestruck-drool', usedIds),
      role: 'lovestruck-drool',
      sourceName: 'lovestruck-drool',
      order: 0,
      side: null,
      group: 'head',
      ...placeFaceBitmap(
        bitmap,
        bitmap.width * 0.36,
        0,
        faceFramePoint(
          frame,
          anchors.mouth.cx,
          anchors.mouth.cy,
          mouthWidth * 0.5,
          mouthHeight * 0.08,
        ),
        frame.roll,
      ),
      synthetic: true,
    })
  }

  return generated.length > 0 ? [...layers, ...generated] : layers
}

/** Cheek blush sits just under the drawn lower lids, not at a face-box ratio. */
function cheekDrop(
  eyeL: Readonly<Anime25DEyeAnchor>,
  eyeR: Readonly<Anime25DEyeAnchor>,
): number {
  const lowerLid = (eyeL.y1 - eyeL.icy + (eyeR.y1 - eyeR.icy)) / 2
  const eyeHeight = (eyeL.y1 - eyeL.y0 + (eyeR.y1 - eyeR.y0)) / 2
  return lowerLid + eyeHeight * 0.2
}

function clipBitmapAlphaToLayer(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  left: number,
  top: number,
  mask: RasterLayer,
): void {
  for (let y = 0; y < height; y += 1) {
    const maskY = Math.floor(top + y - mask.top)
    for (let x = 0; x < width; x += 1) {
      const targetAlpha = (y * width + x) * 4 + 3
      if (data[targetAlpha] === 0) continue
      const maskX = Math.floor(left + x - mask.left)
      if (
        maskX < 0 ||
        maskX >= mask.width ||
        maskY < 0 ||
        maskY >= mask.height
      ) {
        data[targetAlpha] = 0
        continue
      }
      const maskAlpha = mask.data[(maskY * mask.width + maskX) * 4 + 3]
      data[targetAlpha] = Math.round((data[targetAlpha] * maskAlpha) / 255)
    }
  }
}

function synthesizeMissingManiacEyeShadows(
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

function synthesizeMissingMouthExpressions(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
): RasterLayer[] {
  const reference =
    layers.find((layer) => layer.role === 'mouth-close') ??
    layers.find((layer) => layer.role === 'mouth-open')
  if (!reference) return layers
  const expressions: ReadonlyArray<{
    role:
      | 'mouth-open'
      | 'mouth-wide'
      | 'mouth-round'
      | 'mouth-narrow'
      | 'mouth-cry'
      | 'mouth-maniac'
      | 'mouth-silly'
    kind: MouthExpressionKind
  }> = [
    { role: 'mouth-open', kind: 'open' },
    { role: 'mouth-wide', kind: 'wide' },
    { role: 'mouth-round', kind: 'round' },
    { role: 'mouth-narrow', kind: 'narrow' },
    { role: 'mouth-cry', kind: 'cry' },
    { role: 'mouth-maniac', kind: 'maniac' },
    { role: 'mouth-silly', kind: 'silly' },
  ]
  const missing = expressions.filter(
    ({ role }) => !layers.some((layer) => layer.role === role),
  )
  const needsManiacShadow = !layers.some(
    (layer) => layer.role === 'maniac-mouth-shadow',
  )
  if (missing.length === 0 && !needsManiacShadow) return layers

  const sizes = mouthExpressionGeneratedSizes(reference, {
    width: Math.max(1, anchors.face.x1 - anchors.face.x0),
    height: Math.max(1, anchors.face.y1 - anchors.face.y0),
    mouthToChin: Math.max(1, anchors.face.y1 - anchors.mouth.cy),
  })
  const palette = sampleMouthExpressionPalette(reference.data)
  // A painted mouth with lips gets speaking mouths painted to match, not cel glyphs.
  const paintedLips = detectPaintedLips(reference)
  const usedIds = new Set(layers.map((layer) => layer.id))
  const generated: RasterLayer[] = []
  const mouthCenter = { x: anchors.mouth.cx, y: anchors.mouth.cy }
  const placeOnMouth = (
    bitmap: { width: number; height: number; data: Uint8ClampedArray },
    kind: MouthExpressionKind,
  ) =>
    placeFaceBitmap(
      bitmap,
      bitmap.width / 2,
      mouthPivotY(kind, bitmap.height),
      mouthCenter,
      frame.roll,
    )
  let generatedManiacSize: { width: number; height: number } | null = null
  for (const { role, kind } of missing) {
    const bitmap = paintedLips && LIP_MOUTH_KINDS.has(kind)
      ? createLipMouthBitmap(kind, lipMouthSize(kind, sizes[kind]), paintedLips)
      : createMouthExpressionBitmap(kind, sizes[kind], palette)
    if (kind === 'maniac') {
      generatedManiacSize = { width: bitmap.width, height: bitmap.height }
    }
    generated.push({
      id: uniquePartId(role, usedIds),
      role,
      sourceName: role,
      order: 0,
      side: null,
      group: 'head',
      ...placeOnMouth(bitmap, kind),
      synthetic: true,
    })
  }
  if (needsManiacShadow) {
    const authoredManiac = layers.find((layer) => layer.role === 'mouth-maniac')
    const bitmap = createManiacMouthShadowBitmap(
      authoredManiac
        ? { width: authoredManiac.width, height: authoredManiac.height }
        : (generatedManiacSize ?? sizes.maniac),
      palette,
    )
    generated.unshift({
      id: uniquePartId('maniac-mouth-shadow', usedIds),
      role: 'maniac-mouth-shadow',
      sourceName: 'maniac-mouth-shadow',
      order: 0,
      side: null,
      group: 'head',
      // Authored artwork already carries the portrait's tilt.
      ...(authoredManiac
        ? {
            left: authoredManiac.left,
            top: authoredManiac.top,
            width: bitmap.width,
            height: bitmap.height,
            data: bitmap.data,
          }
        : placeOnMouth(bitmap, 'maniac')),
      synthetic: true,
    })
  }

  const output = Iterator.from(layers).toArray()
  let insertAt = -1
  for (let index = 0; index < output.length; index += 1) {
    if (
      output[index].role === 'mouth-open' ||
      output[index].role === 'mouth-close' ||
      output[index].role === 'mouth-wide' ||
      output[index].role === 'mouth-round' ||
      output[index].role === 'mouth-narrow' ||
      output[index].role === 'mouth-cry' ||
      output[index].role === 'mouth-maniac' ||
      output[index].role === 'mouth-silly'
    ) {
      insertAt = index
    }
  }
  return output.toSpliced(insertAt + 1, 0, ...generated)
}

/** Where each generated glyph's own mouth line sits, measured from its top. */
function mouthPivotY(kind: MouthExpressionKind, height: number): number {
  if (kind === 'cry') return height * 0.46
  if (kind === 'maniac') return height * 0.61 - 3
  return height * 0.5
}

function synthesizeMissingExpressionSymbols(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
): RasterLayer[] {
  const needsAnger = !layers.some((layer) => layer.role === 'anger-mark')
  const needsSweat = !layers.some((layer) => layer.role === 'speechless-sweat')
  if (!needsAnger && !needsSweat) return layers

  const faceWidth = Math.max(1, anchors.face.x1 - anchors.face.x0)
  const faceHeight = Math.max(1, anchors.face.y1 - anchors.face.y0)
  const sizes = expressionSymbolGeneratedSizes(faceWidth)
  const usedIds = new Set(layers.map((layer) => layer.id))
  const eyelash = layers.find((layer) => layer.role === 'eyelash')
  const tint = sampleDizzyEyeTint(eyelash?.data)
  const generated: RasterLayer[] = []

  // Both accents hang off the drawn face contour at eye height, so a turned
  // or tilted head keeps them on its temple and cheek instead of box corners.
  const faceLayer = layers.find((layer) => layer.role === 'face')
  const contour = (
    eye: Readonly<Anime25DEyeAnchor> | undefined,
    direction: -1 | 1,
  ) =>
    frame.landmarks && eye && faceLayer
      ? faceContourAlongEyeLine(
          frame,
          faceLayer,
          { x: eye.icx, y: eye.icy },
          direction,
        )
      : null

  if (needsAnger) {
    const bitmap = createAngerMarkBitmap(sizes.anger, tint)
    const temple = contour(anchors.eyeL, -1)
    const center = temple
      ? faceFramePoint(
          frame,
          temple.x,
          temple.y,
          faceWidth * 0.13,
          -faceHeight * 0.36,
        )
      : {
          x: anchors.face.x0 + faceWidth * 0.13,
          y: anchors.face.y0 + faceHeight * 0.22,
        }
    generated.push({
      id: uniquePartId('anger-mark', usedIds),
      role: 'anger-mark',
      sourceName: 'anger-mark',
      order: 0,
      side: null,
      group: 'head',
      ...placeFaceBitmap(
        bitmap,
        bitmap.width / 2,
        bitmap.height / 2,
        center,
        temple ? frame.roll : 0,
      ),
      synthetic: true,
    })
  }

  if (needsSweat) {
    const bitmap = createSpeechlessSweatBitmap(sizes.speechless)
    const cheek = contour(anchors.eyeR, 1)
    const eyeY = anchors.eyeR?.icy ?? anchors.eyeL?.icy
    const center = cheek
      ? faceFramePoint(frame, cheek.x, cheek.y, -faceWidth * 0.015, 0)
      : {
          x: anchors.face.x1 - faceWidth * 0.015,
          y: eyeY ?? anchors.face.y0 + faceHeight * 0.48,
        }
    generated.push({
      id: uniquePartId('speechless-sweat', usedIds),
      role: 'speechless-sweat',
      sourceName: 'speechless-sweat',
      order: 0,
      side: null,
      group: 'head',
      ...placeFaceBitmap(
        bitmap,
        bitmap.width / 2,
        bitmap.height * 0.2,
        center,
        cheek ? frame.roll : 0,
      ),
      synthetic: true,
    })
  }

  return [...layers, ...generated]
}
