import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DEyeAnchor } from '../anime25drig/types'
import type { Anime25DFaceFrame } from '../expressionShapes/faceFrame'
import type { RasterLayer } from './anime25dImportTypes'
import { sampleDizzyEyeTint } from '../expressionShapes/dizzyEye'
import { createAngerMarkBitmap, createSpeechlessSweatBitmap, expressionSymbolGeneratedSizes } from '../expressionShapes/expressionSymbols'
import { faceContourAlongEyeLine, faceFramePoint, placeFaceBitmap } from '../expressionShapes/faceFrame'
import { createLovestruckDroolBitmap, createLovestruckFaceEffectBitmap, createLovestruckHeartBitmap, LOVESTRUCK_CHEEK_ROW, lovestruckDroolGeneratedSize, lovestruckFaceEffectGeneratedSize, lovestruckHeartGeneratedSize } from '../expressionShapes/lovestruckExpression'
import { sampleMouthExpressionPalette } from '../expressionShapes/mouthExpression'
import { uniquePartId } from './anime25dRaster'

export function synthesizeMissingLovestruckEffects(
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

export function synthesizeMissingExpressionSymbols(
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
