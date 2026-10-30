import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { applyCropBoundary, bindCropBoundary } from './cropBoundary'

function mesh(role: string, bottom = 100) {
  const rest = new Float32Array([0, 0, 50, 0, 100, 0, 0, 80, 50, 80, 100, 80, 0, bottom, 50, bottom, 100, bottom])
  return { source: { role, x: 0, y: 0, w: 100, h: bottom } as Anime25DPlaybackLayer, rest, deformed: rest.slice() }
}
const opaque = () => ({ width: 100, height: 100, pixels: new Uint8ClampedArray(100 * 100 * 4).fill(255) })

test('shared cut follows the moving, tilted torso without flattening the upper mesh', () => {
  const hair = mesh('back-hair'); const torso = mesh('topwear')
  const binding = bindCropBoundary(hair, torso, opaque(), opaque(), 100)!
  assert.ok(binding)
  for (let frame = 0; frame < 100; frame++) {
    hair.deformed.set(hair.rest)
    for (let i = 1; i < hair.deformed.length; i += 2) hair.deformed[i] += 20
    torso.deformed.set(torso.rest)
    for (let i = 0; i < torso.deformed.length; i += 2) torso.deformed[i + 1] += frame * 0.01 + torso.deformed[i] * 0.1
    assert.equal(applyCropBoundary(binding, hair.deformed), true)
    assert.deepEqual([...hair.deformed.slice(0, 6)], [0, 20, 50, 20, 100, 20])
    for (const i of binding.ownEdge) assert.ok(Math.abs(hair.deformed[i + 1] - torso.deformed[i + 1]) < 0.0001)
    for (let i = 0; i < hair.rest.length; i += 2) assert.equal(hair.deformed[i], hair.rest[i])
  }
})

test('natural tips, inset transparency, unmatched cuts and ornaments are not bound', () => {
  const hair = mesh('back-hair'); const torso = mesh('topwear')
  const tips = opaque()
  tips.pixels.fill(0, 99 * 100 * 4)
  tips.pixels.fill(255, (99 * 100 + 49) * 4, (99 * 100 + 52) * 4)
  assert.equal(bindCropBoundary(hair, torso, tips, opaque(), 100), null)
  assert.equal(bindCropBoundary(hair, torso, opaque(), tips, 100), null)
  assert.equal(bindCropBoundary(mesh('back-hair', 99), torso, opaque(), opaque(), 100), null)
  assert.equal(bindCropBoundary(hair, torso, opaque(), opaque(), 110), null)
  assert.equal(bindCropBoundary(mesh('neckwear'), torso, opaque(), opaque(), 100), null)
  assert.equal(bindCropBoundary(hair, torso, null, opaque(), 100), null)
  assert.ok(bindCropBoundary(mesh('handwear'), torso, opaque(), opaque(), 100))
})

test('a folded host boundary is not propagated', () => {
  const hair = mesh('back-hair'); const torso = mesh('topwear')
  const binding = bindCropBoundary(hair, torso, opaque(), opaque(), 100)!
  torso.deformed[14] = -20
  assert.equal(applyCropBoundary(binding, hair.deformed), false)
  assert.deepEqual(hair.deformed, hair.rest)
})
