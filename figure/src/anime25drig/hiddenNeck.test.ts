import assert from 'node:assert/strict'
import test from 'node:test'
import { trimHiddenNeck } from './hiddenNeck'

function image(width: number, height: number, opaque: (x: number, y: number) => boolean) {
  const pixels = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (opaque(x, y)) pixels[(y * width + x) * 4 + 3] = 255
    }
  }
  return { pixels, width, height }
}

// A neck 20 wide showing below y = 40; above it the decomposition painted a
// field 60 wide behind the face.
const neck = { x: 0, y: 0, w: 60, h: 80 }
const neckImage = image(60, 80, (x, y) => (y < 40 ? true : x >= 20 && x < 40))
const face = { x: 0, y: 0, w: 60, h: 40 }
const faceImage = image(60, 40, () => true)
const alpha = (out: { pixels: Uint8ClampedArray }, x: number, y: number) => out.pixels[(y * 60 + x) * 4 + 3]

test('behind the face the neck goes on up as a neck, not a field', () => {
  const out = trimHiddenNeck(neck, neckImage, face, faceImage)!
  assert.ok(out)
  assert.equal(alpha(out, 30, 10), 255)
  assert.equal(alpha(out, 21, 30), 255)
  assert.equal(alpha(out, 5, 10), 0)
  assert.equal(alpha(out, 55, 30), 0)
  // What shows is left as drawn.
  for (let y = 40; y < 80; y++) assert.equal(alpha(out, 25, y), 255)
})

test('a neck the face does not cover is left alone', () => {
  assert.equal(trimHiddenNeck(neck, neckImage, { x: 0, y: 200, w: 60, h: 40 }, faceImage), null)
})

test('a sliver of neck beside the jaw does not narrow the neck carried up', () => {
  // The face ends at y = 40 except on its right, where the jaw stops at x = 50
  // from y = 25: the neck field shows there as a 4 px sliver before the chin.
  const jawFace = image(60, 40, (x, y) => !(y >= 25 && x >= 50))
  const field = image(60, 80, (x, y) => (y < 40 ? x < 54 : x >= 20 && x < 40))
  const out = trimHiddenNeck(neck, field, face, jawFace)!
  assert.ok(out)
  // The neck itself (x 20..40) goes on up behind the chin.
  assert.equal(alpha(out, 30, 30), 255)
  assert.equal(alpha(out, 22, 10), 255)
  assert.equal(alpha(out, 5, 10), 0)
})
