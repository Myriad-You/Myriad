import assert from 'node:assert/strict'
import test from 'node:test'
import { applySurfaceContact, bindSurfaceContact } from './surfaceContact'

function surface() {
  const rest = new Float32Array([0, 0, 10, 0, 10, 10, 0, 10])
  return {
    rest,
    deformed: rest.slice(),
    indices: new Uint16Array([0, 1, 2, 0, 2, 3]),
  }
}

test('contact preserves neutral geometry including its outside feather', () => {
  const host = surface()
  const rest = new Float32Array([2, 3, -2, 4, 12, 5, 3, 12])
  const weights = new Float32Array([1, 0.6, 0.2, 0])
  const contact = bindSurfaceContact(host, rest, weights)
  weights.fill(0)
  const actual = rest.slice()
  applySurfaceContact(contact, actual)
  assert.deepEqual(actual, rest)
  assert.equal(
    contact.weights[0],
    1,
    'compiled field does not alias mutable input',
  )
})

test('contact follows final host affine motion without constraining the distal vertices', () => {
  for (const fps of [30, 60, 120]) {
    const host = surface()
    const rest = new Float32Array([2, 3, -2, 4, 12, 5, 3, 12])
    const weights = new Float32Array([1, 0.6, 0.2, 0])
    const contact = bindSurfaceContact(host, rest, weights)
    for (let frame = 0; frame <= fps * 4; frame++) {
      const t = frame / fps
      const theta = Math.sin(t * 2) * 0.6
      const c = Math.cos(theta)
      const s = Math.sin(theta)
      const tx = Math.sin(t) * 3
      const ty = Math.cos(t) * 2
      for (let i = 0; i < host.rest.length; i += 2) {
        host.deformed[i] = c * host.rest[i] - s * host.rest[i + 1] + tx
        host.deformed[i + 1] = s * host.rest[i] + c * host.rest[i + 1] + ty
      }
      const free = rest.map((v, i) => v + Math.sin(t * 3 + i) * 2)
      const actual = free.slice()
      applySurfaceContact(contact, actual)
      for (let vertex = 0; vertex < weights.length; vertex++) {
        const i = vertex * 2
        const w = weights[vertex]
        const expectedX =
          free[i] * (1 - w) + (c * rest[i] - s * rest[i + 1] + tx) * w
        const expectedY =
          free[i + 1] * (1 - w) + (s * rest[i] + c * rest[i + 1] + ty) * w
        assert.ok(Math.abs(actual[i] - expectedX) < 3e-6)
        assert.ok(Math.abs(actual[i + 1] - expectedY) < 3e-6)
      }
    }
  }
})

test('CPU and shader host ownership produce identical contacts, without double transforms', () => {
  const cpu = surface()
  const shader = {
    ...surface(),
    transform: new Float32Array([0, 1, 0, -1, 0, 0, 20, 30, 1]),
  }
  for (let i = 0; i < cpu.rest.length; i += 2) {
    shader.deformed[i] += i * 0.2
    cpu.deformed[i] = 20 - shader.deformed[i + 1]
    cpu.deformed[i + 1] = 30 + shader.deformed[i]
  }
  const rest = new Float32Array([2, 3, 8, 7])
  const weights = new Float32Array([1, 0.5])
  const a = rest.slice()
  const b = rest.slice()
  applySurfaceContact(bindSurfaceContact(cpu, rest, weights), a)
  applySurfaceContact(bindSurfaceContact(shader, rest, weights), b)
  for (let i = 0; i < a.length; i++) assert.ok(Math.abs(a[i] - b[i]) < 3e-6)
})

test('invalid contact fields and degenerate hosts fail at binding, not during playback', () => {
  const rest = new Float32Array([1, 2])
  for (const weights of [[], [NaN], [-1], [2]]) {
    assert.throws(
      () => bindSurfaceContact(surface(), rest, new Float32Array(weights)),
      /weight/,
    )
  }
  const host = surface()
  host.rest.fill(0)
  assert.throws(
    () => bindSurfaceContact(host, rest, new Float32Array([1])),
    /degenerate/,
  )
})

test('unchanged final contacts stay clean without accumulating the previous correction', () => {
  const host = surface()
  host.deformed[0] += 0.7
  const rest = new Float32Array([2, 3, 3, 12])
  const contact = bindSurfaceContact(host, rest, new Float32Array([0.8, 0]))
  const output = rest.slice()
  assert.equal(
    applySurfaceContact(contact, contact.unconstrained, output),
    true,
  )
  const expected = output.slice()
  for (let frame = 0; frame < 120; frame++) {
    assert.equal(
      applySurfaceContact(contact, contact.unconstrained, output),
      false,
    )
    assert.deepEqual(output, expected)
  }
  assert.deepEqual(contact.unconstrained, rest)
  contact.unconstrained[2] += 1
  assert.equal(
    applySurfaceContact(contact, contact.unconstrained, output),
    true,
  )
  assert.equal(output[2], rest[2] + 1, 'unbound distal geometry still updates')
})
