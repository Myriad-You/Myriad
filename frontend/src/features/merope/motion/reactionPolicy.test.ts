import type { PerformanceDirective } from '../../../services/agent/types'
import type { BehaviorSnapshot } from './behavior'
import assert from 'node:assert/strict'
import test from 'node:test'
import { HumanReactionPolicy } from './reactionPolicy'

function directive(
  intent: PerformanceDirective['plan']['cues'][number]['intent'],
  interrupt: 'replace' | 'queue' | 'if-lower' = 'if-lower',
  intensity = 1,
): PerformanceDirective {
  return {
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: {
      cues: [
        {
          intent,
          atMs: 0,
          intensity,
          tempo: 1,
          fadeInMs: 80,
          fadeOutMs: 120,
          interrupt,
        },
      ],
    },
  }
}

function activeMusic(): BehaviorSnapshot {
  return {
    id: 'music',
    function: 'entrain',
    kind: 'rhythmic',
    source: 'music',
    resources: ['body.head', 'body.torso'],
    channels: ['headBody'],
    form: { family: 'music', id: 'listen' },
    phase: 'holding',
    startedAtMs: 0,
    strokeAtMs: 100,
    relaxAtMs: null,
    endsAtMs: null,
    remainingMs: null,
  }
}

test('habituation removes a duplicate refinement but permits a stronger reaction', () => {
  const policy = new HumanReactionPolicy()
  assert.equal(
    policy.select(directive('respond'), [], 0).directive.plan.cues.length,
    1,
  )
  const duplicate = policy.select(directive('respond'), [], 300)
  assert.equal(duplicate.directive.plan.cues.length, 0)
  assert.equal(duplicate.decisions[0]?.reason, 'habituated')
  assert.equal(
    policy.select(directive('respond', 'replace', 1.3), [], 320).directive.plan
      .cues.length,
    1,
  )
})

test('if-lower follows channel priority instead of treating music specially', () => {
  const policy = new HumanReactionPolicy()
  const body = policy.select(directive('greet', 'replace'), [activeMusic()], 0)
  assert.equal(body.directive.plan.cues.length, 1)
  assert.equal(body.decisions[0]?.reason, 'selected')
  const defer = policy.select(
    directive('question', 'if-lower'),
    [activeMusic()],
    2_000,
  )
  assert.equal(defer.directive.plan.cues.length, 1)
  assert.equal(defer.decisions[0]?.reason, 'selected')
  const samePriority = policy.select(
    directive('notify', 'if-lower'),
    [{ ...activeMusic(), source: 'performance' }],
    4_000,
  )
  assert.equal(samePriority.directive.plan.cues.length, 0)
  assert.equal(samePriority.decisions[0]?.reason, 'resource-busy')
  const acknowledgement = policy.select(
    directive('respond', 'replace'),
    [activeMusic()],
    0,
  )
  assert.equal(acknowledgement.directive.plan.cues.length, 1)
})

test('queued reactions move behind a conflicting live behavior', () => {
  const policy = new HumanReactionPolicy()
  const active = {
    ...activeMusic(),
    source: 'performance' as const,
    remainingMs: 600,
  }
  const selected = policy.select(directive('greet', 'queue'), [active], 0)
  assert.equal(selected.directive.plan.cues[0]?.atMs, 690)
  assert.equal(selected.decisions[0]?.reason, 'retimed')
})

test('timeline replay respects overrides and avoids refinement double-takes', () => {
  const policy = new HumanReactionPolicy()
  const replay = [
    policy.select(directive('respond'), [], 0),
    policy.select(directive('respond'), [], 180),
    policy.select(directive('greet', 'replace'), [activeMusic()], 500),
    policy.select(directive('think', 'replace'), [activeMusic()], 700),
  ]
  assert.deepEqual(
    replay.map((frame) => frame.directive.plan.cues.map((cue) => cue.intent)),
    [['respond'], [], ['greet'], ['think']],
  )
  assert.deepEqual(
    policy.timeline().map((event) => event.intent),
    ['respond', 'greet', 'think'],
  )
})
