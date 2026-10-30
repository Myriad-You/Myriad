import assert from 'node:assert/strict'
import test from 'node:test'
import { GazeShiftBlink } from './gazeBlink'

const DT = 1 / 60

/** Plays a look from 0 to `to` over `seconds` (minimum-jerk), then holds; returns the frames that blink. */
function look(blink: GazeShiftBlink, to: number, seconds: number, random = () => 0, blinking = () => false) {
  const fired: number[] = []
  for (let frame = 0; frame < 120; frame++) {
    const t = Math.min(1, (frame * DT) / seconds)
    const x = to * t * t * t * (t * (t * 6 - 15) + 10)
    if (blink.step(x, 0, 0, 0, DT, blinking(), random)) fired.push(frame)
  }
  return fired
}

test('a long look blinks once, early in the movement; a glance does not', () => {
  const long = look(new GazeShiftBlink(), 0.7, 0.5)
  assert.equal(long.length, 1)
  assert.ok(long[0] * DT < 0.35, `blinked at ${long[0] * DT}s of a 0.5s look`)
  assert.deepEqual(look(new GazeShiftBlink(), 0.2, 0.35), [])
})

test('some long looks keep the eyes open, and eyes just opened from a blink stay open', () => {
  assert.deepEqual(look(new GazeShiftBlink(), 0.7, 0.5, () => 0.9), [])
  let frame = 0
  const justBlinked = look(new GazeShiftBlink(), 0.7, 0.5, () => 0, () => frame++ < 10)
  assert.deepEqual(justBlinked, [])
})
