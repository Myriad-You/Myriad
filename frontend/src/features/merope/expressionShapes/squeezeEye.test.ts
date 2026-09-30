import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createSqueezeEyeBitmap,
  squeezeEyeDisplayScale,
  squeezeEyeGeneratedSize,
} from './squeezeEye'

test('generates thick, slightly irregular inward-facing chevrons', () => {
  const tint = { red: 42, green: 31, blue: 68 }
  const size = { width: 72, height: 36 }
  const left = createSqueezeEyeBitmap(size, tint, 'left')
  const right = createSqueezeEyeBitmap(size, tint, 'right')
  assert.equal(left.width, 72)
  assert.equal(left.height, 36)
  assert.equal(left.data.length, 72 * 36 * 4)

  let leftVisible = 0
  let rightVisible = 0
  let mirroredAlphaDifference = 0
  for (let y = 0; y < left.height; y += 1) {
    for (let x = 0; x < left.width; x += 1) {
      const leftOffset = (y * left.width + x) * 4
      const rightOffset = (y * right.width + (right.width - 1 - x)) * 4
      const leftAlpha = left.data[leftOffset + 3]
      const rightAlpha = right.data[rightOffset + 3]
      if (leftAlpha > 0) {
        leftVisible += 1
        assert.equal(left.data[leftOffset], tint.red)
        assert.equal(left.data[leftOffset + 1], tint.green)
        assert.equal(left.data[leftOffset + 2], tint.blue)
      }
      if (rightAlpha > 0) rightVisible += 1
      mirroredAlphaDifference += Math.abs(leftAlpha - rightAlpha)
    }
  }
  assert.ok(leftVisible > 300)
  assert.ok(rightVisible > 300)
  assert.ok(leftVisible < left.width * left.height * 0.5)
  assert.ok(mirroredAlphaDifference > 5_000)
  assert.ok(alphaAt(left, 0.82, 0.49) > 128, 'left eye points inward as >')
  assert.ok(alphaAt(right, 0.19, 0.51) > 128, 'right eye points inward as <')
})

test('fits generated chevrons to each eye without forcing a square', () => {
  assert.deepEqual(
    squeezeEyeGeneratedSize({ x0: 10, x1: 90, y0: 20, y1: 52 }),
    { width: 83, height: 42 },
  )
})

test('display scale enlarges undersized marks and leaves fitted art alone', () => {
  const eye = { x0: 10, x1: 90 }
  assert.ok(squeezeEyeDisplayScale(28, eye) > 2)
  assert.equal(squeezeEyeDisplayScale(83.2, eye), 1)
})

function alphaAt(
  bitmap: { width: number; height: number; data: Uint8ClampedArray },
  normalizedX: number,
  normalizedY: number,
): number {
  const x = Math.round(normalizedX * (bitmap.width - 1))
  const y = Math.round(normalizedY * (bitmap.height - 1))
  return bitmap.data[(y * bitmap.width + x) * 4 + 3]
}
