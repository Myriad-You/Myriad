import type { Anime25DPlaybackAnchors } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { Anime25DStanding, applyStanding, bindStanding, standingWeightShift } from './standing'

const anchors = {
  bodyPivot: { x: 360, y: 500 },
  groundY: 1300,
} as Anime25DPlaybackAnchors

function at(
  layer: Parameters<typeof bindStanding>[1],
  role: string,
  y: number,
  frame: { skirtSwing: number; pelvisTilt: number; bendL: number; bendR: number },
) {
  const binding = bindStanding(role, layer, anchors)!
  const x = layer.x + layer.w / 2
  const point = { x, y }
  applyStanding(point, binding, x, y, frame)
  return { dx: point.x - x, dy: point.y - y }
}

const leftLeg = { x: 250, y: 500, w: 100, h: 700, side: 'L', group: 'body' }
const rightLeg = { x: 370, y: 500, w: 100, h: 700, side: 'R', group: 'body' }

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

test('a relaxed leg bends in at the knee and lifts its heel, toes kept down', () => {
  const relaxed = { skirtSwing: 0, pelvisTilt: 0, bendL: 1, bendR: 0 }
  const hip = at(leftLeg, 'legwear', 500, relaxed)
  const knee = at(leftLeg, 'legwear', 500 + 0.45 * 800, relaxed)
  const ankle = at(leftLeg, 'legwear', 500 + 0.85 * 800, relaxed)
  const toe = at(leftLeg, 'footwear', 1300, relaxed)
  assert.equal(hip.dx, 0)
  assert.ok(knee.dx > 30, 'the knee goes in towards the other leg')
  assert.ok(ankle.dy < -15, 'the heel lifts')
  assert.ok(toe.dy > ankle.dy && toe.dy < 0, 'the toes stay nearly down')
  assert.ok(toe.dx < knee.dx, 'the foot follows the knee only a little')
  const straight = at(rightLeg, 'legwear', 500 + 0.45 * 800, relaxed)
  assert.equal(straight.dx, 0, 'the leg with the weight stays straight')
})

test('the pelvis tilts the skirt hem and the tops of the legs, never the soles', () => {
  const tilted = { skirtSwing: 0, pelvisTilt: 0.05, bendL: 0, bendR: 0 }
  const skirt = { x: 200, y: 450, w: 320, h: 250, group: 'body' }
  const hemLeft = bindStanding('bottomwear', skirt, anchors)!
  const point = { x: 220, y: 700 }
  applyStanding(point, hemLeft, 220, 700, tilted)
  assert.ok(point.y > 700, 'the hem drops on the left as the right hip rises')
  assert.ok(at(rightLeg, 'legwear', 500, tilted).dy < 0)
  assert.equal(at(rightLeg, 'footwear', 1300, tilted).dy, 0)
})

test('a part on a lower leg goes with that leg; hands and the skirt do not', () => {
  const buckle = bindStanding('objects', { x: 260, y: 1140, w: 60, h: 40, group: 'body' }, anchors)
  assert.equal(buckle?.kind, 'leg')
  assert.equal(buckle?.kind === 'leg' && buckle.side, 'L')
  assert.equal(bindStanding('handwear', { x: 200, y: 600, w: 60, h: 80, group: 'body' }, anchors), null)
  assert.equal(bindStanding('objects', { x: 200, y: 520, w: 60, h: 60, group: 'body' }, anchors), null)
})

test('standing idle settles on one leg, holds, then moves to the other', () => {
  const samples = Array.from({ length: 80 }, (_, i) => standingWeightShift(i * 0.5))
  assert.ok(samples.some((value) => value > 0.5) && samples.some((value) => value < -0.5))
  // Held for seconds at a time, not swaying.
  assert.equal(standingWeightShift(4), standingWeightShift(6))
  for (let i = 1; i < samples.length; i++) {
    assert.ok(Math.abs(samples[i] - samples[i - 1]) <= 0.6, 'moves smoothly')
  }
})
