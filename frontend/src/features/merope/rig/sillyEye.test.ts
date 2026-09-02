import assert from 'node:assert/strict'
import test from 'node:test'
import { SILLY_IRIS_DRIFT_LIMIT } from '../anime25drig/stylizedExpressionMotion'
import {
  createSillyEyeWhiteBitmap,
  createSillyIrisBitmap,
  createSillyIrisFromArtwork,
  sampleSillyEyePalette,
  SILLY_IRIS_REST_SHARE,
  sillyEyeGeneratedSize,
  sillyIrisTravelRoom,
} from './sillyEye'

const OUTLINE = { red: 38, green: 28, blue: 62 }

test('fits a near-round eye frame over a much wider eye anchor', () => {
  assert.deepEqual(sillyEyeGeneratedSize({ x0: 10, x1: 90, y0: 20, y1: 52 }), {
    width: 90,
    height: 74,
    iris: 40,
  })
})

test('reserves enough room that a drifting iris never leaves the sclera', () => {
  for (const eye of [
    { x0: 10, x1: 90, y0: 20, y1: 52 },
    { x0: 0, x1: 120, y0: 0, y1: 40 },
    { x0: 0, x1: 180, y0: 0, y1: 70 },
  ]) {
    const size = sillyEyeGeneratedSize(eye)
    const room = sillyIrisTravelRoom(size)
    assert.ok(room.x > 0 && room.y > 0)
    // Import seeds the divergence, the runtime loop spends the rest.
    const restX = room.x * SILLY_IRIS_REST_SHARE
    const restY = room.y * SILLY_IRIS_REST_SHARE
    const driftX = SILLY_IRIS_DRIFT_LIMIT.x * (eye.x1 - eye.x0)
    const driftY = SILLY_IRIS_DRIFT_LIMIT.y * (eye.y1 - eye.y0)
    assert.ok(restX + driftX <= room.x, `horizontal budget for ${size.width}`)
    assert.ok(restY + driftY <= room.y, `vertical budget for ${size.height}`)
  }
})

test('samples the character iris hue and keeps the sclera light', () => {
  const irisPixels = solidPixels(96, 64, 210)
  const scleraPixels = solidPixels(248, 246, 250)
  const palette = sampleSillyEyePalette(OUTLINE, irisPixels, scleraPixels)
  assert.ok(palette.iris.blue > palette.iris.red)
  assert.ok(palette.iris.blue > 150)
  assert.ok(luminance(palette.sclera) > 220)
  assert.ok(luminance(palette.pupil) < luminance(palette.irisDark))
  assert.ok(luminance(palette.irisLight) > luminance(palette.iris))

  const fallback = sampleSillyEyePalette(OUTLINE)
  assert.ok(luminance(fallback.sclera) > 220)
  assert.ok(fallback.iris.blue > fallback.iris.red)
})

test('draws a filled round eye instead of a thin expression mark', () => {
  const palette = sampleSillyEyePalette(
    OUTLINE,
    solidPixels(96, 64, 210),
    solidPixels(248, 246, 250),
  )
  const size = sillyEyeGeneratedSize({ x0: 10, x1: 90, y0: 20, y1: 52 })
  const left = createSillyEyeWhiteBitmap(size, palette, 'left')
  const right = createSillyEyeWhiteBitmap(size, palette, 'right')
  assert.equal(left.width, 90)
  assert.equal(left.height, 74)
  assert.equal(left.data.length, 90 * 74 * 4)

  let opaque = 0
  for (let index = 3; index < left.data.length; index += 4) {
    if (left.data[index] > 200) opaque += 1
  }
  assert.ok(opaque > left.width * left.height * 0.6)
  assert.equal(alphaAt(left, 0.5, 0.5), 255)
  assert.equal(alphaAt(left, 0.02, 0.02), 0)
  // A thick dark rim reads as the wide-open cartoon eye the pose needs.
  assert.ok(colorAt(left, 0.5, 0.03) + 40 < colorAt(left, 0.5, 0.5))
  // Both eyes come from the same anchor, so only the tilt may differ.
  assert.notDeepEqual([...left.data], [...right.data])
})

test('reuses the drawn iris rather than inventing a replacement', () => {
  const bitmap = createSillyIrisFromArtwork(irisArtwork(), 40)
  assert.ok(bitmap)
  assert.equal(bitmap.width, 40)
  assert.equal(bitmap.height, 40)
  assert.equal(alphaAt(bitmap, 0.02, 0.02), 0)

  // The authored hues survive the resample, including where they sit.
  const center = rgbAt(bitmap, 0.5, 0.5)
  assert.ok(center.red > 200 && center.blue < 90, 'iris body keeps its colour')
  const upperLeft = rgbAt(bitmap, 0.205, 0.205)
  assert.ok(upperLeft.blue > 150 && upperLeft.red < 90, 'no mirroring or drift')

  const empty = new Uint8ClampedArray(8 * 8 * 4)
  assert.equal(
    createSillyIrisFromArtwork({ data: empty, width: 8, height: 8 }, 40),
    null,
  )
})

test('draws an oversized glossy iris with a dark pupil and highlights', () => {
  const palette = sampleSillyEyePalette(
    OUTLINE,
    solidPixels(96, 64, 210),
    solidPixels(248, 246, 250),
  )
  const left = createSillyIrisBitmap(47, palette, 'left')
  const right = createSillyIrisBitmap(47, palette, 'right')
  assert.equal(left.width, 47)
  assert.equal(left.height, 47)
  assert.equal(alphaAt(left, 0.5, 0.5), 255)
  assert.equal(alphaAt(left, 0.02, 0.02), 0)
  assert.ok(colorAt(left, 0.5, 0.45) < colorAt(left, 0.5, 0.86))

  let brightest = 0
  for (let index = 0; index + 3 < left.data.length; index += 4) {
    if (left.data[index + 3] < 200) continue
    brightest = Math.max(brightest, left.data[index + 1])
  }
  assert.ok(brightest > 200, 'catchlight keeps the stare glossy, not dead')
  assert.notDeepEqual([...left.data], [...right.data])
})

function irisArtwork() {
  const width = 24
  const height = 16
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (Math.hypot(x - 11.5, y - 7.5) > 6) continue
      const offset = (y * width + x) * 4
      const catchlight = x < 9.5 && y < 5.5
      data[offset] = catchlight ? 20 : 226
      data[offset + 1] = catchlight ? 40 : 44
      data[offset + 2] = catchlight ? 210 : 52
      data[offset + 3] = 255
    }
  }
  return { data, width, height }
}

function rgbAt(
  bitmap: { width: number; height: number; data: Uint8ClampedArray },
  normalizedX: number,
  normalizedY: number,
) {
  const offset = pixelOffset(bitmap, normalizedX, normalizedY)
  return {
    red: bitmap.data[offset],
    green: bitmap.data[offset + 1],
    blue: bitmap.data[offset + 2],
  }
}

function solidPixels(red: number, green: number, blue: number) {
  const data = new Uint8ClampedArray(16 * 4)
  for (let index = 0; index < 16; index += 1) {
    data[index * 4] = red
    data[index * 4 + 1] = green
    data[index * 4 + 2] = blue
    data[index * 4 + 3] = 255
  }
  return data
}

function luminance(color: { red: number; green: number; blue: number }) {
  return color.red * 0.299 + color.green * 0.587 + color.blue * 0.114
}

function pixelOffset(
  bitmap: { width: number; height: number },
  normalizedX: number,
  normalizedY: number,
) {
  const x = Math.round(normalizedX * (bitmap.width - 1))
  const y = Math.round(normalizedY * (bitmap.height - 1))
  return (y * bitmap.width + x) * 4
}

function alphaAt(
  bitmap: { width: number; height: number; data: Uint8ClampedArray },
  normalizedX: number,
  normalizedY: number,
): number {
  return bitmap.data[pixelOffset(bitmap, normalizedX, normalizedY) + 3]
}

function colorAt(
  bitmap: { width: number; height: number; data: Uint8ClampedArray },
  normalizedX: number,
  normalizedY: number,
): number {
  const offset = pixelOffset(bitmap, normalizedX, normalizedY)
  return luminance({
    red: bitmap.data[offset],
    green: bitmap.data[offset + 1],
    blue: bitmap.data[offset + 2],
  })
}
