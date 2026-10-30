import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import { openEyeShare } from './mouthRuntime'

test('a held squint is the open eye alone; the closed drawing only takes a slit', () => {
  const ease = IDENTITY_DRIVER.eyeEase
  // The director's tiers: slight 0.65, clear 0.35; a heavy squint 0.25.
  for (const open of [1, 0.65, 0.35, 0.3, 0.26]) assert.equal(openEyeShare(open, ease), 1, `${open}`)
  for (const open of [0.18, 0.1, 0]) assert.equal(openEyeShare(open, ease), 0, `${open}`)
  const mid = openEyeShare(0.22, ease)
  assert.ok(mid > 0 && mid < 1)
  // Eyes that close more readily hand over to the closed drawing sooner.
  assert.ok(openEyeShare(0.3, 1) < openEyeShare(0.3, ease))
})
