import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { RasterLayer } from '../rig/anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { compileAnime25DExpressionLayers } from '../rig/anime25dExpressionCompiler'
import { rgbaPrincipalAngleDegrees } from '../rig/closedEyeCompensation'
import {
  estimateAnime25DMouthAnchor,
  faceContourAlongEyeLine,
  placeFaceBitmap,
  resolveAnime25DFaceFrame,
} from './faceFrame'

const FACE = { cx: 200, cy: 200, x0: 100, x1: 300, y0: 80, y1: 330 }

function eye(icx: number, icy: number) {
  return {
    icx,
    icy,
    x0: icx - 20,
    x1: icx + 20,
    y0: icy - 12,
    y1: icy + 12,
    closeY: icy + 4,
  }
}

function anchors(rollDegrees: number): Anime25DRiggerAnchors {
  const roll = (rollDegrees * Math.PI) / 180
  const half = 40
  const dx = Math.cos(roll) * half
  const dy = Math.sin(roll) * half
  // Mouth sits 90px down the tilted face's own vertical.
  const mouthX = 200 - Math.sin(roll) * 90
  const mouthY = 180 + Math.cos(roll) * 90
  return {
    face: FACE,
    eyeL: eye(200 - dx, 180 - dy),
    eyeR: eye(200 + dx, 180 + dy),
    mouth: {
      cx: mouthX,
      cy: mouthY,
      x0: mouthX - 20,
      x1: mouthX + 20,
      y0: mouthY - 6,
      y1: mouthY + 6,
    },
    neckPivot: { cx: 200, cy: 360 },
    neckTop: 320,
    neckBottom: 380,
    bodyPivot: { cx: 200, cy: 512 },
    faceScale: 0.6,
  }
}

function solid(
  role: RasterLayer['role'],
  left: number,
  top: number,
  width: number,
  height: number,
  side: RasterLayer['side'] = null,
): RasterLayer {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let index = 0; index < data.length; index += 4) {
    data.set([200, 120, 110, 255], index)
  }
  return {
    id: `${role}${side ? `-${side}` : ''}`,
    role,
    sourceName: role,
    order: 0,
    side,
    group: 'head',
    left,
    top,
    width,
    height,
    data,
  }
}

function sourceLayers(): RasterLayer[] {
  return [
    solid('face', FACE.x0, FACE.y0, FACE.x1 - FACE.x0, FACE.y1 - FACE.y0),
    solid('mouth-close', 180, 267, 40, 6),
    solid('eyelash', 140, 160, 40, 8, 'left'),
    solid('eyelash', 220, 160, 40, 8, 'right'),
  ]
}

function generated(rollDegrees: number, role: string) {
  const layer = compileAnime25DExpressionLayers(
    sourceLayers(),
    anchors(rollDegrees),
  ).find((candidate) => candidate.role === role && candidate.side !== 'right')
  assert.ok(layer, `${role} is generated`)
  return layer
}

function alphaCentroid(layer: Readonly<RasterLayer>) {
  let total = 0
  let x = 0
  let y = 0
  for (let row = 0; row < layer.height; row += 1) {
    for (let column = 0; column < layer.width; column += 1) {
      const alpha = layer.data[(row * layer.width + column) * 4 + 3]
      total += alpha
      x += (layer.left + column + 0.5) * alpha
      y += (layer.top + row + 0.5) * alpha
    }
  }
  return { x: x / total, y: y / total }
}

test('face frame takes its roll and origin from the iris line', () => {
  const frame = resolveAnime25DFaceFrame(anchors(-8))
  assert.equal(frame.landmarks, true)
  assert.ok(Math.abs((frame.roll * 180) / Math.PI + 8) < 1e-9)
  assert.ok(Math.abs(frame.originX - 200) < 1e-9)
  assert.ok(Math.abs(frame.originY - 180) < 1e-9)
  assert.ok(Math.abs(frame.eyeSpan - 80) < 1e-9)

  assert.equal(resolveAnime25DFaceFrame(anchors(0.2)).roll, 0)
  assert.ok(resolveAnime25DFaceFrame(anchors(40)).roll < (21 * Math.PI) / 180)

  const oneEyed = { ...anchors(-8), eyeR: undefined }
  const fallback = resolveAnime25DFaceFrame(oneEyed)
  assert.equal(fallback.landmarks, false)
  assert.equal(fallback.roll, 0)
})

test('upright placement keeps the existing pivot rounding untouched', () => {
  const bitmap = { width: 9, height: 7, data: new Uint8ClampedArray(9 * 7 * 4) }
  const placed = placeFaceBitmap(bitmap, 4.5, 3.5, { x: 50.2, y: 20.7 }, 0)
  assert.equal(placed.left, Math.round(50.2 - 4.5))
  assert.equal(placed.top, Math.round(20.7 - 3.5))
  assert.equal(placed.data, bitmap.data)
})

test('rolled placement turns the art about its pivot onto the target', () => {
  const width = 60
  const height = 8
  const data = new Uint8ClampedArray(width * height * 4)
  for (let index = 0; index < data.length; index += 4) {
    data.set([10, 20, 30, 255], index)
  }
  const roll = (12 * Math.PI) / 180
  const placed = placeFaceBitmap(
    { width, height, data },
    width / 2,
    height / 2,
    { x: 100, y: 100 },
    roll,
  )
  const angle = rgbaPrincipalAngleDegrees(placed)
  assert.ok(angle !== null && Math.abs(angle - 12) < 1, `angle ${angle}`)
  const center = alphaCentroid({ ...solid('unknown', 0, 0, 1, 1), ...placed })
  assert.ok(Math.hypot(center.x - 100, center.y - 100) < 0.75)
  for (let index = 0; index < placed.data.length; index += 4) {
    if (placed.data[index + 3] === 0) continue
    assert.deepEqual(
      [...placed.data.subarray(index, index + 3)],
      [10, 20, 30],
      'transparent RGB never bleeds into resampled edges',
    )
  }
})

test('contour search stops at the drawn face edge along the eye line', () => {
  const face = solid('face', 100, 80, 200, 250)
  const frame = resolveAnime25DFaceFrame(anchors(0))
  const right = faceContourAlongEyeLine(frame, face, { x: 240, y: 180 }, 1)
  const left = faceContourAlongEyeLine(frame, face, { x: 160, y: 180 }, -1)
  assert.equal(right?.x, 299)
  assert.equal(left?.x, 100)
})

test('mouth estimate follows the eye line instead of a fixed pixel offset', () => {
  const tilted = anchors(-10)
  const frame = resolveAnime25DFaceFrame(tilted)
  const mouth = estimateAnime25DMouthAnchor(frame, tilted.face)
  const alongEyeLine =
    (mouth.cx - frame.originX) * frame.cos +
    (mouth.cy - frame.originY) * frame.sin
  assert.ok(Math.abs(alongEyeLine) < 1e-9, 'mouth stays on the face midline')
  assert.ok(mouth.cy > frame.originY && mouth.cy < tilted.face.y1)
  assert.ok(Math.abs(mouth.x1 - mouth.x0 - frame.eyeSpan * 0.4) < 1e-9)
})

test('generated expression art tilts with the portrait', () => {
  for (const role of ['mouth-wide', 'eye-squeeze', 'maniac-eye-shadow']) {
    const level = rgbaPrincipalAngleDegrees(generated(0, role))
    const tilted = rgbaPrincipalAngleDegrees(generated(-9, role))
    assert.ok(level !== null && tilted !== null)
    assert.ok(
      Math.abs(tilted - level + 9) < 1.5,
      `${role}: ${level} -> ${tilted}`,
    )
  }
})

test('generated mouths stay centred on the drawn mouth when tilted', () => {
  const tilted = anchors(-9)
  const mouth = generated(-9, 'mouth-open')
  const center = alphaCentroid(mouth)
  assert.ok(
    Math.hypot(center.x - tilted.mouth.cx, center.y - tilted.mouth.cy) < 2,
  )
})

test('manga accents hang off the face contour, not the face box corners', () => {
  const level = anchors(0)
  const anger = alphaCentroid(generated(0, 'anger-mark'))
  const sweat = alphaCentroid(generated(0, 'speechless-sweat'))
  assert.ok(anger.x > level.face.x0 && anger.y < level.eyeL!.icy)
  assert.ok(Math.abs(sweat.x - (level.face.x1 - 3)) < 6)

  const tiltedAnger = alphaCentroid(generated(-9, 'anger-mark'))
  const tiltedSweat = alphaCentroid(generated(-9, 'speechless-sweat'))
  assert.ok(
    tiltedSweat.y < sweat.y - 2,
    'the raised right eye lifts its sweat drop with it',
  )
  assert.ok(Math.abs(tiltedAnger.y - anger.y) > 2, 'anger follows the tilt')
})
