import assert from 'node:assert/strict'
import test from 'node:test'
import { createExpressionAccentBitmap, expressionAccentSize } from './expressionAccents'

const ACCENTS = ['notes', 'question', 'sleep', 'sparkle', 'blush'] as const

function coverage(bitmap: { width: number; height: number; data: Uint8ClampedArray }) {
  let opaque = 0
  for (let index = 3; index < bitmap.data.length; index += 4) { if (bitmap.data[index] > 32) opaque += 1
}
  return opaque / (bitmap.width * bitmap.height)
}

test('each accent is drawn inside its box, neither empty nor a filled block', () => {
  for (const accent of ACCENTS) {
    const bitmap = createExpressionAccentBitmap(accent, expressionAccentSize(accent, 300, 190))
    assert.equal(bitmap.data.length, bitmap.width * bitmap.height * 4, accent)
    const share = coverage(bitmap)
    assert.ok(share > 0.08 && share < 0.75, `${accent}: ${share.toFixed(2)} of the box drawn`)
    const corner = bitmap.data[3]
    assert.equal(corner, 0, `${accent}: the corner stays clear`)
  }
})

test('accents grow with the face, the blush spans the eyes', () => {
  const small = expressionAccentSize('notes', 200)
  const large = expressionAccentSize('notes', 400)
  assert.ok(large.height > small.height)
  const blush = expressionAccentSize('blush', 300, 190)
  assert.ok(blush.width > 190 && blush.height < blush.width / 4)
})
