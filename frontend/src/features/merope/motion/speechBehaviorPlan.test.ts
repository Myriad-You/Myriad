import assert from 'node:assert/strict'
import test from 'node:test'
import { realizeAnime25DBehaviorPlan } from '../anime25drig/behaviorRealizer'
import { continueTextProsody, predictTextProsody } from '../speech/textProsody'
import { HumanPerformanceRuntime } from './humanPerformanceRuntime'
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
  assert.ok(
    plan.behaviors.some((candidate) => candidate.form.id === 'presence'),
  )
})

test('appending a later phrase keeps a committed peak and gives the new beat its own lifecycle', () => {
  const input = {
    utteranceId: 'phrases',
    text: '其实可以试试。',
    startedAtMs: 1_000,
    streaming: true,
  }
  const first = continueTextProsody(predictTextProsody(input), null, 1_000)
  const before = compileSpeechBehaviorPlan(first)
  const runtime = new HumanPerformanceRuntime()
  runtime.frame([before], 1_000)
  const accent = before.behaviors[1]!
  const peak = before.pegs.find(
    (peg) => peg.id === accent.timing.strokePeak,
  )!.atMs
  runtime.frame([before], peak)
  const later = continueTextProsody(
    predictTextProsody({ ...input, text: `${input.text}不过后面还有个问题。` }),
    first,
    peak,
  )
  const after = runtime.frame([compileSpeechBehaviorPlan(later)], peak)
  assert.equal(
    after.plan!.pegs.find((peg) => peg.id === accent.timing.strokePeak)!.atMs,
    peak,
  )
  assert.equal(
    after.behaviors.filter((behavior) => behavior.id === accent.id).length,
    1,
  )
  assert.ok(after.plan!.behaviors.length > before.behaviors.length)
  const realization = realizeAnime25DBehaviorPlan(after.plan!, peak)
  assert.equal(realization.units.length, after.plan!.behaviors.length)
  const finishedAt = 20_000
  runtime.frame([compileSpeechBehaviorPlan(later)], finishedAt)
  const completed = runtime.frame(
    [compileSpeechBehaviorPlan(later)],
    finishedAt + 100,
  )
  assert.ok(
    !completed.behaviors.some(
      (behavior) => behavior.id === accent.id && behavior.phase !== 'completed',
    ),
  )
})

test('adding audio evidence before a text beat never renames its behavior', () => {
  const input = {
    utteranceId: 'audio',
    startedAtMs: 100,
    durationMs: 2_000,
    accents: [{ textOffset: 12, offsetMs: 900, intensity: 0.9 }],
  }
  const initial = compileSpeechBehaviorPlan(input).behaviors[1]!
  const refined = compileSpeechBehaviorPlan({
    ...input,
    accents: [{ offsetMs: 100, intensity: 0.8 }, ...input.accents],
  })
  assert.deepEqual(
    refined.behaviors.find((behavior) => behavior.id === initial.id),
    initial,
  )
})

test('speech presence survives a provisional duration and recovers only when the source ends', () => {
  const plan = compileSpeechBehaviorPlan({
    utteranceId: 'slow-stream',
    startedAtMs: 1_000,
    durationMs: 650,
    accents: [],
  })
  const runtime = new HumanPerformanceRuntime()
  runtime.frame([plan], 1_000)
  const live = runtime.frame([plan], 10_000)
  assert.equal(live.behaviors[0]?.phase, 'holding')
  assert.equal(live.behaviors[0]?.endsAtMs, null)
  const stopped = runtime.frame([], 10_000)
  assert.equal(stopped.behaviors[0]?.phase, 'recovering')
  assert.ok(
    !runtime
      .frame([], 11_000)
      .behaviors.some((behavior) => behavior.phase === 'holding'),
  )
})
