import type {
  PerformanceCue,
  PerformanceDirective,
} from '../../../services/agent/types'
import type { BehaviorPlan } from '../motion/behavior'
import assert from 'node:assert/strict'
import test from 'node:test'
import { compilePerformanceBehaviorPlan } from '../motion/performanceBehaviorPlan'
import { compileSpeechBehaviorPlan } from '../motion/speechBehaviorPlan'
import { realizeAnime25DBehaviorPlan } from './behaviorRealizer'
import { cueVisualEnvelope } from './performanceMotion'

const directive: PerformanceDirective = {
  phase: 'delivery',
  moodRevision: 2,
  motionStyle: 'even',
  plan: {
    cues: [
      {
        intent: 'emphasize',
        atMs: 200,
        intensity: 1.1,
        tempo: 1.2,
        fadeInMs: 100,
        fadeOutMs: 180,
        interrupt: 'queue',
      },
    ],
  },
}

test('a performance behavior realizes as a unit, not back into a cue', () => {
  const plan = compilePerformanceBehaviorPlan(directive, 1_000, 'plan-a')
  const realized = realizeAnime25DBehaviorPlan(plan, 900)
  assert.equal(realized.units.length, 1)
  const unit = realized.units[0]!
  assert.equal(unit.family, 'performance')
  assert.equal(unit.form, 'emphasize')
  // The unit keeps the pegs the scheduler resolved. Nothing repacks them into
  // a cue's three durations, so nothing has to guess them back out.
  const behavior = plan.behaviors[0]!
  const at = (id: string): number =>
    plan.pegs.find((peg) => peg.id === id)!.atMs
  assert.equal(unit.timing.startMs, at(behavior.timing.start))
  assert.equal(unit.timing.strokePeakMs, at(behavior.timing.strokePeak))
  assert.equal(unit.timing.strokeEndMs, at(behavior.timing.strokeEnd))
  assert.equal(unit.timing.relaxMs, at(behavior.timing.relax!))
  assert.equal(unit.timing.endMs, at(behavior.timing.end!))
  assert.deepEqual(realized.reports, [
    {
      behaviorId: 'plan-a:cue-emphasize-0',
      result: 'accepted',
      atMs: 900,
    },
  ])
})

test('unsupported forms are rejected per behavior while supported peers survive', () => {
  const supported = compilePerformanceBehaviorPlan(directive, 0, 'plan-b')
  const plan: BehaviorPlan = {
    ...supported,
    behaviors: [
      ...supported.behaviors,
      {
        ...supported.behaviors[0]!,
        id: 'plan-b:unsupported',
        form: { family: 'future-locomotion', id: 'step-left' },
      },
    ],
  }
  const realized = realizeAnime25DBehaviorPlan(plan, 10)
  assert.equal(realized.units.length, 1)
  assert.equal(realized.reports[1]?.behaviorId, 'plan-b:unsupported')
  assert.equal(realized.reports[1]?.result, 'rejected')
  assert.equal(realized.reports[1]?.reason, 'unsupported-form')
})

test('co-speech presence and accents realize as procedural motion units', () => {
  const plan = compileSpeechBehaviorPlan({
    utteranceId: 'utterance-a',
    startedAtMs: 1_000,
    durationMs: 1_200,
    accents: [{ offsetMs: 500, intensity: 0.84 }],
  })
  const realized = realizeAnime25DBehaviorPlan(plan, 900)
  assert.deepEqual(
    realized.units.map((unit) => unit.form),
    ['presence', 'accent'],
  )
  assert.ok(realized.units.every((unit) => unit.family === 'co-speech'))
  assert.ok(realized.reports.every((report) => report.result === 'accepted'))
})

test('music groove realizes through the same registry', () => {
  const speech = compileSpeechBehaviorPlan({
    utteranceId: 'music-template',
    startedAtMs: 0,
    durationMs: 1_000,
    accents: [],
  })
  const plan: BehaviorPlan = {
    ...speech,
    behaviors: [
      {
        ...speech.behaviors[0]!,
        id: 'music:groove',
        function: 'entrain',
        kind: 'rhythmic',
        source: 'music',
        form: { family: 'music', id: 'groove' },
        timing: { ...speech.behaviors[0]!.timing, relax: null, end: null },
      },
    ],
  }
  const realized = realizeAnime25DBehaviorPlan(plan, 0)
  assert.equal(realized.units[0]?.family, 'music')
  assert.equal(realized.reports[0]?.result, 'accepted')
})

test('missing canonical timing pegs are rejected instead of approximated', () => {
  const plan = compilePerformanceBehaviorPlan(directive, 0, 'invalid')
  plan.pegs = plan.pegs.filter(
    (peg) => peg.id !== plan.behaviors[0]!.timing.strokeStart,
  )
  const realized = realizeAnime25DBehaviorPlan(plan, 10)
  assert.equal(realized.units.length, 0)
  assert.equal(realized.reports[0]?.result, 'rejected')
  assert.equal(realized.reports[0]?.reason, 'invalid-timing')
})

test('a scheduled cue carries the resolved hold span instead of a tempo guess', () => {
  const scheduled: PerformanceCue = {
    intent: 'greet',
    atMs: 0,
    intensity: 1,
    tempo: 1.6,
    fadeInMs: 120,
    holdMs: 900,
    fadeOutMs: 200,
    interrupt: 'replace',
  }
  const { holdMs: _dropped, ...unscheduled } = scheduled
  // 0.72 / 1.6 = 0.45s is what the heuristic would have produced.
  assert.equal(cueVisualEnvelope(scheduled).hold, 0.9)
  assert.equal(
    Math.round(cueVisualEnvelope(unscheduled).hold * 100) / 100,
    0.45,
  )
})
