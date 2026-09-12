import type {
  PerformanceCue,
  PerformanceDirective,
} from '../../../services/agent/types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { compilePerformanceBehaviorPlan } from './performanceBehaviorPlan'

function directive(): PerformanceDirective {
  return {
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: {
      baseline: {
        expression: 'warm',
        posture: 'open',
        motionEnergy: 0.8,
        attention: 0.9,
      },
      cues: [
        {
          intent: 'emphasize',
          atMs: 200,
          intensity: 1,
          tempo: 1,
          fadeInMs: 100,
          fadeOutMs: 160,
          interrupt: 'replace',
        },
      ],
    },
  }
}

test('compiles only transient functions and monotonic time pegs', () => {
  const plan = compilePerformanceBehaviorPlan(directive(), 1_000, 'plan-a')
  const cue = plan.behaviors.find(
    (behavior) => behavior.form.id === 'emphasize',
  )
  assert.equal(cue?.function, 'emphasize')
  assert.deepEqual(cue?.channels, ['expression', 'headBody'])
  assert.deepEqual(cue?.resources, [
    'face.expression',
    'body.head',
    'body.torso',
  ])
  assert.equal(plan.originMs, 1_000)
  assert.deepEqual(plan.metadata, { phase: 'delivery', moodRevision: 1 })
  assert.equal(
    plan.behaviors.some(
      (behavior) => behavior.form.family === 'performance-baseline',
    ),
    false,
  )
  const times = plan.pegs
    .filter((peg) => peg.id.startsWith('plan-a:cue-emphasize-0'))
    .map((peg) => peg.atMs)
  assert.deepEqual(
    times,
    times.toSorted((left, right) => left - right),
  )
})

test('declares head and torso resources for a readable acknowledgement', () => {
  const value = directive()
  value.plan.baseline = undefined
  value.plan.cues[0] = { ...value.plan.cues[0]!, intent: 'respond' }
  const plan = compilePerformanceBehaviorPlan(value, 0, 'plan-b')
  const cue = plan.behaviors.find((behavior) => behavior.form.id === 'respond')
  assert.deepEqual(cue?.channels, ['expression', 'headBody'])
  assert.deepEqual(cue?.resources, [
    'face.expression',
    'body.head',
    'body.torso',
  ])
})

test('recompiling a realized cue does not shrink it', () => {
  const cue: PerformanceCue = {
    intent: 'greet',
    atMs: 0,
    intensity: 1,
    tempo: 1,
    fadeInMs: 160,
    fadeOutMs: 240,
    interrupt: 'replace',
  }
  const directive: PerformanceDirective = {
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: { cues: [cue] },
  }
  const first = compilePerformanceBehaviorPlan(directive, 0, 'plan-1')
  const holdMs = Math.round(
    pegAt(first, 'plan-1:cue-greet-0:relax') - pegAt(first, 'plan-1:cue-greet-0:stroke-end'),
  )
  const second = compilePerformanceBehaviorPlan(
    { ...directive, plan: { cues: [{ ...cue, holdMs }] } },
    0,
    'plan-2',
  )
  assert.equal(
    pegAt(second, 'plan-2:cue-greet-0:end'),
    pegAt(first, 'plan-1:cue-greet-0:end'),
  )
})

test('a repeated beat keeps its identity so a refinement can continue it', () => {
  const floor = compilePerformanceBehaviorPlan(directive(), 1_000, 'performance')
  const later = directive()
  later.plan.cues[0] = { ...later.plan.cues[0]!, atMs: 640, intensity: 1.3 }
  const refinement = compilePerformanceBehaviorPlan(later, 1_000, 'performance')
  assert.deepEqual(
    refinement.behaviors.map((behavior) => behavior.id),
    floor.behaviors.map((behavior) => behavior.id),
  )
  assert.notEqual(
    pegAt(refinement, 'performance:cue-emphasize-0:start'),
    pegAt(floor, 'performance:cue-emphasize-0:start'),
  )
  assert.equal(refinement.behaviors[0]?.intensity, 1.3)
})

test('a beat the refinement did not choose keeps a distinct identity', () => {
  const floor = compilePerformanceBehaviorPlan(directive(), 0, 'performance')
  const other = directive()
  other.plan.cues[0] = { ...other.plan.cues[0]!, intent: 'delight' }
  const refinement = compilePerformanceBehaviorPlan(other, 0, 'performance')
  assert.notDeepEqual(
    refinement.behaviors.map((behavior) => behavior.id),
    floor.behaviors.map((behavior) => behavior.id),
  )
})

test('the same intent twice in one plan stays two behaviors', () => {
  const twice = directive()
  const first = twice.plan.cues[0]!
  twice.plan.cues = [first, { ...first, atMs: 900 }]
  const plan = compilePerformanceBehaviorPlan(twice, 0, 'performance')
  assert.deepEqual(plan.behaviors.map((behavior) => behavior.id), [
    'performance:cue-emphasize-0',
    'performance:cue-emphasize-1',
  ])
})

function pegAt(plan: ReturnType<typeof compilePerformanceBehaviorPlan>, id: string): number {
  const found = plan.pegs.find((peg) => peg.id === id)
  assert.ok(found, `${id} missing`)
  return found.atMs
}
