import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createMouthExpressionBitmap,
  mouthExpressionGeneratedSizes,
  sampleMouthExpressionPalette,
} from './mouthExpression'

test('builds distinct flat anime speaking and crying mouth glyphs', () => {
  const palette = sampleMouthExpressionPalette(
    new Uint8ClampedArray([72, 42, 63, 255, 246, 186, 193, 255]),
  )
  const sizes = mouthExpressionGeneratedSizes({ width: 57, height: 19 })
  const open = createMouthExpressionBitmap('open', sizes.open, palette)
  const cry = createMouthExpressionBitmap('cry', sizes.cry, palette)

  assert.ok(cry.width > open.width)
  assert.ok(cry.height > open.height)
  assert.equal(open.data.length, open.width * open.height * 4)
  assert.equal(cry.data.length, cry.width * cry.height * 4)

  const openColors = visibleColors(open.data)
  const cryColors = visibleColors(cry.data)
  assert.ok(
    openColors.size <= 12,
    'only cel colors and antialiased edges remain',
  )
  assert.ok(cryColors.size <= 6, 'cry art stays a two-color cel glyph')
  assert.ok(
    countDarkPixels(open.data) > 40,
    'speaking mouth has a readable cavity',
  )
  assert.ok(
    countPinkPixels(open.data) > 15,
    'speaking mouth has a flat lower accent',
  )
  assert.ok(countPinkPixels(cry.data) > 100, 'cry mouth is a flat pink symbol')
})

test('uses the source dark line while preventing a pale invisible outline', () => {
  const dark = sampleMouthExpressionPalette(
    new Uint8ClampedArray([44, 30, 58, 255, 244, 210, 215, 255]),
  )
  assert.ok(dark.line.red < 100)
  const pale = sampleMouthExpressionPalette(
    new Uint8ClampedArray([245, 220, 224, 255, 250, 228, 232, 255]),
  )
  assert.ok(pale.line.red < 150)
})

function visibleColors(data: Uint8ClampedArray): Set<string> {
  const colors = new Set<string>()
  for (let index = 0; index < data.length; index += 4) {
    if (data[index + 3] < 250) continue
    colors.add(`${data[index]},${data[index + 1]},${data[index + 2]}`)
  }
  return colors
}

function countDarkPixels(data: Uint8ClampedArray): number {
  let count = 0
  for (let index = 0; index < data.length; index += 4) {
    if (data[index + 3] > 160 && data[index] < 150) count += 1
  }
  return count
}

function countPinkPixels(data: Uint8ClampedArray): number {
  let count = 0
  for (let index = 0; index < data.length; index += 4) {
    if (
      data[index + 3] > 160 &&
      data[index] > data[index + 1] * 1.2 &&
      data[index] > data[index + 2] * 1.05
    ) {
      count += 1
    }
  }
  return count
}
