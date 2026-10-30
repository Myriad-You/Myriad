import type { PoseCorrection } from './poseCorrections'
import type { Anime25DPlayback } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { WORKBENCH_DRIVER } from './driver'
import {
  appendPoseCorrectionPatch,
  clampPoseCorrectionPatch,
  countPoseCorrectionChanges,
  nearestPoseCorrectionRegion,
  newPoseCorrectionPatch,
  poseCorrectionCorner,
  poseCorrectionPreviewDriver,
  poseCorrectionRegionPoint,
  poseCorrectionTransitionDriver,
} from './poseAuthoring'

const playback = {
  anchors: { face: { y1: 450 }, eyeL: { icx: 210, closeY: 260 }, mouth: { cx: 300, cy: 380 } },
  layers: [{ role: 'face' }, { role: 'front-hair' }],
  shellProfile: { head: { centerX: 300, centerY: 250, radiusX: 180, radiusY: 240 } },
} as Anime25DPlayback
const driver = { ...WORKBENCH_DRIVER, angleX: 0.8, angleY: -0.6, eyeOpenL: 0 }

function capturePoseCorrection(p: Anime25DPlayback, d: typeof driver, region: Parameters<typeof poseCorrectionRegionPoint>[1]): PoseCorrection | null {
  const anchor = poseCorrectionRegionPoint(p, region)
  if (!anchor) return null
  const corner = poseCorrectionCorner(p, d, anchor.surface, anchor)
  return 'problem' in corner ? null : { surface: anchor.surface, at: corner.at, patches: [newPoseCorrectionPatch(p, anchor.surface, anchor)] }
}

test('capture uses actual asset anchors and pose conditions, not avatar names', () => {
  const c = capturePoseCorrection(playback, driver, 'leftEye')!
  assert.deepEqual(c.at, { angleX: 0.8, angleY: -0.6, eyeCloseL: 1 })
  assert.equal(c.surface, 'head')
  assert.equal(c.patches[0].x, -0.5)
  assert.equal(c.patches[0].y, Math.round(10 / 240 * 1000) / 1000)
  assert.equal(c.patches[0].dx, 0)
  assert.equal(capturePoseCorrection(playback, WORKBENCH_DRIVER, 'leftEye'), null)
  assert.equal(capturePoseCorrection(playback, driver, 'rightEye'), null)
  assert.equal(capturePoseCorrection(playback, driver, 'rearCrown'), null)
  assert.equal(capturePoseCorrection(playback, driver, 'frontCrown')?.surface, 'front-hair')
})

test('adding patches reuses the existing corner without overwriting its calibration or source arrays', () => {
  const c = capturePoseCorrection(playback, driver, 'leftEye')!
  c.patches[0].dx = 0.1
  const current = [c]
  const candidate = { ...c, at: { ...c.at, angleX: 1 }, patches: [{ ...c.patches[0], dx: 0 }] }
  const result = appendPoseCorrectionPatch(current, candidate)!
  assert.equal(result.index, 0)
  assert.equal(result.patch, 1)
  assert.equal(result.corrections.length, 1)
  assert.equal(result.corrections[0].at.angleX, 0.8)
  assert.equal(result.corrections[0].patches[0].dx, 0.1)
  assert.equal(current[0].patches.length, 1)
  result.corrections[0].patches[0].dx = 0.2
  assert.equal(current[0].patches[0].dx, 0.1)
  assert.equal(appendPoseCorrectionPatch([{ ...c, patches: Array.from({ length: 8 }).fill(c.patches[0]) }], candidate), null)
})

test('selecting a correction freezes distracting motion and preserves unrelated expression controls', () => {
  const c = capturePoseCorrection(playback, driver, 'leftEye')!
  const selected = poseCorrectionPreviewDriver(c, { ...WORKBENCH_DRIVER, mouthForm: 0.7, eyeOpenR: 0 })
  assert.equal(selected.angleX, 0.8)
  assert.equal(selected.angleY, -0.6)
  assert.equal(selected.eyeOpenL, 0)
  assert.equal(selected.eyeOpenR, 0)
  assert.equal(selected.mouthForm, 0.7)
  for (const key of ['idle', 'rand', 'blink', 'talk', 'mouse', 'phys'] as const) assert.equal(selected[key], false)
})

test('a pose needs a head turn, and a second condition the patch actually sits on', () => {
  const eye = poseCorrectionRegionPoint(playback, 'leftEye')!
  assert.deepEqual(poseCorrectionCorner(playback, { ...WORKBENCH_DRIVER, eyeOpenL: 0 }, 'head', eye), { problem: 'needsTurn' })
  assert.deepEqual(poseCorrectionCorner(playback, { ...WORKBENCH_DRIVER, angleX: 0.5 }, 'head', eye), { problem: 'needsSecond' })
  // A closed eye counts for a patch on that eye, never for one on the hair.
  assert.deepEqual(poseCorrectionCorner(playback, { ...WORKBENCH_DRIVER, angleX: 0.5, eyeOpenL: 0.2 }, 'head', eye), { at: { angleX: 0.5, eyeCloseL: 0.8 } })
  assert.deepEqual(poseCorrectionCorner(playback, { ...WORKBENCH_DRIVER, angleX: 0.5, eyeOpenL: 0.2 }, 'front-hair', eye), { problem: 'needsSecond' })
  assert.deepEqual(poseCorrectionCorner(playback, { ...WORKBENCH_DRIVER, angleX: 0.5, angleY: 0.3 }, 'front-hair', { x: 0, y: -0.9 }), { at: { angleX: 0.5, angleY: 0.3 } })
})

test('a patch is named and sized by the feature it sits on', () => {
  const eye = poseCorrectionRegionPoint(playback, 'leftEye')!
  assert.equal(nearestPoseCorrectionRegion(playback, 'head', eye), 'leftEye')
  assert.equal(nearestPoseCorrectionRegion(playback, 'head', { x: 1.5, y: 1.5 }), null)
  assert.equal(nearestPoseCorrectionRegion(playback, 'back-hair', eye), null)
  const onEye = newPoseCorrectionPatch(playback, 'head', eye)
  const elsewhere = newPoseCorrectionPatch(playback, 'head', { x: 1.5, y: 1.5 })
  assert.ok(onEye.radiusX < elsewhere.radiusX + 0.01)
  assert.deepEqual(clampPoseCorrectionPatch({ x: 3, y: -3, radiusX: 0, radiusY: 5, dx: 1, dy: -1 }), { x: 2, y: -2, radiusX: 0.1, radiusY: 2, dx: 0.25, dy: -0.25 })
})

test('the transition check eases from facing front to the correction pose', () => {
  const c = capturePoseCorrection(playback, driver, 'leftEye')!
  const half = poseCorrectionTransitionDriver(c, WORKBENCH_DRIVER, 0.5)
  assert.equal(half.angleX, 0.4)
  assert.equal(half.angleY, -0.3)
  assert.equal(half.eyeOpenL, 0.5)
  const front = poseCorrectionTransitionDriver(c, WORKBENCH_DRIVER, 0)
  assert.equal(front.angleX, 0)
  assert.equal(front.eyeOpenL, 1)
})

test('unsaved changes count each spot added, removed or moved once', () => {
  const c = capturePoseCorrection(playback, driver, 'leftEye')!
  const saved = [c]
  assert.equal(countPoseCorrectionChanges(saved, structuredClone(saved)), 0)
  const moved = structuredClone(saved)
  moved[0].patches[0].dx = 0.1
  assert.equal(countPoseCorrectionChanges(saved, moved), 1)
  const added = appendPoseCorrectionPatch(saved, c)!.corrections
  assert.equal(countPoseCorrectionChanges(saved, added), 1)
  assert.equal(countPoseCorrectionChanges(saved, []), 1)
})
