import type { JellyElement } from './jellyVolume'
import assert from 'node:assert/strict'
import test from 'node:test'
import { HAIR_JELLY, jellyDisplacement, JellyVolume } from './jellyVolume'

const DT = 1 / 60
const HANGING: JellyElement = { anchorX: 0, anchorY: 0, axisX: 0, axisY: 1, length: 200, cutY: null }

/** The anchor rises 40 px in 0.2 s and stops dead; returns the stretch per frame. */
function rise(jelly = new JellyVolume(HAIR_JELLY), dynamic = true) {
  const trace: number[] = []
  for (let i = 0; i < 180; i += 1) {
    const t = Math.min(1, (i * DT) / 0.2)
    const y = -40 * t * t * (3 - 2 * t)
    jelly.step(0, y, 0, 1, 200, DT, dynamic)
    trace.push(jelly.stretch)
  }
  return trace
}

test('a still anchor leaves the volume exactly as drawn', () => {
  const jelly = new JellyVolume(HAIR_JELLY)
  for (let i = 0; i < 120; i += 1) jelly.step(10, 20, 0, 1, 200, DT, true)
  assert.equal(jelly.stretch, 0)
  assert.equal(jelly.sway, 0)
})

test('a mass hung from a rising anchor stretches, then squashes past rest, and settles', () => {
  const trace = rise()
  assert.ok(Math.max(...trace) > 0.01, `stretch ${Math.max(...trace)}`)
  assert.ok(Math.min(...trace) < -0.005, `squash ${Math.min(...trace)}`)
  let crossings = 0
  for (let i = 1; i < trace.length; i += 1) { if (Math.sign(trace[i]) !== Math.sign(trace[i - 1])) crossings += 1
}
  assert.ok(crossings >= 3, `${crossings} wobbles`)
  assert.ok(Math.abs(trace.at(-1)!) < 1e-3)
  assert.ok(Math.max(...trace.map(Math.abs)) <= HAIR_JELLY.stretchLimit)
})

test('physics off holds the drawing still', () => {
  assert.ok(rise(new JellyVolume(HAIR_JELLY), false).every((value) => value === 0))
})

test('stretching narrows and squashing widens: the free end keeps its area', () => {
  const shift = { x: 0, y: 0 }
  const moved = (x: number, y: number, stretch: number) => {
    jellyDisplacement(x, y, HANGING, stretch, 0, shift)
    return [x + shift.x, y + shift.y]
  }
  for (const stretch of [0.05, -0.05]) {
    // A small triangle past the free end, where the whole stretch applies.
    const [a, b, c] = [moved(0, 220, stretch), moved(10, 220, stretch), moved(0, 230, stretch)]
    const area = Math.abs((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2
    assert.ok(Math.abs(area - 50) < 1e-6, `${stretch}: ${area}`)
    const width = b[0] - a[0]
    assert.ok(stretch > 0 ? width < 10 : width > 10)
  }
})

test('the anchor and a canvas cut stay where they are drawn', () => {
  const shift = { x: 0, y: 0 }
  jellyDisplacement(30, 0, HANGING, 0.05, 0.02, shift)
  assert.deepEqual(shift, { x: 0, y: 0 })
  jellyDisplacement(30, 180, { ...HANGING, cutY: 180 }, 0.05, 0.02, shift)
  assert.deepEqual(shift, { x: 0, y: 0 })
  jellyDisplacement(30, 120, { ...HANGING, cutY: 180 }, 0.05, 0.02, shift)
  assert.ok(Math.hypot(shift.x, shift.y) > 0)
})
