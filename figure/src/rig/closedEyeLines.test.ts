import assert from 'node:assert/strict'
import test from 'node:test'
import { closedEyeLineShare, keepClosedEyeLines } from './closedEyeLines'

test('a closed eye keeps its dark lines and lets a light fill go', () => {
  // Measured on the decomposed closed eyes: lashes and crease, and the pink painted on the lid.
  assert.equal(closedEyeLineShare(45, 28, 30), 1, 'the lashes')
  assert.equal(closedEyeLineShare(75, 46, 51), 1, 'the crease line')
  assert.equal(closedEyeLineShare(250, 149, 169), 0, 'a pink fill')
  assert.equal(closedEyeLineShare(243, 220, 207), 0, 'a skin fill')
  const between = closedEyeLineShare(157, 95, 106)
  assert.ok(between > 0 && between < 1, 'a line edge fades rather than cuts')
  const pixels = Uint8ClampedArray.of(45, 28, 30, 185, 250, 149, 169, 204, 0, 0, 0, 0)
  const kept = keepClosedEyeLines(pixels)
  assert.deepEqual([...kept], [45, 28, 30, 185, 250, 149, 169, 0, 0, 0, 0, 0])
  assert.equal(pixels[7], 204, 'the drawing it was given stays as it was')
})
