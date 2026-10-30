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

test('overstretch is redistributed locally without moving pinned roots or distant hair', () => {
  const surface = fixture()
  surface.candidate[5] = 35
  surface.candidate[7] = 35
  const distant = surface.candidate.slice(8)
  constrainHairSurface(surface)
  assert.deepEqual(surface.candidate.slice(0, 4), surface.base.slice(0, 4))
  assert.deepEqual(surface.candidate.slice(8), distant)
  // Shared diagonal belongs to both triangles, but is constrained only once.
  assert.equal(surface.edges.length, 16)
  for (let i = 0; i < surface.edges.length; i += 2) {
    const a = surface.edges[i] * 2; const b = surface.edges[i + 1] * 2
    const length = (p: Float32Array) => Math.hypot(p[b] - p[a], p[b + 1] - p[a + 1])
    assert.ok(length(surface.candidate) / length(surface.base) <= 1.251)
  }
})

test('projected hair shape and ordinary local give do not get pulled back to the neutral drawing', () => {
  const surface = fixture()
  for (let i = 1; i < surface.base.length; i += 2) surface.base[i] *= 2
  surface.candidate.set(surface.base)
  surface.candidate[5] *= 1.15
  surface.candidate[7] *= 1.15
  const before = surface.candidate.slice()
  constrainHairSurface(surface)
  assert.deepEqual(surface.candidate, before)
})

test('dense long-hair compression propagates across shared vertices without folding', () => {
  const side = 24
  const rest: number[] = []
  const indices: number[] = []
  const along: number[] = []
  for (let y = 0; y < side; y++) {
    for (let x = 0; x < side; x++) {
      rest.push(x * 10, y * 10)
      along.push(y / (side - 1))
      if (x < side - 1 && y < side - 1) {
        const a = y * side + x
        indices.push(a, a + 1, a + side + 1, a, a + side + 1, a + side)
      }
    }
  }
  const surface = bindHairSurface(new Float32Array(rest), new Uint16Array(indices), new Float32Array(along), null)
  for (let i = 0; i < rest.length; i += 2) {
    const offset = 160 * (rest[i + 1] / 230) ** 2 * Math.sin(rest[i] / 70)
    surface.candidate[i] += offset
    surface.candidate[i + 1] += Math.abs(offset) * 0.12
  }
  constrainHairSurface(surface)
  assert.deepEqual(surface.candidate.slice(0, side * 2), surface.base.slice(0, side * 2))
  for (let t = 0; t < indices.length; t += 3) {
    const [a, b, c] = indices.slice(t, t + 3).map(v => v * 2)
    const p = surface.candidate
    const area = (p[b] - p[a]) * (p[c + 1] - p[a + 1]) - (p[b + 1] - p[a + 1]) * (p[c] - p[a])
    assert.ok(area > 5, `triangle ${t / 3} remains oriented`)
  }
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
