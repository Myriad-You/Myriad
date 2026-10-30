import type { Anime25DPlaybackAnchors } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { FigureFloat } from './figureFloat'
import { createFigurePose, figureRest } from './figureSkeleton'

const rest = figureRest({
  face: { x0: 450, y0: 100, x1: 550, y1: 220, cx: 500, cy: 160 },
  neckPivot: { x: 500, y: 240 },
  neckTop: 220,
  neckBottom: 270,
  bodyPivot: { x: 500, y: 600 },
  groundY: 1400,
  mouth: { x0: 480, y0: 190, x1: 520, y1: 205, cx: 500, cy: 198 },
  faceScale: 1,
} as Anime25DPlaybackAnchors)!

function at(bone: { a: number; b: number; c: number; d: number; tx: number; ty: number }, x: number, y: number) {
  return { x: bone.a * x + bone.c * y + bone.tx, y: bone.b * x + bone.d * y + bone.ty }
}

test('floating still, every bone holds still', () => {
  const float = new FigureFloat(rest)
  float.step(1 / 60, 0, true)
  const pose = float.pose(0, createFigurePose())
  for (const bone of [pose.chest, pose.pelvis, pose.thighL, pose.shinR, pose.footL]) {
    assert.ok(Math.abs(bone.a - 1) < 1e-9 && Math.abs(bone.tx) < 1e-9 && Math.abs(bone.ty) < 1e-9)
  }
})

test('swaying, the chest leads, the hips trail and catch up, the legs swing and settle', () => {
  const float = new FigureFloat(rest)
  float.step(1 / 60, 0, true)
  const pose = createFigurePose()
  let hipsBehind = false
  for (let i = 0; i < 20; i++) {
    float.step(1 / 60, 1, true)
    float.pose(1, pose)
    const chest = at(pose.chest, 500, 300).x - 500
    const hips = at(pose.pelvis, 500, 600).x - 500
    if (hips < chest - 5) hipsBehind = true
  }
  assert.ok(hipsBehind, 'the hips trail the chest')
  const swinging = Math.abs(at(pose.footL, 465, 1390).x - at(pose.pelvis, 465, 620).x)
  for (let i = 0; i < 900; i++) float.step(1 / 60, 1, true)
  float.pose(1, pose)
  const chest = at(pose.chest, 500, 300).x - 500
  const hips = at(pose.pelvis, 500, 600).x - 500
  assert.ok(chest > 30 && Math.abs(hips - chest) < 6, `settled: chest ${chest}, hips ${hips}`)
  assert.ok(swinging > 0, 'the legs hang from the moving hips')
  // The leg the sway leaves behind folds back: its ankle drawn up, the other's not.
  const ankleL = at(pose.shinL, rest.legs.L.ankle.x, rest.legs.L.ankle.y)
  const ankleR = at(pose.shinR, rest.legs.R.ankle.x, rest.legs.R.ankle.y)
  const hipL = at(pose.pelvis, rest.legs.L.hip.x, rest.legs.L.hip.y)
  const hipR = at(pose.pelvis, rest.legs.R.hip.x, rest.legs.R.hip.y)
  assert.ok(ankleL.y - hipL.y < ankleR.y - hipR.y - 50, 'the left knee folds')
})

test('without physics it holds the pose at once', () => {
  const float = new FigureFloat(rest)
  float.step(1 / 60, 1, false)
  const pose = float.pose(1, createFigurePose())
  const chest = at(pose.chest, 500, 300).x - 500
  const hips = at(pose.pelvis, 500, 600).x - 500
  assert.ok(Math.abs(hips - chest) < 6, `hips under the chest at once: ${chest} ${hips}`)
})
