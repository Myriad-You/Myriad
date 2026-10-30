import assert from 'node:assert/strict'
import test from 'node:test'
import { AutoSpeechController } from './speechMotion'
import { stepMouthForm, stepMouthOpen } from './speechResponse'

test('opens promptly and returns to rest more softly', () => {
  const opened = stepMouthOpen(0, 1, 1 / 60)
  const released = stepMouthOpen(1, 0, 1 / 60)
  assert.ok(opened > 1 - released)
  assert.ok(opened > 0.25 && opened < 0.3)
  assert.ok(released > 0.8 && released < 0.85)
})

test('keeps mouth form steadier than jaw opening', () => {
  const jawTravel = stepMouthOpen(0, 1, 1 / 60)
  const formTravel = stepMouthForm(0, 1, 1 / 60)
  assert.ok(formTravel < jawTravel)
})

test('produces the same constant-target response at 30 and 60 fps', () => {
  let at30 = 0
  let at60 = 0
  for (let frame = 0; frame < 30; frame += 1) {
    at30 = stepMouthOpen(at30, 0.75, 1 / 30)
  }
  for (let frame = 0; frame < 60; frame += 1) {
    at60 = stepMouthOpen(at60, 0.75, 1 / 60)
  }
  assert.ok(Math.abs(at30 - at60) < 1e-12)
})

test('keeps the complete preview response consistent at 30 and 60 fps', () => {
  const run = (framesPerSecond: number) => {
    const speech = new AutoSpeechController(() => 0.5)
    let mouthOpen = 0
    let mouthForm = 0
    speech.sample(0, true)
    for (let frame = 1; frame <= framesPerSecond * 12; frame += 1) {
      const target = speech.sample(frame / framesPerSecond, true)
      mouthOpen = stepMouthOpen(
        mouthOpen,
        target.mouthOpen,
        1 / framesPerSecond,
      )
      mouthForm = stepMouthForm(
        mouthForm,
        target.mouthForm,
        1 / framesPerSecond,
      )
    }
    return { mouthOpen, mouthForm }
  }
  const at30 = run(30)
  const at60 = run(60)
  assert.ok(Math.abs(at30.mouthOpen - at60.mouthOpen) < 0.01)
  assert.ok(Math.abs(at30.mouthForm - at60.mouthForm) < 0.01)
})

test('settles exactly and ignores unusable frame deltas', () => {
  assert.equal(stepMouthOpen(0.5, 0.5 + 1e-6, 1 / 60), 0.5 + 1e-6)
  assert.equal(stepMouthOpen(0.25, 1, Number.NaN), 0.25)
  assert.equal(stepMouthForm(0.25, 1, -1), 0.25)
})
