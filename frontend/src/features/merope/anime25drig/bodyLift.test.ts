import assert from 'node:assert/strict'
import test from 'node:test'
import { applyBodyLift, BodyLiftResponse } from './bodyLift'

test('pitch is rest-relative, bounded, and carries the head without flattening it', () => {
  for (const pitch of [-1, -0.18, 0, 0.18, 1]) {
    const f = { centerX: 500, shoulderY: 600, upperY: 800, lowerY: 1200, amount: 0, pitch, depth: 200 }
    const head = [{ x: 450, y: 400 }, { x: 550, y: 400 }, { x: 450, y: 500 }]
    head.forEach(p => applyBodyLift(p, f))
    assert.ok(Math.abs((head[1].x - head[0].x) - (head[2].y - head[0].y)) < 1e-8)
    const cut = { x: 630, y: 1200 }
    applyBodyLift(cut, f)
    assert.deepEqual(cut, { x: 630, y: 1200 })
    let previous = -Infinity
    for (let y = 200; y <= 1400; y++) {
      const p = { x: 650, y }
      applyBodyLift(p, f)
      assert.ok(p.y > previous)
      assert.ok(p.x > 630 && p.x < 670)
      if (pitch === 0) assert.deepEqual(p, { x: 650, y })
      previous = p.y
    }
  }
})

test('pitch joins the rigid upper frame and crop with continuous first derivatives', () => {
  const f = { centerX: 500, shoulderY: 600, upperY: 800, lowerY: 1200, amount: 0, pitch: 0.18, depth: 200 }
  for (const y of [f.shoulderY, f.lowerY]) {
    const a = { x: 650, y: y - 0.001 }; const b = { x: 650, y }; const c = { x: 650, y: y + 0.001 }
    for (const p of [a, b, c]) applyBodyLift(p, f)
    assert.ok(Math.abs((b.y - a.y) - (c.y - b.y)) / 0.001 < 1e-5)
    assert.ok(Math.abs((b.x - a.x) - (c.x - b.x)) / 0.001 < 1e-5)
  }
})

test('upper structure translates rigidly, cut stays fixed, waist compensates extension', () => {
  for (const amount of [-20, 20]) {
    const f = { centerX: 500, upperY: 800, lowerY: 1200, amount }
    const head = { x: 530, y: 200 }; const chest = { x: 600, y: 750 }; const cut = { x: 100, y: 1200 }; const waist = { x: 650, y: 1000 }
    for (const p of [head, chest, cut, waist]) applyBodyLift(p, f)
    assert.equal(head.y, 200 - amount)
    assert.equal(chest.y, 750 - amount)
    assert.equal(chest.x, 600)
    assert.deepEqual(cut, { x: 100, y: 1200 })
    assert.equal(waist.y, 1000 - amount / 2)
    assert.equal(waist.x < 650, amount > 0)
  }
})

test('bounded postures remain monotone without collapsing width', () => {
  for (const amount of [-500, -20, 0, 20, 500]) {
    const f = { centerX: 500, upperY: 800, lowerY: 1200, amount }
    let lastY = -Infinity
    for (let y = 700; y <= 1300; y += 0.5) {
      const p = { x: 640, y }
      applyBodyLift(p, f)
      assert.ok(p.y > lastY)
      lastY = p.y
      assert.ok(p.x > 630 && p.x < 651)
    }
  }
})

test('posture response agrees across frame rates and does not jump on reversal', () => {
  const values = [30, 60, 120].map(fps => {
    const state = new BodyLiftResponse()
    for (let i = 0; i < fps; i++) state.step(1, 1 / fps)
    const before = state.value
    state.step(-1, 0)
    assert.equal(state.value, before)
    for (let i = 0; i < fps; i++) state.step(-1, 1 / fps)
    return state.value
  })
  assert.ok(Math.max(...values) - Math.min(...values) < 1e-10)
})
