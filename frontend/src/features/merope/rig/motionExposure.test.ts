import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { findMotionExposure } from './motionExposure'

const SIZE = 160

type Region = (x: number, y: number) => boolean

const face: Region = (x, y) => x >= 30 && x < 130 && y >= 30 && y < 150
const eye: Region = (x, y) => x >= 52 && x < 72 && y >= 72 && y < 82
const iris: Region = (x, y) => x >= 58 && x < 66 && y >= 72 && y < 82
const mouth: Region = (x, y) => x >= 74 && x < 86 && y >= 121 && y < 125
const bangs: Region = (x, y) => x >= 20 && x < 140 && y >= 20 && y < 56
const backHair: Region = (x, y) => x >= 10 && x < 150 && y >= 10 && y < 160

function layer(
  role: RasterLayer['role'],
  region: Region,
  side: RasterLayer['side'] = null,
): RasterLayer {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      if (region(x, y)) data[(y * SIZE + x) * 4 + 3] = 255
    }
  }
  return { id: role, role, sourceName: role, order: 0, side, group: 'head', left: 0, top: 0, width: SIZE, height: SIZE, data }
}

const without = (region: Region, hole: Region): Region => (x, y) => region(x, y) && !hole(x, y)

function rig(overrides: Partial<Record<'face' | 'eyewhite', Region>> = {}): RasterLayer[] {
  return [
    layer('back-hair', backHair),
    layer('face', overrides.face ?? face),
    layer('eyewhite', overrides.eyewhite ?? eye, 'left'),
    layer('irides', iris, 'left'),
    layer('eyelash', (x, y) => eye(x, y) && y < 74, 'left'),
    layer('mouth-close', mouth),
    layer('front-hair', bangs),
  ]
}

function checks(layers: RasterLayer[]) {
  return findMotionExposure(layers).map((finding) => finding.check)
}

test('a fully painted rig uncovers nothing, whatever its silhouette does', () => {
  assert.deepEqual(checks(rig()), [])
})

test('reports the face left unpainted under the eyes and the mouth, even with hair behind', () => {
  assert.deepEqual(checks(rig({ face: without(face, eye) })), ['face-under-eyes'])
  assert.deepEqual(checks(rig({ face: without(face, mouth) })), ['face-under-mouth'])
})
