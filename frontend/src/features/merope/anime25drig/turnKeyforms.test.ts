import assert from 'node:assert/strict'
import test from 'node:test'
import { bindTurnKeyform, isAnime25DTurnKeyforms, turnKeyformFamily, turnKeyformOffset } from './turnKeyforms'

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
  assert.equal(turnKeyformFamily({ group: 'body', role: 'topwear', side: null }), null)
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
