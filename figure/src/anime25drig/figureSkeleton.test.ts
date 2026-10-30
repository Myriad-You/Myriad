import type { Anime25DPlaybackAnchors } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  bindFigureSkin,
  createFigurePose,
  figureAffineAt,
  figureRest,
  skinFigurePoint,
  solveFigurePose,
} from './figureSkeleton'

const anchors = {
  face: { x0: 450, y0: 100, x1: 550, y1: 220, cx: 500, cy: 160 },
  neckPivot: { x: 500, y: 240 },
  neckTop: 220,
  neckBottom: 270,
  bodyPivot: { x: 500, y: 600 },
  groundY: 1400,
  mouth: { x0: 480, y0: 190, x1: 520, y1: 205, cx: 500, cy: 198 },
  faceScale: 1,
  skeleton: {
    model: 'test',
    joints: {
      hipL: { x: 460, y: 620, score: 0.9 },
      hipR: { x: 540, y: 620, score: 0.9 },
      kneeL: { x: 462, y: 960, score: 0.9 },
      kneeR: { x: 538, y: 960, score: 0.9 },
      ankleL: { x: 465, y: 1300, score: 0.9 },
      ankleR: { x: 535, y: 1300, score: 0.9 },
    },
  },
} as Anime25DPlaybackAnchors

const rest = figureRest(anchors)!

function skinned(kind: Parameters<typeof bindFigureSkin>[0], layer: Parameters<typeof bindFigureSkin>[1], x: number, y: number, pose: ReturnType<typeof createFigurePose>) {
  const skin = bindFigureSkin(kind, layer, rest)!
  const point = { x, y }
  skinFigurePoint(point, y, skin, rest, pose)
  return point
}

const legL = { x: 430, y: 600, w: 70, h: 800, side: 'L', group: 'body' }
const top = { x: 380, y: 270, w: 240, h: 400, side: null, group: 'body' }

test('a bust has no skeleton; a standing figure has its joints as found', () => {
  assert.equal(figureRest({ ...anchors, groundY: undefined }), null)
  assert.deepEqual(rest.legs.L.knee, { x: 462, y: 960 })
  assert.deepEqual(rest.legs.R.ankle, { x: 535, y: 1300 })
  assert.equal(rest.legs.L.toe.y, 1400)
})

test('at rest every bone holds still', () => {
  const pose = solveFigurePose(rest, { body: 0, pelvisTilt: 0, bendL: 0, bendR: 0 }, createFigurePose())
  for (const [x, y, kind, layer] of [
    [470, 900, 'legwear', legL],
    [468, 1350, 'legwear', legL],
    [500, 400, 'topwear', top],
    [500, 640, 'topwear', top],
  ] as const) {
    const point = skinned(kind, layer, x, y, pose)
    assert.ok(Math.abs(point.x - x) < 1e-9 && Math.abs(point.y - y) < 1e-9, `${kind} ${x},${y} moved to ${point.x},${point.y}`)
  }
})

test('swaying, the soles stay planted and the figure moves more the higher up it is', () => {
  const pose = solveFigurePose(rest, { body: 1, pelvisTilt: 0.045, bendL: 1, bendR: 0 }, createFigurePose())
  // The leg bearing the weight stands flat; the free one is on its toes (below).
  const sole = skinned('legwear', { ...legL, x: 500, side: 'R' }, 534, 1390, pose)
  assert.ok(Math.abs(sole.x - 534) < 0.5 && Math.abs(sole.y - 1390) < 0.5, `sole moved to ${sole.x},${sole.y}`)
  const travel = (x: number, y: number, kind: 'legwear' | 'topwear', layer: typeof legL) => Math.abs(skinned(kind, layer, x, y, pose).x - x)
  const shin = travel(464, 1150, 'legwear', legL)
  const thigh = travel(462, 780, 'legwear', legL)
  const chest = travel(500, 330, 'topwear', top)
  assert.ok(shin < thigh, `shin ${shin} < thigh ${thigh}`)
  assert.ok(thigh > 10, `the hips carry the thigh ${thigh}`)
  assert.ok(chest > 10, `the shoulders lean ${chest}`)
  // The figure sways from its feet: the hips go the way of the sway and the shoulders further.
  const hips = skinned('topwear', top, 500, 640, pose).x - 500
  const shoulders = skinned('topwear', top, 500, 300, pose).x - 500
  assert.ok(hips > 10 && shoulders > hips, `hips ${hips}, shoulders ${shoulders}`)
})

test('the free leg bends its knee inwards and rises onto its toes', () => {
  const pose = solveFigurePose(rest, { body: 0, pelvisTilt: 0, bendL: 1, bendR: 0 }, createFigurePose())
  const knee = skinned('legwear', legL, 462, 960, pose)
  assert.ok(knee.x > 462 + 5, `the knee goes inwards: ${knee.x}`)
  const ankle = skinned('legwear', legL, 465, 1300, pose)
  assert.ok(ankle.y < 1300 - 5, `the heel lifts: ${ankle.y}`)
  const toe = skinned('legwear', legL, 465, 1399, pose)
  assert.ok(Math.abs(toe.y - 1399) < 3, `the toes stay down: ${toe.y}`)
})

test('a free foot on its toes is drawn at most a little taller, its knee bending for the whole lift', () => {
  const pose = solveFigurePose(rest, { body: 0, pelvisTilt: 0, bendL: 1, bendR: 0 }, createFigurePose())
  const ankle = skinned('legwear', legL, 465, 1300, pose)
  const toe = skinned('legwear', legL, 465, 1399, pose)
  // Drawn 99 tall from the ankle to the toes.
  assert.ok(toe.y - ankle.y <= 99 * 1.2 + 1, `the shoe is drawn ${toe.y - ankle.y} tall`)
  const knee = skinned('legwear', legL, 462, 960, pose)
  assert.ok(knee.x > 462 + 20, `the knee still comes well in: ${knee.x}`)
})

test('a free leg foreshortens its thigh and shin alike, its knee clear of the other leg', () => {
  const pose = solveFigurePose(rest, { body: 1, pelvisTilt: 0.045, bendL: 1, bendR: 0 }, createFigurePose())
  const hip = skinned('legwear', legL, 460, 621, pose)
  const knee = skinned('legwear', legL, 462, 960, pose)
  const ankle = skinned('legwear', legL, 465, 1300, pose)
  const thigh = Math.hypot(knee.x - hip.x, knee.y - hip.y) / Math.hypot(2, 339)
  const shin = Math.hypot(ankle.x - knee.x, ankle.y - knee.y) / Math.hypot(3, 340)
  assert.ok(Math.abs(thigh - shin) < 0.02, `thigh ${thigh}, shin ${shin}`)
  assert.ok(thigh < 1.06 && shin < 1.06, `thigh ${thigh}, shin ${shin}`)
  const other = skinned('legwear', { ...legL, x: 500, side: 'R' }, 538, 960, pose)
  assert.ok(knee.x > 462, `the free knee comes in: ${knee.x}`)
  assert.ok(knee.x < other.x - 20, `but not over the other knee: ${knee.x} against ${other.x}`)
})

test('the skin is continuous across the knee and the waist', () => {
  const pose = solveFigurePose(rest, { body: -0.8, pelvisTilt: 0.04, bendL: 0.7, bendR: 0 }, createFigurePose())
  for (const [kind, layer, x, from, to] of [
    ['legwear', legL, 462, 800, 1200],
    ['topwear', top, 500, 300, 700],
  ] as const) {
    let last = skinned(kind, layer, x, from, pose)
    for (let y = from + 2; y <= to; y += 2) {
      const point = skinned(kind, layer, x, y, pose)
      assert.ok(Math.hypot(point.x - last.x, point.y - last.y) < 4, `${kind} jumps at ${y}`)
      last = point
    }
  }
})

test('a rigid layer rides the affine blend of its bones', () => {
  const pose = solveFigurePose(rest, { body: 0.6, pelvisTilt: 0, bendL: 0, bendR: 0 }, createFigurePose())
  const m = figureAffineAt(250, { kind: 'chest' }, rest, pose, new Float32Array(9))
  const point = { x: 500, y: 250 }
  skinFigurePoint(point, 250, { kind: 'chest' }, rest, pose)
  assert.ok(Math.abs(m[0] * 500 + m[3] * 250 + m[6] - point.x) < 1e-4)
  assert.ok(Math.abs(m[1] * 500 + m[4] * 250 + m[7] - point.y) < 1e-4)
})

test('layers ride the bones they hang from', () => {
  assert.deepEqual(bindFigureSkin('bottomwear', top, rest), { kind: 'pelvis' })
  assert.deepEqual(bindFigureSkin('handwear', top, rest), { kind: 'chest' })
  assert.deepEqual(bindFigureSkin('face', { ...top, group: 'head' }, rest), { kind: 'chest' })
  assert.deepEqual(bindFigureSkin('legwear', { ...legL, side: null }, rest), { kind: 'leg', side: 'L' })
  assert.deepEqual(bindFigureSkin('objects', { x: 520, y: 1100, w: 30, h: 30, group: 'body' }, rest), { kind: 'leg', side: 'R' })
  // A coat's lining tucked behind a thigh hangs from the shoulders with the coat.
  assert.deepEqual(bindFigureSkin('topwear', { x: 430, y: 900, w: 70, h: 400, group: 'body' }, rest), { kind: 'torso' })
  assert.equal(bindFigureSkin('topwear', top, null), null)
})

test('the collar clip a neck is drawn through goes where the bones carry the neck', async () => {
  const { carryCollarClipOnFigure } = await import('./collarRuntime')
  const neck = { ...top, role: 'neck', y: 220, h: 80, x: 470, w: 60 }
  const figureSkin = bindFigureSkin('neck', neck, rest)
  assert.ok(figureSkin, 'the neck rides the bones')
  const binding = { standing: null, figureSkin, partSways: null, shaderGlobalTransform: false }
  const rest2 = Float32Array.from([480, 230, 520, 230, 480, 290, 520, 290])
  const clip = { rest: rest2, deformed: rest2.slice() } as Parameters<typeof carryCollarClipOnFigure>[0]
  const pose = solveFigurePose(rest, { body: 1, pelvisTilt: 0.045, bendL: 1, bendR: 0 }, createFigurePose())
  carryCollarClipOnFigure(clip, binding, { standing: null, figure: { rest, pose } as never })
  for (let i = 0; i < 8; i += 2) {
    const neckPoint = skinned('neck', neck, rest2[i], rest2[i + 1], pose)
    assert.ok(Math.abs(clip.deformed[i] - neckPoint.x) < 1e-3 && Math.abs(clip.deformed[i + 1] - neckPoint.y) < 1e-3,
      `${clip.deformed[i]},${clip.deformed[i + 1]} vs ${neckPoint.x},${neckPoint.y}`)
    assert.ok(Math.abs(clip.deformed[i] - rest2[i]) > 5, 'the sway moved it')
  }
  // A bust has no figure: the clip stays where the collar motion put it.
  const still = { rest: rest2, deformed: rest2.slice() } as Parameters<typeof carryCollarClipOnFigure>[0]
  carryCollarClipOnFigure(still, binding, { standing: null, figure: null })
  assert.deepEqual(Array.from(still.deformed), Array.from(rest2))
})
