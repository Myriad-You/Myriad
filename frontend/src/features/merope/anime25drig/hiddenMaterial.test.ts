import assert from 'node:assert/strict'
import test from 'node:test'
import { shadeHiddenBackHair } from './hiddenHair'
import { fillHiddenSkin } from './hiddenSkin'

type Paint = (x: number, y: number) => [number, number, number, number]

function image(width: number, height: number, paint: Paint) {
  const pixels = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) pixels.set(paint(x, y), (y * width + x) * 4)
  }
  return { pixels, width, height }
}

function at(out: { pixels: Uint8ClampedArray; width: number }, x: number, y: number) {
  return Array.from(out.pixels.slice((y * out.width + x) * 4, (y * out.width + x) * 4 + 4))
}

test('the face under the fringe is plain skin, its painted shadow gone', () => {
  const box = { x: 0, y: 0, w: 100, h: 100 }
  // Skin everywhere; a dark stripe of shadow up where the fringe hides it.
  const face = image(100, 100, (_, y) => (y >= 20 && y < 30 ? [120, 80, 80, 255] : [250, 220, 210, 255]))
  const fringe = image(100, 100, (_, y) => [0, 0, 0, y < 40 ? 255 : 0])
  const out = fillHiddenSkin(box, face, [{ layer: box, image: fringe }])!
  assert.ok(out)
  const [r, g, b] = at(out, 50, 25)
  assert.ok(r > 230 && g > 200 && b > 190, `under the fringe ${[r, g, b]}`)
  // What shows is untouched.
  assert.deepEqual(at(out, 50, 70), [250, 220, 210, 255])
})

test('behind the head the back hair is the hair in its own shadow', () => {
  const box = { x: 0, y: 0, w: 100, h: 100 }
  // Hair with a lighter and a darker tone, painted pale behind the face.
  const hair = image(100, 100, (x, y) => (x > 30 && x < 70 && y > 30 ? [250, 240, 245, 255] : x % 10 < 2 ? [150, 90, 120, 255] : [240, 170, 200, 255]))
  const face = image(100, 100, (x, y) => [0, 0, 0, x > 25 && x < 75 && y > 25 ? 255 : 0])
  const out = shadeHiddenBackHair(box, hair, [{ layer: box, image: face }])!
  assert.ok(out)
  const [r, g, b] = at(out, 50, 60)
  assert.ok(Math.abs(r - 150) < 15 && Math.abs(g - 90) < 15 && Math.abs(b - 120) < 15, `behind the face ${[r, g, b]}`)
  assert.deepEqual(at(out, 10, 10), [150, 90, 120, 255])
  assert.deepEqual(at(out, 15, 10), [240, 170, 200, 255])
})
