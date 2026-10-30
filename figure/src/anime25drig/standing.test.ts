import type { Anime25DPlaybackAnchors } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { Anime25DStanding, applySkirtTrail, bindStanding, standingWeightShift } from './standing'

const anchors = {
  bodyPivot: { x: 360, y: 500 },
  groundY: 1300,
} as Anime25DPlaybackAnchors

const leftLeg = { x: 250, y: 500, w: 100, h: 700, side: 'L', group: 'body' }

test('the weight goes to the leg the hips move over; the other relaxes', () => {
  const standing = Anime25DStanding.of(anchors)!
  // A lean the one way moves the hips the other, over that leg.
  assert.ok(standing.shift(1) < 0)
  const frame = standing.step(1 / 60, 1, false)
  assert.equal(frame.bendR, 1, 'hips over the left leg: the right relaxes')
  assert.equal(frame.bendL, 0)
  assert.ok(frame.pelvisTilt < 0, 'the left hip rises')
  const other = standing.step(1 / 60, -1, false)
  assert.equal(other.bendL, 1, 'hips over the right leg: the left relaxes')
  assert.equal(other.bendR, 0)
  assert.ok(other.pelvisTilt > 0, 'the right hip rises')
  assert.equal(Anime25DStanding.of({ ...anchors, groundY: undefined }), null, 'a bust never stands')
})

test("a skirt's trail swings it about the waist: the hem goes aside and tips up on that side", () => {
  const swung = { skirtSwing: 20, pelvisTilt: 0, bendL: 0, bendR: 0 }
  const skirt = bindStanding('bottomwear', { x: 200, y: 450, w: 320, h: 250, group: 'body' }, anchors)!
  const moved = (x: number, y: number) => {
    const point = { x, y }
    applySkirtTrail(point, skirt, x, y, swung)
    return { dx: point.x - x, dy: point.y - y }
  }
  assert.deepEqual(moved(300, 450), { dx: 0, dy: 0 })
  const right = moved(500, 700)
  const left = moved(220, 700)
  // An everyday trail (8% of the skirt) goes nearly as it is.
  assert.ok(Math.abs(right.dx - 20) < 0.6, `${right.dx}`)
  assert.ok(Math.abs(left.dx - right.dx) < 1e-9)
  assert.ok(right.dy < 0 && left.dy > 0, 'the hem tips, rising on the side it swings to')
  assert.ok(Math.abs(moved(360, 700).dy) < 1e-9, 'the middle of the hem only goes aside')
  // A flung one is held short of a third of its length.
  const flung = { ...swung, skirtSwing: 600 }
  const point = { x: 360, y: 700 }
  applySkirtTrail(point, skirt, 360, 700, flung)
  assert.ok(point.x - 360 > 70 && point.x - 360 <= 0.3 * 250, `${point.x - 360}`)
})

test('a part on a lower leg goes with that leg; hands and the skirt do not', () => {
  const buckle = bindStanding('objects', { x: 260, y: 1140, w: 60, h: 40, group: 'body' }, anchors)
  assert.equal(buckle?.kind, 'leg')
  assert.equal(buckle?.kind === 'leg' && buckle.side, 'L')
  assert.equal(bindStanding('handwear', { x: 200, y: 600, w: 60, h: 80, group: 'body' }, anchors), null)
  assert.equal(bindStanding('objects', { x: 200, y: 520, w: 60, h: 60, group: 'body' }, anchors), null)
})

test('standing idle settles on one leg, holds, then moves to the other', () => {
  // Day to day it holds a lean for seconds at a time, moving off it slowly.
  const samples = Array.from({ length: 300 }, (_, i) => standingWeightShift(i * 0.2))
  assert.ok(samples.some((value) => value > 0.4) && samples.some((value) => value < -0.4))
  assert.equal(standingWeightShift(0), 0, 'it starts standing straight')
  const held = [standingWeightShift(3), standingWeightShift(5)]
  assert.ok(held[0] !== held[1] && Math.abs(held[0] - held[1]) < 0.1, `${held}`)
  for (let i = 1; i < samples.length; i++) {
    assert.ok(Math.abs(samples[i] - samples[i - 1]) <= 0.35, 'moves smoothly')
  }
})

test('a leg found on the portrait has its own hip, knee and ankle', () => {
  const skeleton = {
    model: 'dwpose',
    joints: {
      hipL: { x: 300, y: 600, score: 0.9 },
      kneeL: { x: 300, y: 900, score: 0.9 },
      ankleL: { x: 300, y: 1200, score: 0.9 },
    },
  }
  const found = bindStanding('legwear', leftLeg, { ...anchors, skeleton })!
  assert.ok(found.kind === 'leg')
  if (found.kind !== 'leg') return
  assert.equal(found.kneeAt, 0.5)
  assert.equal(found.ankleAt, 0.875)
  assert.equal(found.hipAt, 0.125)
  // A misread knee keeps the usual proportions.
  const odd = bindStanding('legwear', leftLeg, {
    ...anchors,
    skeleton: { model: 'dwpose', joints: { ...skeleton.joints, kneeL: { x: 300, y: 1250, score: 0.9 } } },
  })!
  assert.ok(odd.kind === 'leg' && odd.kneeAt === 0.45)
})
