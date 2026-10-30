import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { ExpressionAccent } from '../expressionShapes/expressionAccents'
import type { Anime25DFaceFrame } from '../expressionShapes/faceFrame'
import type { Anime25DLayerRole } from './anime25d'
import type { RasterLayer } from './anime25dImportTypes'
import { createExpressionAccentBitmap, expressionAccentSize } from '../expressionShapes/expressionAccents'
import { faceFramePoint, placeFaceBitmap } from '../expressionShapes/faceFrame'
import { uniquePartId } from './anime25dRaster'

/**
 * Drawn accents beside her face, in the order they are kept when the part
 * budget runs short: a face that cannot fit one simply does not offer it.
 */
export const EXPRESSION_ACCENT_ROLES = [
  ['accent-music-notes', 'notes'],
  ['accent-blush', 'blush'],
  ['accent-question', 'question'],
  ['accent-sleep', 'sleep'],
  ['accent-sparkle', 'sparkle'],
] as const satisfies readonly (readonly [Anime25DLayerRole, ExpressionAccent])[]

/**
 * Adds the accents a package lacks, as many as `budget` parts allow. They
 * hang off the face, turned with its roll, and are kept inside the picture
 * the other layers cover so none is cut off at the edge.
 */
export function synthesizeMissingExpressionAccents(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
  budget: number,
): RasterLayer[] {
  const faceWidth = Math.max(1, anchors.face.x1 - anchors.face.x0)
  const faceHeight = Math.max(1, anchors.face.y1 - anchors.face.y0)
  const eyeSpan =
    anchors.eyeL && anchors.eyeR
      ? Math.abs(anchors.eyeR.icx - anchors.eyeL.icx) + (anchors.eyeL.x1 - anchors.eyeL.x0)
      : faceWidth * 0.62
  const bounds = layers.reduce(
    (box, layer) => ({
      left: Math.min(box.left, layer.left),
      top: Math.min(box.top, layer.top),
      right: Math.max(box.right, layer.left + layer.width),
      bottom: Math.max(box.bottom, layer.top + layer.height),
    }),
    { left: Infinity, top: Infinity, right: -Infinity, bottom: -Infinity },
  )
  const usedIds = new Set(layers.map((layer) => layer.id))
  const generated: RasterLayer[] = []
  let room = Math.max(0, budget)
  const center = { x: anchors.face.cx, y: anchors.face.cy }
  for (const [role, accent] of EXPRESSION_ACCENT_ROLES) {
    if (room <= 0) break
    if (layers.some((layer) => layer.role === role)) continue
    const bitmap = createExpressionAccentBitmap(
      accent,
      expressionAccentSize(accent, faceWidth, eyeSpan),
    )
    const target = accentTarget(accent, anchors, frame, center, faceWidth, faceHeight)
    const placed = placeFaceBitmap(bitmap, bitmap.width / 2, bitmap.height / 2, target, frame.roll)
    const left = Math.round(
      Math.min(Math.max(placed.left, bounds.left), bounds.right - placed.width),
    )
    const top = Math.round(
      Math.min(Math.max(placed.top, bounds.top), bounds.bottom - placed.height),
    )
    generated.push({
      id: uniquePartId(role, usedIds),
      role,
      sourceName: role,
      order: 0,
      side: null,
      group: 'head',
      ...placed,
      left,
      top,
      synthetic: true,
    })
    room -= 1
  }
  return [...layers, ...generated]
}

function accentTarget(
  accent: ExpressionAccent,
  anchors: Anime25DRiggerAnchors,
  frame: Readonly<Anime25DFaceFrame>,
  center: { x: number; y: number },
  faceWidth: number,
  faceHeight: number,
): { x: number; y: number } {
  switch (accent) {
    case 'notes':
    case 'sleep':
      return faceFramePoint(frame, center.x, center.y, faceWidth * 0.66, -faceHeight * 0.5)
    case 'question':
      return faceFramePoint(frame, center.x, center.y, -faceWidth * 0.64, -faceHeight * 0.52)
    case 'sparkle':
      return faceFramePoint(frame, center.x, center.y, -faceWidth * 0.68, -faceHeight * 0.18)
    case 'blush': {
      const eyes = [anchors.eyeL, anchors.eyeR].filter((eye) => eye !== undefined)
      if (eyes.length === 0) {
        return faceFramePoint(frame, center.x, center.y, 0, faceHeight * 0.12)
      }
      const x = eyes.reduce((sum, eye) => sum + eye.icx, 0) / eyes.length
      const y = eyes.reduce((sum, eye) => sum + Math.max(eye.closeY, eye.y1), 0) / eyes.length
      const eyeHeight =
        eyes.reduce((sum, eye) => sum + (eye.y1 - eye.y0), 0) / eyes.length
      return faceFramePoint(frame, x, y, 0, eyeHeight * 0.5)
    }
  }
}
