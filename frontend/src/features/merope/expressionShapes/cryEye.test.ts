import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createCryEyeBitmap,
  cryEyeDisplayScale,
  cryEyeGeneratedSize,
} from './cryEye'

test('builds asymmetric crying replacements with a stable dark eye and blue lower stream', () => {
  const tint = { red: 42, green: 31, blue: 68 }
  const size = { width: 88, height: 108 }
  const left = createCryEyeBitmap(size, tint, 'left')
  const right = createCryEyeBitmap(size, tint, 'right')
  assert.equal(left.data.length, size.width * size.height * 4)

  const leftTop = visibleColorCount(left, 0, 0.38)
  const leftBottom = visibleColorCount(left, 0.38, 1)
  assert.ok(leftTop.dark > 180, 'squeeze artwork remains prominent')
  assert.ok(leftBottom.blue > 300, 'tear stream remains visible below the eye')
  assert.ok(leftBottom.highlight > 15, 'tear stream retains a water highlight')

  let mirroredAlphaDifference = 0
  for (let y = 0; y < left.height; y += 1) {
    for (let x = 0; x < left.width; x += 1) {
      const leftAlpha = left.data[(y * left.width + x) * 4 + 3]
      const rightAlpha =
        right.data[(y * right.width + (right.width - 1 - x)) * 4 + 3]
      mirroredAlphaDifference += Math.abs(leftAlpha - rightAlpha)
    }
  }
  assert.ok(mirroredAlphaDifference > 20_000)
})

test('sizes each crying replacement from its own eye bounds', () => {
  assert.deepEqual(cryEyeGeneratedSize({ x0: 10, x1: 90, y0: 20, y1: 52 }), {
    width: 88,
    height: 131,
  })
  assert.deepEqual(cryEyeGeneratedSize({ x0: 10, x1: 70, y0: 20, y1: 48 }), {
    width: 66,
    height: 98,
  })
})

test('display scale only enlarges undersized authored crying art', () => {
  const eye = { x0: 10, x1: 90 }
  assert.ok(cryEyeDisplayScale(36, eye) > 2)
  assert.equal(cryEyeDisplayScale(88, eye), 1)
})

function visibleColorCount(
  bitmap: { width: number; height: number; data: Uint8ClampedArray },
  startY: number,
  endY: number,
): { dark: number; blue: number; highlight: number } {
  const output = { dark: 0, blue: 0, highlight: 0 }
  const from = Math.floor(bitmap.height * startY)
  const to = Math.ceil(bitmap.height * endY)
  for (let y = from; y < to; y += 1) {
    for (let x = 0; x < bitmap.width; x += 1) {
      const offset = (y * bitmap.width + x) * 4
      if (bitmap.data[offset + 3] < 24) continue
      const red = bitmap.data[offset]
      const green = bitmap.data[offset + 1]
      const blue = bitmap.data[offset + 2]
      if (red < 80 && green < 80 && blue < 110) output.dark += 1
      if (blue > red * 1.25 && blue > green) output.blue += 1
      if (red > 210 && green > 230 && blue > 235) output.highlight += 1
    }
  }
  return output
}
