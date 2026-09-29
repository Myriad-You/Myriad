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

/** A blink: closed in 0.08 s, held, open again by 0.58 s. */
function blink(t: number) {
  return t < 0.08 ? 1 - t / 0.08 : t < 0.42 ? 0 : t < 0.58 ? (t - 0.42) / 0.16 : 1
}

test('a still eye is a still iris', () => {
  const iris = new Anime25DIrisRebound()
  for (let i = 0; i < 120; i++) {
    iris.step(1 / 60, 1, 0, 0.2, -0.1, false)
    assert.deepEqual([iris.x, iris.y], [1, 1])
  }
})

test('a reopening lid sets the iris wobbling, gently, keeping its area, and it settles still', () => {
  for (const fps of [30, 60, 144]) {
    const iris = new Anime25DIrisRebound()
    let afterOpening = 0
    for (let i = 0; i <= fps * 2; i++) {
      const t = i / fps
      iris.step(1 / fps, blink(t), 0, 0, 0, false)
      assert.ok(Math.abs(iris.x * iris.y - 1) < 1e-9)
      assert.ok(Math.abs(iris.x - 1) < 0.11 && Math.abs(iris.y - 1) < 0.11)
      if (t > 0.58 && t < 1) afterOpening = Math.max(afterOpening, Math.abs(iris.y - 1))
    }
    assert.ok(afterOpening > 0.01, `${fps} fps wobble ${afterOpening}`)
    assert.deepEqual([iris.x, iris.y], [1, 1])
  }
})

test('a darting gaze wobbles the iris too, and effects that redraw the eye hold it still', () => {
  const iris = new Anime25DIrisRebound()
  let most = 0
  for (let i = 0; i < 60; i++) {
    const s = Math.min(1, i / 6)
    iris.step(1 / 60, 1, 0, 0.6 * s * s * (3 - 2 * s), 0, false)
    most = Math.max(most, Math.abs(iris.x - 1))
  }
  assert.ok(most > 0.005, `gaze wobble ${most}`)
  const held = new Anime25DIrisRebound()
  for (let i = 0; i < 60; i++) {
    held.step(1 / 60, blink(i / 60), 0, Math.sin(i), 0, true)
    assert.deepEqual([held.x, held.y], [1, 1])
  }
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
