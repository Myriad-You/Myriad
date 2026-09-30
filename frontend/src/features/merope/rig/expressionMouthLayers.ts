import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DFaceFrame } from '../expressionShapes/faceFrame'
import type { MouthExpressionKind } from '../expressionShapes/mouthExpression'
import type { RasterLayer } from './anime25dImportTypes'
import { placeFaceBitmap } from '../expressionShapes/faceFrame'
import { createLipMouthBitmap, detectPaintedLips, LIP_MOUTH_KINDS, lipMouthSize } from '../expressionShapes/lipMouth'
import { createManiacMouthShadowBitmap, createMouthExpressionBitmap, mouthExpressionGeneratedSizes, sampleMouthExpressionPalette } from '../expressionShapes/mouthExpression'
import { uniquePartId } from './anime25dRaster'

export function synthesizeMissingMouthExpressions(
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
