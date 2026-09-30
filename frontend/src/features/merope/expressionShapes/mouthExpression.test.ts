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

test('the narrow mouth is the open mouth drawn flatter, tongue included', () => {
  const palette = sampleMouthExpressionPalette(
    new Uint8ClampedArray([162, 102, 90, 255, 236, 176, 160, 255]),
  )
  const sizes = mouthExpressionGeneratedSizes({ width: 74, height: 26 })
  const open = createMouthExpressionBitmap('open', sizes.open, palette)
  const narrow = createMouthExpressionBitmap('narrow', sizes.narrow, palette)
  assert.ok(narrow.width / narrow.height > (open.width / open.height) * 2)
  const tongue = (data: Uint8ClampedArray) => {
    let count = 0
    for (let index = 0; index < data.length; index += 4) {
      if (data[index + 3] > 160 && data[index] > 180) count += 1
    }
    return count
  }
  assert.ok(countDarkPixels(narrow.data) > 40, 'a readable cavity')
  // Passing through a consonant never drops the tongue the vowels show.
  assert.ok(tongue(narrow.data) > 30, `${tongue(narrow.data)}`)
  assert.ok(tongue(open.data) > 30)
})

test('a tiny painted mouth still speaks at a size its face can read', () => {
  const face = { width: 377, height: 457, mouthToChin: 70 }
  const tiny = mouthExpressionGeneratedSizes({ width: 25, height: 13 }, face)
  for (const kind of ['open', 'wide', 'round', 'narrow'] as const) {
    assert.ok(tiny[kind].width >= face.width * 0.12 * 0.75, `${kind} ${tiny[kind].width}`)
  }
  // A mouth already drawn at a readable size keeps its own proportions.
  const drawn = { width: 59, height: 17 }
  assert.deepEqual(
    mouthExpressionGeneratedSizes(drawn, { width: 407, height: 519, mouthToChin: 70 }).open,
    mouthExpressionGeneratedSizes(drawn).open,
  )
})

test('the wide mouth is drawn level and symmetric', () => {
  const palette = sampleMouthExpressionPalette(undefined)
  const wide = createMouthExpressionBitmap('wide', { width: 80, height: 30 }, palette)
  let difference = 0
  let total = 0
  for (let y = 0; y < wide.height; y++) {
    for (let x = 0; x < wide.width; x++) {
      const alpha = wide.data[(y * wide.width + x) * 4 + 3]
      const mirrored = wide.data[(y * wide.width + wide.width - 1 - x) * 4 + 3]
      difference += Math.abs(alpha - mirrored)
      total += alpha
    }
  }
  assert.ok(difference / total < 0.02, `${difference / total}`)
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
