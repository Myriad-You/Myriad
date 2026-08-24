import assert from 'node:assert/strict'
import test from 'node:test'
import { baselineDriverPatch, cueDriverPatch, cueDurationMs } from './performanceMotion'

test('maps semantic baselines without authoring lip sync', () => {
  const patch = baselineDriverPatch({
    expression: 'warm',
    posture: 'open',
    motionEnergy: 1.4,
    attention: 0.8,
  })
  assert.equal(patch.mouthOpen, undefined)
  assert.equal(patch.talk, undefined)
  assert.ok((patch.mouthForm || 0) > 0)
  assert.ok((patch.armPos || 0) <= 0.2)
})

test('centers semantic hair energy on the runtime sway defaults', () => {
  const patch = baselineDriverPatch({
    expression: 'steady',
    posture: 'neutral',
    motionEnergy: 1,
    attention: 1,
  })
  assert.equal(patch.fhAmp, 1)
  assert.equal(patch.physAmp, 0.5)
})

test('maps cues to bounded deterministic patches and durations', () => {
  const cue = {
    intent: 'emphasize' as const,
    atMs: 0,
    intensity: 1.4,
    tempo: 1,
    fadeInMs: 150,
    fadeOutMs: 220,
    interrupt: 'replace' as const,
  }
  assert.ok((cueDriverPatch(cue).body || 0) < 0.35)
  assert.equal(cueDurationMs(cue), 1_090)
})
