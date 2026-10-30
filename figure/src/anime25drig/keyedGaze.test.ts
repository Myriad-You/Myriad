import assert from 'node:assert/strict'
import test from 'node:test'
import { bindKeyedGaze, GAZE_REACH_X, GAZE_REACH_Y, keyedGazeShift } from './keyedGaze'

// A lattice over 0..200 whose backward offset at turned point (x, y) is back(x, y).
function lattice(back: (x: number, y: number) => [number, number]) {
  const grid = 21
  const values: number[] = []
  for (let j = 0; j < grid; j++) {
    for (let i = 0; i < grid; i++) values.push(...back((i / (grid - 1)) * 200, (j / (grid - 1)) * 200))
  }
  return { box: [0, 0, 200, 200] as [number, number, number, number], grid, back: values }
}

const still = lattice(() => [0, 0])
// The white 40..160 × 80..120, its iris centred.
const eye = { x0: 40, y0: 80, x1: 160, y1: 120, icx: 100, icy: 100, closeY: 105 }
const iris = { group: 'head', role: 'irides', side: 'L' } as const

test('at rest the gaze moves the iris as before', () => {
  const gaze = bindKeyedGaze({ 'eye:L': { plus: still, minus: still } }, iris, eye)!
  keyedGazeShift(gaze, 0, 0, 1, -1, 1)
  assert.deepEqual(gaze.shift, { x: GAZE_REACH_X, y: -GAZE_REACH_Y })
})

test('a key that already looks to a corner leaves the gaze no further room that way', () => {
  // Turned toward +x, the white stays; the iris moved 11 px toward +x (looking at the viewer).
  const looked = lattice(() => [-11, 0])
  const gaze = bindKeyedGaze({ 'eye:L': { plus: still, minus: still }, 'iris:L': { plus: looked, minus: looked } }, iris, eye)!
  keyedGazeShift(gaze, 1, 0, 1, 0, 1)
  assert.ok(Math.abs(gaze.shift.x) < 1e-3, `${gaze.shift.x}`)
  keyedGazeShift(gaze, 1, 0, -1, 0, 1)
  assert.ok(Math.abs(gaze.shift.x + GAZE_REACH_X) < 1e-3, `${gaze.shift.x}`)
  // Half way through the turn the key has used half the room.
  keyedGazeShift(gaze, 0.5, 0, 1, 0, 1)
  assert.ok(Math.abs(gaze.shift.x - GAZE_REACH_X / 2) < 1e-3, `${gaze.shift.x}`)
})

test('a foreshortened white moves its iris as much less as it is narrower', () => {
  // Turned toward +x, everything squeezed to 70% about x = 100: back(t) = 100 + (t − 100) / 0.7 − t.
  const squeezed = lattice((x) => [100 + (x - 100) / 0.7 - x, 0])
  const gaze = bindKeyedGaze({ 'eye:L': { plus: squeezed, minus: squeezed } }, iris, eye)!
  keyedGazeShift(gaze, 1, 0, 1, 0, 1)
  assert.ok(Math.abs(gaze.shift.x - GAZE_REACH_X * 0.7) < 0.05, `${gaze.shift.x}`)
})

test('without a key for the eye there is nothing to keep', () => {
  assert.equal(bindKeyedGaze({ face: { plus: still, minus: still } }, iris, eye), null)
  assert.equal(bindKeyedGaze(undefined, iris, eye), null)
})
