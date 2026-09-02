import assert from 'node:assert/strict'
import test from 'node:test'
import { compileSpeechBehaviorPlan } from './speechBehaviorPlan'

test('turns future TTS accents into co-speech behaviors that prepare before the stroke', () => {
  const plan = compileSpeechBehaviorPlan({
    utteranceId: 'utt-1',
    startedAtMs: 1_000,
    durationMs: 800,
    accents: [{ offsetMs: 300, intensity: 0.8 }],
  })
  const behavior = plan.behaviors.find(
    (candidate) => candidate.form.id === 'accent',
  )
  assert.equal(behavior?.function, 'emphasize')
  assert.equal(behavior?.source, 'coSpeech')
  const times = new Map(plan.pegs.map((peg) => [peg.id, peg.atMs]))
  assert.ok(
    times.get(behavior!.timing.start)! <
      times.get(behavior!.timing.strokePeak)!,
  )
  assert.equal(times.get(behavior!.timing.strokePeak), 1_300)
  assert.ok(plan.behaviors.some((candidate) => candidate.form.id === 'presence'))
})
