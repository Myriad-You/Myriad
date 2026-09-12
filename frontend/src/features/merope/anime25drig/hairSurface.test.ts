import assert from 'node:assert/strict'
import test from 'node:test'
import { bindHairSurface, constrainHairSurface } from './hairSurface'

function fixture() {
  return bindHairSurface(new Float32Array([0, 0, 10, 0, 10, 10, 0, 10, 100, 0, 110, 0, 100, 10]),
    new Uint16Array([0, 1, 2, 0, 2, 3, 4, 5, 6]), new Float32Array([0, 0, 1, 1, 1, 1, 1]), null)
}

test('safe large hair motion is unchanged, not globally attenuated', () => {
  const surface = fixture()
  for (let i = 0; i < surface.candidate.length; i += 2) surface.candidate[i] += 100
  const before = surface.candidate.slice()
  constrainHairSurface(surface)
  assert.deepEqual(surface.candidate, before)
})

test('crossing tips are locally separated without moving roots or distant hair', () => {
  const surface = fixture()
  surface.candidate[4] = -2
  surface.candidate[6] = 12
  const distant = surface.candidate.slice(8)
  constrainHairSurface(surface)
  assert.deepEqual(surface.candidate.slice(0, 4), surface.base.slice(0, 4))
  assert.deepEqual(surface.candidate.slice(8), distant)
  for (let t = 0; t < 6; t += 3) {
    const [a, b, c] = [...surface.indices.slice(t, t + 3)].map(v => v * 2)
    const p = surface.candidate
    const area = (p[b] - p[a]) * (p[c + 1] - p[a + 1]) - (p[b + 1] - p[a + 1]) * (p[c] - p[a])
    assert.ok(area > 19.9)
  }
})

test('local correction commutes with the parent rotation and translation', () => {
  const original = fixture()
  original.candidate[4] = -2
  original.candidate[6] = 12
  const free = original.candidate.slice()
  constrainHairSurface(original)
  for (const angle of [-0.4, 0.4]) {
    const c = Math.cos(angle)
    const s = Math.sin(angle)
    const transform = (points: Float32Array) => {
      const output = points.slice()
      for (let i = 0; i < output.length; i += 2) {
        output[i] = c * points[i] - s * points[i + 1] + 50
        output[i + 1] = s * points[i] + c * points[i + 1] - 30
      }
      return output
    }
    const moved = fixture()
    moved.base.set(transform(original.base))
    moved.candidate.set(transform(free))
    constrainHairSurface(moved)
    const expected = transform(original.candidate)
    for (let i = 0; i < expected.length; i++) assert.ok(Math.abs(moved.candidate[i] - expected[i]) < 0.00003)
  }
})

test('the moving primary pose is the reference, with no accumulated correction', () => {
  for (const fps of [30, 60, 120]) {
    const surface = fixture()
    const rest = surface.base.slice()
    for (let frame = 0; frame < fps * 2; frame++) {
      const angle = Math.sin(frame / fps) * 0.4
      const c = Math.cos(angle)
      const s = Math.sin(angle)
      for (let i = 0; i < rest.length; i += 2) {
        surface.base[i] = (c * rest[i] - s * rest[i + 1]) * 0.6 + 20
        surface.base[i + 1] = (s * rest[i] + c * rest[i + 1]) * 0.6 - 30
      }
      surface.candidate.set(surface.base)
      const before = surface.candidate.slice()
      constrainHairSurface(surface)
      assert.deepEqual(surface.candidate, before)
      surface.candidate[4] -= 15
      const free = surface.candidate.slice()
      constrainHairSurface(surface)
      const final = surface.candidate.slice()
      surface.candidate.set(free)
      constrainHairSurface(surface)
      assert.deepEqual(surface.candidate, final)
    }
  }
})
