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

test('a nod key and the turn key make the corners together', () => {
  const still = lattice(5, () => [0, 0])
  const keyforms = {
    face: {
      plus: lattice(5, () => [-10, 0]),
      minus: lattice(5, () => [10, 0]),
      up: lattice(5, () => [0, 6]),
      down: lattice(5, () => [0, -4]),
    },
    nose: { plus: still, minus: still },
    ears: { plus: still, minus: still },
  }
  assert.ok(isAnime25DTurnKeyforms(keyforms))
  const rest = new Float32Array([50, 50])
  const bound = bindTurnKeyform(keyforms, { group: 'head', role: 'face', side: null }, rest)!
  const out = { x: 0, y: 0 }
  turnKeyformOffset(bound, 0, 1, out, 1)
  assert.ok(Math.abs(out.x - 10) < 0.05 && Math.abs(out.y + 6) < 0.05, `${out.x} ${out.y}`)
  turnKeyformOffset(bound, 0, 0, out, -0.5)
  assert.ok(Math.abs(out.x) < 0.05 && Math.abs(out.y - 2) < 0.05, `${out.x} ${out.y}`)
  // Without nod keys the nod is left to the computed head, but for a feature on the face, which nods with it.
  const ears = bindTurnKeyform(keyforms, { group: 'head', role: 'ears', side: null }, rest)!
  assert.equal(ears.up, null)
  const nose = bindTurnKeyform(keyforms, { group: 'head', role: 'nose', side: null }, rest)!
  assert.ok(nose.up !== null && Math.abs(nose.up[1] + 6) < 0.05)
})

test('a rest point squeezed hard by its key still finds where it went', () => {
  // The far side squeezed to a third about x = 50: stepping t ← q − back(t)
  // overshoots twice over each step and never settles there.
  const keyforms = { face: { plus: lattice(9, (x) => [50 + (x - 50) * 3 - x, 0]), minus: lattice(9, () => [0, 0]) } }
  const rest = new Float32Array([20, 50, 45, 50, 50, 50, 80, 50])
  const bound = bindTurnKeyform(keyforms, { group: 'head', role: 'face', side: null }, rest)!
  for (let i = 0; i < rest.length; i += 2) {
    const turned = rest[i] + bound.plus[i]
    assert.ok(Math.abs(turned - (50 + (rest[i] - 50) / 3)) < 0.01, `${rest[i]} → ${turned}`)
  }
})

test('a turn and a nod that each narrow a part narrow it in turn, not twice over', () => {
  // Both keys halve the width about x = 50. Added, the full corner would
  // take the whole width away; composed, a quarter is left.
  const halved = lattice(9, (x) => [50 + (x - 50) * 2 - x, 0])
  const keyforms = { face: { plus: halved, minus: halved, up: halved, down: halved } }
  const rest = new Float32Array([40, 50, 60, 50])
  const bound = bindTurnKeyform(keyforms, { group: 'head', role: 'face', side: null }, rest)!
  const width = (amount: number, nod: number) => {
    const a = { x: 0, y: 0 }
    const b = { x: 0, y: 0 }
    turnKeyformOffset(bound, 0, amount, a, nod)
    turnKeyformOffset(bound, 1, amount, b, nod)
    return 60 + b.x - (40 + a.x)
  }
  assert.ok(Math.abs(width(1, 1) - 5) < 0.01, `${width(1, 1)}`)
  assert.ok(Math.abs(width(-1, -1) - 5) < 0.01, `${width(-1, -1)}`)
  // Along an axis the key alone, as before; in between, the four keys mixed.
  assert.ok(Math.abs(width(1, 0) - 10) < 0.01 && Math.abs(width(0, -1) - 10) < 0.01)
  assert.ok(Math.abs(width(0.5, 0.5) - 20 * (1 - 0.25 - 0.25 + 0.25 * 0.25)) < 0.01, `${width(0.5, 0.5)}`)
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

test('a mouth without a key of its own turns with the face', () => {
  const face = { plus: shiftedLattice(6), minus: shiftedLattice(-6) }
  const keyforms = { face }
  const rest = Float32Array.of(100, 100)
  const mouth = bindTurnKeyform(keyforms, { group: 'head', role: 'mouth-close', side: null }, rest)!
  assert.ok(Math.abs(mouth.plus[0] - 6) < 1e-3 && Math.abs(mouth.minus[0] + 6) < 1e-3)
  // Its own key, where there is one, still wins; hair has no such fallback.
  const own = bindTurnKeyform({ face, mouth: { plus: shiftedLattice(2), minus: shiftedLattice(-2) } }, { group: 'head', role: 'mouth-close', side: null }, rest)!
  assert.ok(Math.abs(own.plus[0] - 2) < 1e-3)
  assert.equal(bindTurnKeyform(keyforms, { group: 'head', role: 'back-hair', side: null }, rest), null)
})

test('a feature keyed across but not nodding nods with the face under it', () => {
  const nodded = (dy: number) => ({ box: [0, 0, 200, 200] as [number, number, number, number], grid: 2, back: [0, -dy, 0, -dy, 0, -dy, 0, -dy], multiply: [0.8, 0.8, 0.8] as [number, number, number] })
  const face = { plus: shiftedLattice(6), minus: shiftedLattice(-6), up: nodded(-9), down: nodded(9) }
  const mouthKey = { plus: shiftedLattice(2), minus: shiftedLattice(-2) }
  const rest = Float32Array.of(100, 100)
  const mouth = bindTurnKeyform({ face, mouth: mouthKey }, { group: 'head', role: 'mouth-close', side: null }, rest)!
  // Its own turn stays; the nod is the face's, without the face's colour.
  assert.ok(Math.abs(mouth.plus[0] - 2) < 1e-3)
  assert.ok(Math.abs(mouth.down![1] - 9) < 1e-3 && Math.abs(mouth.up![1] + 9) < 1e-3)
  assert.equal(mouth.multiply, null)
  // A nod of its own still wins, and hair takes no nod from the face.
  const ownNod = bindTurnKeyform({ face, mouth: { ...mouthKey, up: nodded(-3), down: nodded(3) } }, { group: 'head', role: 'mouth-close', side: null }, rest)!
  assert.ok(Math.abs(ownNod.down![1] - 3) < 1e-3)
  const hair = bindTurnKeyform({ face, 'back-hair': mouthKey }, { group: 'head', role: 'back-hair', side: null }, rest)!
  assert.equal(hair.down, null)
})

function shiftedLattice(dx: number) {
  return { box: [0, 0, 200, 200] as [number, number, number, number], grid: 2, back: [-dx, 0, -dx, 0, -dx, 0, -dx, 0] }
}

test('a mouth\'s key moves and narrows it but does not shear it', () => {
  // Sheared: what the turned drawing shows came from further left the lower it lies.
  const sheared = lattice(9, (_, y) => [-0.4 * y, 0])
  const keyforms = { mouth: { plus: sheared, minus: sheared }, face: { plus: sheared, minus: sheared } }
  // A mouth's corners, top and bottom.
  const rest = new Float32Array([40, 40, 60, 40, 40, 60, 60, 60])
  const offsets = (role: string) => {
    const bound = bindTurnKeyform(keyforms, { group: 'head', role, side: null }, rest)!
    return [0, 1, 2, 3].map((v) => {
      const out = { x: 0, y: 0 }
      turnKeyformOffset(bound, v, 1, out)
      return out.x
    })
  }
  const [topLeft, topRight, bottomLeft, bottomRight] = offsets('mouth-open')
  assert.ok(Math.abs(topLeft - bottomLeft) < 1e-4 && Math.abs(topRight - bottomRight) < 1e-4, 'the top and bottom move alike')
  assert.ok(Math.abs(topLeft - 20) < 0.5, `moved with the key's mean (${topLeft})`)
  // The face under it keeps its key whole.
  const face = offsets('face')
  assert.ok(face[2] - face[0] > 5, 'the face is sheared as keyed')
})

test('an accessory lying outside its key\'s box is another piece: it is not moved by that key', () => {
  const key = { plus: shiftedLattice(10), minus: shiftedLattice(-10) }
  const earwear = { group: 'head' as const, role: 'earwear', side: null }
  assert.ok(bindTurnKeyform({ earwear: key }, earwear, new Float32Array([50, 50, 60, 60])), 'inside, keyed')
  assert.equal(bindTurnKeyform({ earwear: key }, earwear, new Float32Array([250, 50, 260, 60])), null, 'outside, left to its host')
  // Other parts keep their key wherever they lie.
  assert.ok(bindTurnKeyform({ face: key }, { group: 'head', role: 'face', side: null }, new Float32Array([250, 50, 260, 60])))
})
