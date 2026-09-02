import assert from 'node:assert/strict'
import test from 'node:test'
import { localToAtlasUv } from './atlasUv'

test('shared-atlas UV conversion maps layer-local coordinates into the rect', () => {
  const rect = { x: 0.25, y: 0.375, w: 0.125, h: 0.25 }
  assert.deepEqual(localToAtlasUv(rect, 0, 0), [0.25, 0.375])
  assert.deepEqual(localToAtlasUv(rect, 1, 1), [0.375, 0.625])
  const [u, v] = localToAtlasUv(rect, 0.35, 0.72)
  assert.ok(Math.abs(u - 0.29375) < 1e-12)
  assert.ok(Math.abs(v - 0.555) < 1e-12)
})
