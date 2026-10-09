import assert from 'node:assert/strict'
import test from 'node:test'
import { bindAttachmentTurn, bindTurnKeyform, deformAttachmentTurn, isAnime25DTurnKeyforms, turnKeyformFamily, turnKeyformMultiply, turnKeyformOffset } from './turnKeyforms'

function lattice(grid: number, back: (x: number, y: number) => [number, number]) {
  const box: [number, number, number, number] = [0, 0, 100, 100]
  const values: number[] = []
  for (let j = 0; j < grid; j++) {
    for (let i = 0; i < grid; i++) values.push(...back((i / (grid - 1)) * 100, (j / (grid - 1)) * 100))
  }
  return { box, grid, back: values }
}

test('every drawing of one eye turns with that eye', () => {
  assert.equal(turnKeyformFamily({ group: 'head', role: 'eyewhite', side: 'L' }), 'eye:L')
  assert.equal(turnKeyformFamily({ group: 'head', role: 'eye-close', side: 'L' }), 'eye:L')
  assert.equal(turnKeyformFamily({ group: 'head', role: 'mouth-open', side: null }), 'mouth')
  assert.equal(turnKeyformFamily({ group: 'head', role: 'eyebrow', side: 'R' }), 'brow:R')
  assert.equal(turnKeyformFamily({ group: 'head', role: 'anger-mark', side: null }), 'face')
  assert.equal(turnKeyformFamily({ group: 'body', role: 'neck', side: null }), 'neck')
  assert.equal(turnKeyformFamily({ group: 'body', role: 'neckwear', side: null }), 'neckwear')
  assert.equal(turnKeyformFamily({ group: 'body', role: 'topwear', side: null }), null)
})

test('an iris turns by its own key where one was measured, else with its eye', () => {
  const eye = { plus: lattice(3, () => [1, 0]), minus: lattice(3, () => [1, 0]) }
  const iris = { plus: lattice(3, () => [5, 0]), minus: lattice(3, () => [5, 0]) }
  const rest = new Float32Array([50, 50])
  const offset = (keyforms: Record<string, typeof eye>, role: string) => {
    const bound = bindTurnKeyform(keyforms, { group: 'head', role, side: 'L' }, rest)!
    const out = { x: 0, y: 0 }
    turnKeyformOffset(bound, 0, 1, out)
    return out.x
  }
  assert.equal(offset({ 'eye:L': eye, 'iris:L': iris }, 'irides'), -5)
  assert.equal(offset({ 'eye:L': eye, 'iris:L': iris }, 'iris-silly'), -5)
  assert.equal(offset({ 'eye:L': eye, 'iris:L': iris }, 'eyewhite'), -1)
  assert.equal(offset({ 'eye:L': eye, 'iris:R': iris }, 'irides'), -1)
  assert.equal(offset({ 'eye:L': eye }, 'irides'), -1)
})

test('a rest point goes where the turned drawing says it came from', () => {
  // Turned toward +x, everything came from 10 px to its left and was squeezed
  // to 80% about x = 50: back(t) = (50 + (t − 50) / 0.8 − 10) − t.
  const keyforms = {
    face: {
      plus: lattice(9, (x) => [50 + (x - 50) / 0.8 - 10 - x, 0]),
      minus: lattice(9, () => [10, 0]),
    },
  }
  assert.ok(isAnime25DTurnKeyforms(keyforms))
  const rest = new Float32Array([40, 50, 70, 50])
  const bound = bindTurnKeyform(keyforms, { group: 'head', role: 'face', side: null }, rest)!
  const out = { x: 0, y: 0 }
  turnKeyformOffset(bound, 0, 1, out)
  // Forward: t = 50 + (q + 10 − 50) · 0.8.
  assert.ok(Math.abs(40 + out.x - (50 + (40 + 10 - 50) * 0.8)) < 0.05, `${out.x}`)
  turnKeyformOffset(bound, 1, 1, out)
  assert.ok(Math.abs(70 + out.x - (50 + (70 + 10 - 50) * 0.8)) < 0.05, `${out.x}`)
  // Halfway is half the move; the other way uses the other key.
  turnKeyformOffset(bound, 1, 0.5, out)
  assert.ok(Math.abs(out.x - (50 + (70 + 10 - 50) * 0.8 - 70) / 2) < 0.05)
  turnKeyformOffset(bound, 0, -1, out)
  assert.ok(Math.abs(out.x + 10) < 0.05, `${out.x}`)
})

test('a part with no key is left to the computed turn', () => {
  assert.equal(bindTurnKeyform({}, { group: 'head', role: 'face', side: null }, new Float32Array(2)), null)
  assert.equal(bindTurnKeyform(undefined, { group: 'head', role: 'face', side: null }, new Float32Array(2)), null)
})

test('malformed keys are refused', () => {
  assert.equal(isAnime25DTurnKeyforms({ face: { plus: lattice(3, () => [0, 0]) } }), false)
  assert.equal(isAnime25DTurnKeyforms({ face: { plus: { box: [0, 0, 1, 1], grid: 3, back: [0] }, minus: lattice(3, () => [0, 0]) } }), false)
  assert.equal(isAnime25DTurnKeyforms([]), false)
})

test('a nod key adds to the turn key, as Live2D fills the corners from the edges', () => {
  const still = lattice(5, () => [0, 0])
  const keyforms = {
    face: {
      plus: lattice(5, () => [-10, 0]),
      minus: lattice(5, () => [10, 0]),
      up: lattice(5, () => [0, 6]),
      down: lattice(5, () => [0, -4]),
    },
    nose: { plus: still, minus: still },
  }
  assert.ok(isAnime25DTurnKeyforms(keyforms))
  const rest = new Float32Array([50, 50])
  const bound = bindTurnKeyform(keyforms, { group: 'head', role: 'face', side: null }, rest)!
  const out = { x: 0, y: 0 }
  turnKeyformOffset(bound, 0, 1, out, 1)
  assert.ok(Math.abs(out.x - 10) < 0.05 && Math.abs(out.y + 6) < 0.05, `${out.x} ${out.y}`)
  turnKeyformOffset(bound, 0, 0, out, -0.5)
  assert.ok(Math.abs(out.x) < 0.05 && Math.abs(out.y - 2) < 0.05, `${out.x} ${out.y}`)
  // Without nod keys the nod is left to the computed head.
  const nose = bindTurnKeyform(keyforms, { group: 'head', role: 'nose', side: null }, rest)!
  assert.equal(nose.up, null)
})

test('an accessory keyed like its host stays put on it: the carry brings the host turning, once', () => {
  // The host turns 0.1 rad about (50, 50) and moves (4, -3); backward: where each turned point came from.
  const angle = 0.1
  const cos = Math.cos(angle)
  const sin = Math.sin(angle)
  const back = (x: number, y: number): [number, number] => {
    const rx = x - 50 - 4
    const ry = y - 50 + 3
    return [50 + rx * cos + ry * sin - x, 50 - rx * sin + ry * cos - y]
  }
  const keyforms = {
    ears: { plus: lattice(9, back), minus: lattice(9, () => [0, 0]) },
    earwear: { plus: lattice(9, back), minus: lattice(9, () => [0, 0]) },
  }
  const rest = new Float32Array([50, 70, 60, 80])
  const turn = bindAttachmentTurn(keyforms, { group: 'head', role: 'earwear', side: null }, rest,
    { group: 'head', role: 'ears', side: null }, { x: 50, y: 50 })!
  const deformed = new Float32Array(rest.length)
  assert.equal(deformAttachmentTurn(turn, 1, 0, rest, deformed), true)
  for (let i = 0; i < rest.length; i++) assert.ok(Math.abs(deformed[i] - rest[i]) < 0.05, `${i}: ${deformed[i]} vs ${rest[i]}`)
  assert.equal(deformAttachmentTurn(turn, 1, 0, rest, deformed), false)
})

test('a lock of front hair turns by its own key, else with the whole front hair', () => {
  const keyforms = {
    'front-hair': { plus: lattice(5, () => [-10, 0]), minus: lattice(5, () => [10, 0]) },
    'front-hair:2': { plus: lattice(5, () => [-4, 0]), minus: lattice(5, () => [4, 0]) },
  }
  const rest = new Float32Array([50, 50])
  const out = { x: 0, y: 0 }
  const own = bindTurnKeyform(keyforms, { group: 'head', role: 'front-hair', side: null, name: 'front-hair-2' }, rest)!
  turnKeyformOffset(own, 0, 1, out)
  assert.ok(Math.abs(out.x - 4) < 0.05, `${out.x}`)
  const whole = bindTurnKeyform(keyforms, { group: 'head', role: 'front-hair', side: null, name: 'front-hair-3' }, rest)!
  turnKeyformOffset(whole, 0, 1, out)
  assert.ok(Math.abs(out.x - 10) < 0.05, `${out.x}`)
})

test('a keyed multiply colour mixes from white by the turn and the nod, and a key without one stays white', () => {
  const still = lattice(2, () => [0, 0])
  const neck = { group: 'body' as const, role: 'neck' as const, side: null }
  const rest = new Float32Array([50, 50])
  const bound = bindTurnKeyform({
    neck: {
      plus: { ...still, multiply: [1.2, 1.1, 1] },
      minus: still,
      up: { ...still, multiply: [1.3, 1.3, 1.3] },
      down: { ...still, multiply: [0.8, 0.8, 0.9] },
    },
  }, neck, rest)!
  const out = new Float32Array(3)
  turnKeyformMultiply(bound, 0.5, 0, out)
  assert.deepEqual([...out].map((v) => +v.toFixed(3)), [1.1, 1.05, 1])
  turnKeyformMultiply(bound, -1, -1, out)
  assert.deepEqual([...out].map((v) => +v.toFixed(3)), [0.8, 0.8, 0.9])
  turnKeyformMultiply(bound, 1, 1, out)
  assert.deepEqual([...out].map((v) => +v.toFixed(3)), [1.5, 1.4, 1.3])
  const plain = bindTurnKeyform({ neck: { plus: still, minus: still } }, neck, rest)!
  turnKeyformMultiply(plain, 1, 0, out)
  assert.deepEqual([...out], [1, 1, 1])
})
