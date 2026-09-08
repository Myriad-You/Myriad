import assert from 'node:assert/strict'
import test from 'node:test'
import {
  ANIME25D_DEFORMATION_EYE,
  captureAnime25DDeformationChanges,
  createAnime25DDeformationChangeState,
} from './deformationDependencies'
import { IDENTITY_DRIVER } from './driver'
import { Anime25DIrisRebound } from './irisRebound'
import { deformAnime25DUpstreamFeaturePoint } from './layerDeformation'

test('rebound is bounded, frame-rate independent and ends at exact identity', () => {
  for (const fps of [10, 30, 60, 144]) {
    const motion = new Anime25DIrisRebound()
    let peak = 0
    for (let i = 0; i <= fps * 2; i += 1) {
      const time = i / fps
      motion.step(time, time < 0.58 ? time : -1, false, 1)
      peak = Math.max(peak, Math.abs(motion.x - 1))
      assert.ok(Math.abs(motion.x - 1) <= 0.045)
      assert.ok(Math.abs(motion.y - 1) <= 0.045)
      if (time < 0.42 || time >= 1.1)
        assert.deepEqual([motion.x, motion.y], [1, 1])
    }
    assert.ok(peak > 0.003)
  }
})

test('special-expression suppression consumes the blink without delayed rebound', () => {
  const motion = new Anime25DIrisRebound()
  motion.step(0.45, 0.45, true, 1)
  motion.step(0.5, 0.5, false, 1)
  assert.deepEqual([motion.x, motion.y], [1, 1])
  motion.step(1, -1, false, 1)
  motion.step(2.45, 0.45, false, 1)
  motion.step(2.5, 0.5, false, 1)
  assert.notEqual(motion.x, 1)
  motion.step(2.51, 0.51, true, 1)
  assert.deepEqual([motion.x, motion.y], [1, 1])
})

test('rebound waits for visible reopening, expires hidden blinks and has continuous endpoints', () => {
  const motion = new Anime25DIrisRebound()
  motion.step(0.45, 0.45, false, 0.1)
  motion.step(0.5, 0.5, false, 0.4)
  assert.deepEqual([motion.x, motion.y], [1, 1])
  motion.step(0.6, -1, false, 0.6)
  assert.deepEqual([motion.x, motion.y], [1, 1])
  motion.step(0.60001, -1, false, 0.6)
  assert.ok(Math.abs(motion.x - 1) < 1e-8)
  motion.step(0.66, -1, false, 0.9)
  assert.ok(motion.x > 1.01 && motion.x < 1.04, 'reopening stays subtle')
  motion.step(1.11999, -1, false, 1)
  assert.ok(Math.abs(motion.x - 1) < 1e-8)
  motion.step(1.12, -1, false, 1)
  assert.deepEqual([motion.x, motion.y], [1, 1])
  motion.step(2, 0.45, false, 0.1)
  motion.step(2.4, -1, false, 1)
  motion.step(2.5, -1, false, 1)
  assert.deepEqual([motion.x, motion.y], [1, 1])
})

test('only ordinary iris geometry rebounds and cache sees both activation and reset', () => {
  const expression = { ...IDENTITY_DRIVER }
  const base = {
    side: 'L' as const,
    eye: { x0: 0, x1: 20, y0: 0, y1: 20, icx: 10, icy: 10, closeY: 10 },
    centerX: 10,
    centerY: 10,
    faceScale: 1,
    expression,
  }
  for (const kind of [
    'eye-open-iris',
    'eye-open-lid',
    'eye-close',
    'eyebrow',
  ] as const) {
    const normal = { x: 15, y: 15 }
    const rebound = { ...normal }
    deformAnime25DUpstreamFeaturePoint(normal, { ...base, kind })
    deformAnime25DUpstreamFeaturePoint(
      rebound,
      { ...base, kind },
      { x: 1.04, y: 0.98 },
    )
    if (kind === 'eye-open-iris') assert.notDeepEqual(rebound, normal)
    else assert.deepEqual(rebound, normal)
  }
  const state = createAnime25DDeformationChangeState()
  const capture = (x: number) =>
    captureAnime25DDeformationChanges(
      state,
      expression,
      {} as never,
      0,
      0,
      null,
      { x, y: 1 },
    )
  capture(1)
  assert.ok(capture(1.04) & ANIME25D_DEFORMATION_EYE)
  assert.ok(capture(1) & ANIME25D_DEFORMATION_EYE)
  assert.equal(capture(1) & ANIME25D_DEFORMATION_EYE, 0)
})
