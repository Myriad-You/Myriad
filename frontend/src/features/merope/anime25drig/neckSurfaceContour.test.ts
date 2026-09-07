import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildNeckSurfaceContour,
  NECK_SURFACE_COLUMNS,
} from './neckSurfaceContour'

test('the blend band follows a slanted join while staying inside supported skin', () => {
  const width = 160
  const height = 200
  const agreement = new Uint8Array(width * height).fill(1)
  for (let x = 0; x < width; x++) {
    const top = 130 + Math.floor(x / 8)
    for (let y = top; y < top + 22; y++) agreement[y * width + x] = 2
  }
  const contour = buildNeckSurfaceContour(
    width,
    height,
    { left: 0, right: width, top: 0, bottom: height },
    agreement,
    125,
    185,
  )
  assert.ok(contour.bands.at(-2)! - contour.bands[0] > 0.06)
  for (let column = 0; column < NECK_SURFACE_COLUMNS; column++) {
    const start = contour.bands[column * 2]
    const end = contour.bands[column * 2 + 1]
    assert.ok(start >= 125 / height && end <= 186 / height)
    assert.ok(end - start >= 0.04)
    if (column) assert.ok(start >= contour.bands[(column - 1) * 2])
  }
})

test('isolated matching pixels cannot pinch or move the seam', () => {
  const agreement = new Uint8Array(80 * 100).fill(1)
  for (let x = 0; x < 80; x++) agreement[85 * 80 + x] = 2
  const contour = buildNeckSurfaceContour(
    80,
    100,
    { left: 0, right: 80, top: 0, bottom: 100 },
    agreement,
    80,
    95,
  )
  for (let column = 0; column < NECK_SURFACE_COLUMNS; column++) {
    assert.ok(Math.abs(contour.bands[column * 2] - 0.805) < 1e-6)
    assert.ok(Math.abs(contour.bands[column * 2 + 1] - 0.955) < 1e-6)
  }
})
