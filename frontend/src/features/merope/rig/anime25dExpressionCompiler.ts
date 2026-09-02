import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { RasterLayer } from './anime25dImportTypes'
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
  createLovestruckDroolBitmap,
  createLovestruckFaceEffectBitmap,
  createLovestruckHeartBitmap,
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
  let output = synthesizeMissingDizzyEyes(layers, anchors)
  output = synthesizeMissingSqueezeEyes(output, anchors)
  output = synthesizeMissingCryEyes(output, anchors)
  output = synthesizeMissingSillyEyes(output, anchors)
  output = synthesizeMissingLovestruckEffects(output, anchors)
  output = synthesizeMissingManiacEyeShadows(output, anchors)
  output = synthesizeMissingMouthExpressions(output, anchors)
  return synthesizeMissingExpressionSymbols(output, anchors)
}

function synthesizeMissingDizzyEyes(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
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
      left: Math.round(eye.icx - bitmap.width / 2),
      top: Math.round(eye.icy - bitmap.height / 2),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
    })
  }
  if (generated.length === 0) return layers
  const output = [...layers]
  let insertAt = -1
  for (let index = 0; index < output.length; index += 1) {
    if (
      output[index].role === 'eye-close' ||
      output[index].role === 'eyelash'
    ) {
      insertAt = index
    }
  }
  output.splice(insertAt + 1, 0, ...generated)
  return output
}

function synthesizeMissingSqueezeEyes(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
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
    const centerY = eye.icy + (eye.closeY - eye.icy) * 0.45
    generated.push({
      id: uniquePartId(`eye-squeeze-${side}`, usedIds),
      role: 'eye-squeeze',
      sourceName: `eye-squeeze-${side}`,
      order: 0,
      side,
      group: 'head',
      left: Math.round(eye.icx - bitmap.width / 2),
      top: Math.round(centerY - bitmap.height / 2),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
    })
  }
  if (generated.length === 0) return layers
  const output = [...layers]
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
  output.splice(insertAt + 1, 0, ...generated)
  return output
}

function synthesizeMissingCryEyes(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
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
    const eyeMarkCenterY = eye.icy + (eye.closeY - eye.icy) * 0.45
    generated.push({
      id: uniquePartId(`eye-cry-${side}`, usedIds),
      role: 'eye-cry',
      sourceName: `eye-cry-${side}`,
      order: 0,
      side,
      group: 'head',
      left: Math.round(eye.icx - bitmap.width / 2),
      // Anchor the squeeze mark by eye width so a longer tear canvas extends
      // downward without moving the eye artwork or the tear root.
      top: Math.round(eyeMarkCenterY - bitmap.width * 0.305),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
    })
  }
  if (generated.length === 0) return layers
  const output = [...layers]
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
  output.splice(insertAt + 1, 0, ...generated)
  return output
}

function synthesizeMissingSillyEyes(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
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
        left: Math.round(centerX - bitmap.width / 2),
        top: Math.round(centerY - bitmap.height / 2),
        width: bitmap.width,
        height: bitmap.height,
        data: bitmap.data,
        synthetic: true,
      })
    }
    if (!hasIris) {
      // The character's own iris art always wins; the generated disc only
      // covers portraits that ship no separate iris layer at all.
      const bitmap =
        (irides && createSillyIrisFromArtwork(irides, size.iris)) ??
        createSillyIrisBitmap(size.iris, palette, side)
      // Seeded apart by a fixed share of the room, so the rest of it stays
      // available to the runtime drift and neither eye reaches the rim.
      const room = sillyIrisTravelRoom(size)
      const divergentX =
        (side === 'left' ? -1 : 1) * room.x * SILLY_IRIS_REST_SHARE
      const divergentY =
        (side === 'left' ? 1 : -1) * room.y * SILLY_IRIS_REST_SHARE
      generated.push({
        id: uniquePartId(`iris-silly-${side}`, usedIds),
        role: 'iris-silly',
        sourceName: `iris-silly-${side}`,
        order: 0,
        side,
        group: 'head',
        left: Math.round(centerX + divergentX - bitmap.width / 2),
        top: Math.round(centerY + divergentY - bitmap.height / 2),
        width: bitmap.width,
        height: bitmap.height,
        data: bitmap.data,
        synthetic: true,
      })
    }
  }
  if (generated.length === 0) return layers
  const output = [...layers]
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
  output.splice(insertAt + 1, 0, ...generated)
  return output
}

function synthesizeMissingLovestruckEffects(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
): RasterLayer[] {
  const usedIds = new Set(layers.map((layer) => layer.id))
  const generated: RasterLayer[] = []
  const mouthReference =
    layers.find((layer) => layer.role === 'mouth-close') ||
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
      left: Math.round(eye.icx - bitmap.width / 2),
      top: Math.round(eye.icy - bitmap.height * 0.48),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
      synthetic: true,
    })
  }

  if (!layers.some((layer) => layer.role === 'lovestruck-face-effect')) {
    const bitmap = createLovestruckFaceEffectBitmap(
      lovestruckFaceEffectGeneratedSize(anchors.face),
      pink,
    )
    const faceHeight = Math.max(1, anchors.face.y1 - anchors.face.y0)
    const left = Math.round(anchors.face.cx - bitmap.width / 2)
    const top = Math.round(anchors.face.y0 + faceHeight * 0.12)
    const faceLayer = layers.find((layer) => layer.role === 'face')
    if (faceLayer) {
      clipBitmapAlphaToLayer(
        bitmap.data,
        bitmap.width,
        bitmap.height,
        left,
        top,
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
      left,
      top,
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
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
      left: Math.round(
        anchors.mouth.cx + mouthWidth * 0.5 - bitmap.width * 0.36,
      ),
      top: Math.round(anchors.mouth.cy + mouthHeight * 0.08),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
      synthetic: true,
    })
  }

  return generated.length > 0 ? [...layers, ...generated] : layers
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
      left: Math.round(eye.icx + inwardOffset - bitmap.width / 2),
      // Sink the shadow into the eyewhite edge. Its lower depth lets the
      // eyewhite crop the overlap, so the visible shadow starts flush with the
      // lower lid instead of floating on the cheek.
      top: Math.round(eye.y1 - eyeHeight * 0.22),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
      synthetic: true,
    })
  }
  if (generated.length === 0) return layers

  const output = [...layers]
  const firstEyeLayer = output.findIndex(
    (layer) =>
      layer.role === 'eyewhite' ||
      layer.role === 'irides' ||
      layer.role === 'eyelash' ||
      layer.role === 'eye-close',
  )
  if (firstEyeLayer >= 0) output.splice(firstEyeLayer, 0, ...generated)
  else output.push(...generated)
  return output
}

function synthesizeMissingMouthExpressions(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
): RasterLayer[] {
  const reference =
    layers.find((layer) => layer.role === 'mouth-close') ||
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
  const usedIds = new Set(layers.map((layer) => layer.id))
  const generated: RasterLayer[] = []
  const add = (
    role: (typeof expressions)[number]['role'],
    kind: MouthExpressionKind,
  ) => {
    const bitmap = createMouthExpressionBitmap(kind, sizes[kind], palette)
    generated.push({
      id: uniquePartId(role, usedIds),
      role,
      sourceName: role,
      order: 0,
      side: null,
      group: 'head',
      left: Math.round(anchors.mouth.cx - bitmap.width / 2),
      top: Math.round(
        anchors.mouth.cy -
          bitmap.height *
            (kind === 'cry' ? 0.46 : kind === 'maniac' ? 0.61 : 0.5) +
          (kind === 'maniac' ? 3 : 0),
      ),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
      synthetic: true,
    })
  }
  for (const expression of missing) add(expression.role, expression.kind)
  if (needsManiacShadow) {
    const maniacMouth =
      layers.find((layer) => layer.role === 'mouth-maniac') ||
      generated.find((layer) => layer.role === 'mouth-maniac')
    const bitmap = createManiacMouthShadowBitmap(
      maniacMouth
        ? { width: maniacMouth.width, height: maniacMouth.height }
        : sizes.maniac,
      palette,
    )
    generated.unshift({
      id: uniquePartId('maniac-mouth-shadow', usedIds),
      role: 'maniac-mouth-shadow',
      sourceName: 'maniac-mouth-shadow',
      order: 0,
      side: null,
      group: 'head',
      left:
        maniacMouth?.left ?? Math.round(anchors.mouth.cx - bitmap.width / 2),
      top:
        maniacMouth?.top ??
        Math.round(anchors.mouth.cy - bitmap.height * 0.61 + 3),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
      synthetic: true,
    })
  }

  const output = [...layers]
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
  output.splice(insertAt + 1, 0, ...generated)
  return output
}

function synthesizeMissingExpressionSymbols(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
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

  if (needsAnger) {
    const bitmap = createAngerMarkBitmap(sizes.anger, tint)
    const centerX = anchors.face.x0 + faceWidth * 0.13
    const centerY = anchors.face.y0 + faceHeight * 0.22
    generated.push({
      id: uniquePartId('anger-mark', usedIds),
      role: 'anger-mark',
      sourceName: 'anger-mark',
      order: 0,
      side: null,
      group: 'head',
      left: Math.round(centerX - bitmap.width / 2),
      top: Math.round(centerY - bitmap.height / 2),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
      synthetic: true,
    })
  }

  if (needsSweat) {
    const bitmap = createSpeechlessSweatBitmap(sizes.speechless)
    const eyeY = anchors.eyeR?.icy ?? anchors.eyeL?.icy
    const centerX = anchors.face.x1 - faceWidth * 0.015
    const centerY = eyeY ?? anchors.face.y0 + faceHeight * 0.48
    generated.push({
      id: uniquePartId('speechless-sweat', usedIds),
      role: 'speechless-sweat',
      sourceName: 'speechless-sweat',
      order: 0,
      side: null,
      group: 'head',
      left: Math.round(centerX - bitmap.width / 2),
      top: Math.round(centerY - bitmap.height * 0.2),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
      synthetic: true,
    })
  }

  return [...layers, ...generated]
}
