import assert from 'node:assert/strict'
import test from 'node:test'
import { SpeechFormTransition } from './speechFormTransition'

test('a form correction transports the drawn shares without delaying an initial gesture', () => {
  const shape = new SpeechFormTransition('question')
  assert.equal(shape.sample(1).question, 1)
  shape.revise('hesitate', 1, 1.14)
  assert.equal(shape.sample(1).question, 1)
  assert.ok(shape.sample(1.07).question > 0)
  assert.ok(shape.sample(1.07).hesitate > 0)
  assert.equal(shape.sample(1.14).hesitate, 1)
})

test('restatements do not restart arrival and a second correction starts at the current mixture', () => {
  const shape = new SpeechFormTransition('question')
  shape.revise('hesitate', 1, 1.16)
  const halfway = { ...shape.sample(1.08) }
  shape.revise('hesitate', 1.08, 1.16)
  assert.deepEqual(shape.sample(1.08), halfway)
  shape.revise('tease', 1.08, 1.22)
  assert.deepEqual(shape.sample(1.08), halfway)
  assert.equal(shape.sample(1.23).tease, 1)
})

test('generic accents and cancellation use the same bounded shares, without a hidden tail', () => {
  const shape = new SpeechFormTransition('accent')
  shape.revise('laugh', 1, 1.16)
  const drawn = { ...shape.sample(1.05) }
  assert.ok(drawn.accent > 0 && drawn.laugh > 0)
  shape.freeze(1.05)
  assert.deepEqual(shape.sample(100), drawn)
  for (let at = 1.05; at < 2; at += 1 / 120) {
    const values = Object.values(shape.sample(at))
    assert.ok(values.every((value) => value >= 0 && value <= 1))
    assert.ok(
      Math.abs(values.reduce((sum, value) => sum + value, 0) - 1) < 1e-10,
    )
  }
})

test('sampling cadence does not change the trajectory or allocate a new output', () => {
  const results = [30, 60, 120].map((fps) => {
    const shape = new SpeechFormTransition('question')
    shape.revise('tease', 1, 1.16)
    const identity = shape.sample(1)
    for (let frame = 0; frame < fps / 10; frame++) shape.sample(1 + frame / fps)
    assert.equal(shape.sample(1.1), identity)
    return { ...identity }
  })
  assert.deepEqual(results[0], results[1])
  assert.deepEqual(results[1], results[2])
})
