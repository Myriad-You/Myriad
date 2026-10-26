import assert from 'node:assert/strict'
import test from 'node:test'
import { addBodyTurn, bindBodyTurnKeyform, BODY_KEY_YAW, BODY_TURN_SHARE, bodyKeyShare, bodyTurnAmount, bodyTurnKeyFamily, headRidesBody } from './bodyTurnKeyforms'
import { HEAD_TURN_RADIANS } from './headTurn'
import { HEAD_YAW_SHARE } from './torsoDeformation'

// A lattice over 0..200 moving everything by (dx, dy) in the turned drawing.
function shifted(dx: number, dy: number) {
  const grid = 5
  const back: number[] = []
  for (let i = 0; i < grid * grid; i++) back.push(-dx, -dy)
  return { box: [0, 0, 200, 200] as [number, number, number, number], grid, back }
}

const yawed = (yaw: number) => ({ active: yaw !== 0, yawCosine: Math.cos(yaw), yawSine: Math.sin(yaw) })

test('body parts turn by their own key; the head and loose objects do not', () => {
  assert.equal(bodyTurnKeyFamily({ group: 'body', role: 'handwear', side: 'L' }), 'arm:L')
  assert.equal(bodyTurnKeyFamily({ group: 'body', role: 'collar-front', side: null }), 'topwear')
  assert.equal(bodyTurnKeyFamily({ group: 'body', role: 'legwear', side: 'R' }), 'legwear:R')
  assert.equal(bodyTurnKeyFamily({ group: 'head', role: 'face', side: null }), null)
  assert.equal(bodyTurnKeyFamily({ group: 'body', role: 'objects', side: null }), null)
})

test('the body shows a share of the torso yaw as its keyed turn', () => {
  const full = BODY_KEY_YAW / BODY_TURN_SHARE
  assert.equal(bodyTurnAmount(yawed(0), 1), 0)
  assert.ok(Math.abs(bodyTurnAmount(yawed(full / 2), 1) - 0.5) < 1e-9)
  assert.equal(bodyTurnAmount(yawed(-full * 2), 1), -1)
  assert.ok(Math.abs(bodyTurnAmount(yawed(full), 0.5) - 0.5) < 1e-9)
})

test('a leg turns by its own key, or by both legs\' where it has none', () => {
  const keys = { 'legwear:L': { plus: shifted(4, 0), minus: shifted(4, 0) }, 'legwear': { plus: shifted(9, 0), minus: shifted(9, 0) } }
  const rest = Float32Array.of(100, 100)
  assert.ok(Math.abs(bindBodyTurnKeyform(keys, { group: 'body', role: 'legwear', side: 'L' }, rest)!.plus[0] - 4) < 1e-3)
  assert.ok(Math.abs(bindBodyTurnKeyform(keys, { group: 'body', role: 'legwear', side: 'R' }, rest)!.plus[0] - 9) < 1e-3)
})

test('a vertex moves by its key mixed by the amount', () => {
  const keys = { topwear: { plus: shifted(10, 0), minus: shifted(-6, 2) } }
  const bound = bindBodyTurnKeyform(keys, { group: 'body', role: 'topwear', side: null }, Float32Array.of(100, 100))!
  const point = { x: 100, y: 100 }
  addBodyTurn(point, bound, 0, 0.5)
  assert.ok(Math.abs(point.x - 105) < 1e-3 && Math.abs(point.y - 100) < 1e-3, JSON.stringify(point))
  addBodyTurn(point, bound, 0, -1)
  assert.ok(Math.abs(point.x - 99) < 1e-3 && Math.abs(point.y - 102) < 1e-3, JSON.stringify(point))
})

test("a sleeve's shoulder and inner side go with the torso, its far side with its own key", () => {
  const keys = { 'topwear': { plus: shifted(10, 0), minus: shifted(10, 0) }, 'arm:L': { plus: shifted(-20, 0), minus: shifted(-20, 0) } }
  // A left arm spanning x 0..100, y 0..100: its inner side is its right.
  const rest = Float32Array.of(0, 0, 100, 50, 0, 100, 50, 50)
  const bound = bindBodyTurnKeyform(keys, { group: 'body', role: 'handwear', side: 'L' }, rest)!
  const offset = (vertex: number) => bound.plus[vertex * 2]
  assert.ok(Math.abs(offset(0) - 10) < 1e-3, 'shoulder top')
  assert.ok(Math.abs(offset(1) - 10) < 1e-3, 'inner side')
  assert.ok(Math.abs(offset(2) + 20) < 1e-3, 'outer side, low')
})

test('a directed torso turn carries a keyed head as far round as the keyed body shows', () => {
  // torsoTurn 1 turns the torso HEAD_YAW_SHARE; a standing figure shows all of it.
  const figure = { bodyTurnKeyforms: { 'thigh:L': { plus: shifted(1, 0), minus: shifted(1, 0) } }, anchors: { groundY: 900 } }
  assert.equal(bodyKeyShare(figure), 1)
  assert.ok(Math.abs(headRidesBody(bodyKeyShare(figure), true) * HEAD_TURN_RADIANS - HEAD_YAW_SHARE) < 1e-9)
  assert.equal(bodyKeyShare({ ...figure, anchors: {} }), BODY_TURN_SHARE)
  assert.equal(bodyKeyShare({ anchors: { groundY: 900 } }), null)
  assert.equal(headRidesBody(1, false), 0)
  assert.equal(headRidesBody(null, true), 0)
})

test('a leg measured as thigh and shin takes each above and below its knee', () => {
  const keys = { 'thigh:L': { plus: shifted(10, 0), minus: shifted(10, 0) }, 'shin:L': { plus: shifted(-4, 0), minus: shifted(-4, 0) } }
  // A leg from y 0 to 200, its knee at 80.
  const rest = Float32Array.of(50, 0, 50, 79, 50, 80, 50, 81, 50, 200)
  const bound = bindBodyTurnKeyform(keys, { group: 'body', role: 'legwear', side: 'L' }, rest, 80)!
  const x = (vertex: number) => bound.plus[vertex * 2]
  assert.ok(Math.abs(x(0) - 10) < 1e-3, 'hip')
  assert.ok(Math.abs(x(2) - 3) < 1e-3, 'knee, half way')
  assert.ok(x(1) > x(2) && x(2) > x(3), 'blended across the knee')
  assert.ok(Math.abs(x(4) + 4) < 1e-3, 'ankle')
  // The other leg has no keys of its own.
  assert.equal(bindBodyTurnKeyform(keys, { group: 'body', role: 'legwear', side: 'R' }, rest, 80), null)
})

test('a leg\'s top takes the torso\'s cylinder in place of its key, giving way down the thigh', () => {
  const keys = { 'thigh:L': { plus: shifted(10, 0), minus: shifted(10, 0) }, 'shin:L': { plus: shifted(-4, 0), minus: shifted(-4, 0) } }
  // A leg from y 0 to 200: the cylinder gives way over its top 50.
  const rest = Float32Array.of(50, 0, 50, 25, 50, 50, 50, 200)
  const cylinder = bindBodyTurnKeyform(keys, { group: 'body', role: 'legwear', side: 'L' }, rest, 80)!.cylinder!
  assert.equal(cylinder[0], 1, 'the hip, all the cylinder')
  assert.ok(Math.abs(cylinder[1] - 0.5) < 1e-6, 'half way')
  assert.equal(cylinder[2], 0)
  assert.equal(cylinder[3], 0, 'the ankle, all its key')
  // A shoe is all its key.
  const shoe = bindBodyTurnKeyform({ footwear: { plus: shifted(1, 0), minus: shifted(1, 0) } }, { group: 'body', role: 'footwear', side: null }, rest)!
  assert.equal(shoe.cylinder, undefined)
})
