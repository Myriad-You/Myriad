import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { trimRaster } from './anime25dRaster'
import { addAuthoredExpressionLayers, registerExpression } from './authoredExpression'

const SIZE = 200
const SKIN = [236, 206, 196] as const
const HAIR = [168, 176, 232] as const
const LASH = [52, 36, 60] as const

type Rgb = readonly [number, number, number]
type Paint = (x: number, y: number) => Rgb | null

const anchors: Anime25DRiggerAnchors = {
  face: { cx: 100, cy: 110, x0: 40, x1: 160, y0: 50, y1: 170 },
  eyeL: { x0: 60, y0: 80, x1: 92, y1: 96, icx: 76, icy: 88, closeY: 92 },
  mouth: { x0: 90, x1: 110, y0: 140, y1: 146, cx: 100, cy: 143 },
  neckPivot: { cx: 100, cy: 175 },
  neckTop: 170,
  neckBottom: 190,
  bodyPivot: { cx: 100, cy: 190 },
  faceScale: 1,
}

// Hair outside the face gives registration something to lock onto; the
// pattern must not repeat, or a larger drift would pass for a smaller one.
const backdrop: Paint = (x, y) =>
  (x < 40 || x >= 160 || y < 50) && Math.imul(x * 73856093 ^ y * 19349663, 2654435761) >>> 29 < 3
    ? HAIR
    : null
// A bang crossing the outer corner of the eye.
const bang: Paint = (x, y) => (x >= 58 && x < 64 && y >= 60 && y < 100 ? HAIR : null)
const openEye: Paint = (x, y) => (x >= 62 && x < 90 && y >= 82 && y < 94 ? LASH : null)
const closedEye: Paint = (x, y) => (x >= 62 && x < 90 && y >= 91 && y < 94 ? LASH : null)

function picture(...paints: Paint[]): Anime25DSourceReference {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      const colour = paints.reduce<Rgb>((under, paint) => paint(x, y) ?? under, SKIN)
      data.set([...colour, 255], (y * SIZE + x) * 4)
    }
  }
  return { width: SIZE, height: SIZE, data }
}

function layer(role: RasterLayer['role'], paint: Paint, side: RasterLayer['side'] = null): RasterLayer {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      const colour = paint(x, y)
      if (colour) data.set([...colour, 255], (y * SIZE + x) * 4)
    }
  }
  // Importer layers are trimmed to their paint.
  return trimRaster({ id: role, role, sourceName: role, order: 0, side, group: 'head', left: 0, top: 0, width: SIZE, height: SIZE, data })
}

function shifted(source: Anime25DSourceReference, dx: number): Anime25DSourceReference {
  const data = new Uint8ClampedArray(source.data.length)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      const from = Math.min(SIZE - 1, Math.max(0, x - dx))
      data.set(source.data.subarray((y * SIZE + from) * 4, (y * SIZE + from) * 4 + 4), (y * SIZE + x) * 4)
    }
  }
  return { ...source, data }
}

function alphaAt(part: RasterLayer, x: number, y: number): number {
  const lx = x - part.left
  const ly = y - part.top
  if (lx < 0 || ly < 0 || lx >= part.width || ly >= part.height) return 0
  return part.data[(ly * part.width + lx) * 4 + 3]
}

const neutral = picture(backdrop, openEye, bang)
const squeezed = picture(backdrop, closedEye, bang)
const rig = [
  layer('face', (x, y) => (x >= 40 && x < 160 && y >= 50 && y < 170 ? SKIN : null)),
  layer('eyelash', openEye, 'left'),
  layer('front-hair', (x, y) => backdrop(x, y) ?? bang(x, y)),
]

test('registers a redraw in place and rejects one that drifted', () => {
  assert.deepEqual(registerExpression(neutral, squeezed, anchors.face), { x: 0, y: 0 })
  assert.deepEqual(registerExpression(neutral, shifted(squeezed, 2), anchors.face), { x: 2, y: 0 })
  assert.equal(registerExpression(neutral, shifted(squeezed, 6), anchors.face), null)
})

test('cuts the redrawn eye against the skin, leaving hair to the hair layer', () => {
  const output = addAuthoredExpressionLayers(rig, anchors, neutral, [
    { kind: 'squeeze', ...squeezed },
  ])
  const part = output.find((candidate) => candidate.role === 'eye-squeeze')
  assert.ok(part, 'authored squeeze eye is added')
  assert.equal(part.side, 'left')
  assert.equal(alphaAt(part, 76, 92), 255, 'the closed lid is opaque')
  assert.equal(alphaAt(part, 76, 85), 0, 'where the open eye was, skin shows through')
  assert.equal(alphaAt(part, 60, 92), 0, 'the bang over the lid is not copied')
  assert.ok(
    output.indexOf(part) > output.findIndex((candidate) => candidate.role === 'eyelash'),
    'drawn above the open eye it replaces',
  )
})

test('keeps procedural parts when the redraw cannot be trusted', () => {
  const drifted = addAuthoredExpressionLayers(rig, anchors, neutral, [
    { kind: 'squeeze', ...shifted(squeezed, 6) },
  ])
  assert.equal(drifted, rig)
  const authored = [...rig, layer('eye-squeeze', closedEye, 'left')]
  assert.equal(
    addAuthoredExpressionLayers(authored, anchors, neutral, [{ kind: 'squeeze', ...squeezed }]),
    authored,
    'artist-drawn parts win',
  )
})

test('a redrawn blink replaces the generic closed eye but never an authored one', () => {
  const generic = { ...layer('eye-close', closedEye, 'left'), synthetic: true }
  const replaced = addAuthoredExpressionLayers([...rig, generic], anchors, neutral, [
    { kind: 'close', ...squeezed },
  ])
  const closed = replaced.filter((candidate) => candidate.role === 'eye-close')
  assert.equal(closed.length, 1)
  assert.notEqual(closed[0], generic)
  assert.equal(closed[0].synthetic, undefined)

  const authored = layer('eye-close', closedEye, 'left')
  const kept = [...rig, authored]
  assert.equal(
    addAuthoredExpressionLayers(kept, anchors, neutral, [{ kind: 'close', ...squeezed }]),
    kept,
  )
})
