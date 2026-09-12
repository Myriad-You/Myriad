import type { PerformanceDirective } from '../../../services/agent/types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { cueDurationMs } from '../anime25drig/performanceMotion'
import { RigMotionCoordinator } from './coordinator'
import {
  performanceLeaseWindows,
  PerformanceMotionLeases,
} from './performanceLeases'
import { allowsCoSpeechExpression } from './policy'

function cue(
  intent: PerformanceDirective['plan']['cues'][number]['intent'],
  atMs = 0,
): PerformanceDirective['plan']['cues'][number] {
  return {
    intent,
    atMs,
    intensity: 1,
    tempo: 1,
    fadeInMs: 80,
    fadeOutMs: 120,
    interrupt: 'replace',
  }
}

function directive(
  overrides: Partial<PerformanceDirective['plan']> = {},
): PerformanceDirective {
  return {
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: {
      baseline: {
        expression: 'warm',
        posture: 'neutral',
        motionEnergy: 1,
        attention: 1,
      },
      cues: [],
      ...overrides,
    },
  }
}

test('persistent bearing never acquires a transient lease', () => {
  const windows = performanceLeaseWindows(
    directive({
      baseline: {
        expression: 'warm',
        posture: 'open',
        motionEnergy: 1,
        attention: 1,
      },
    }),
    0,
  )
  assert.equal(windows.planUntilMs, 0)
  assert.equal(windows.headBodyCueUntilMs, null)
  assert.equal(windows.expressionCueUntilMs, null)
  assert.equal(windows.gazeCueUntilMs, null)
})

test('a greet cue times the head/body lease to its actual duration', () => {
  const greet = cue('greet')
  const windows = performanceLeaseWindows(directive({ cues: [greet] }), 0)
  assert.equal(windows.headBodyCueUntilMs, cueDurationMs(greet))
  assert.equal(windows.expressionCueUntilMs, cueDurationMs(greet))
  assert.equal(windows.planUntilMs, cueDurationMs(greet))
})

test('think leases the head channel that its head tilt writes', () => {
  const windows = performanceLeaseWindows(
    directive({ cues: [cue('think')] }),
    0,
  )
  assert.ok((windows.expressionCueUntilMs ?? 0) > 0)
  assert.equal(windows.headBodyCueUntilMs, windows.expressionCueUntilMs)
})

test('body cue lease expires and music groove resumes without a synthetic tail', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('music', ['mouth', 'headBody'], { nowMs: 0 })
  const leases = new PerformanceMotionLeases(coordinator)
  const greet = cue('greet')
  leases.apply(directive({ cues: [greet] }), 0)
  assert.equal(coordinator.owner('headBody', 0), 'performance')
  assert.equal(coordinator.owner('expression', 0), 'performance')
  assert.equal(coordinator.owner('mouth', 0), 'music')
  const end = cueDurationMs(greet)
  coordinator.tick(end)
  assert.equal(coordinator.owner('headBody', end), 'music')
  assert.equal(coordinator.owner('expression', end), 'idle')
})

test('a directed head tilt temporarily takes the coarse channel from music', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('music', ['headBody'], { nowMs: 0 })
  const leases = new PerformanceMotionLeases(coordinator)
  const think = cue('think')
  leases.apply(
    directive({
      baseline: {
        expression: 'warm',
        posture: 'open',
        motionEnergy: 1,
        attention: 1,
      },
      cues: [think],
    }),
    0,
  )
  assert.equal(coordinator.owner('headBody', 0), 'performance')
  const end = cueDurationMs(think)
  coordinator.tick(end)
  assert.equal(coordinator.owner('headBody', end), 'music')
})

test('a baseline-only plan does not exclusive-claim expression', () => {
  const coordinator = new RigMotionCoordinator()
  const leases = new PerformanceMotionLeases(coordinator)
  leases.apply(directive(), 0)
  assert.equal(coordinator.owner('expression', 0), 'idle')
  assert.equal(
    allowsCoSpeechExpression(coordinator.owner('expression', 0)),
    true,
  )
})

test('cue lease expiry returns expression without a cancel event', () => {
  const coordinator = new RigMotionCoordinator()
  const leases = new PerformanceMotionLeases(coordinator)
  const think = cue('think')
  leases.apply(directive({ cues: [think] }), 0)
  assert.equal(coordinator.owner('expression', 0), 'performance')
  const settled = cueDurationMs(think)
  coordinator.tick(settled)
  assert.equal(coordinator.owner('expression', settled), 'idle')
  assert.equal(
    allowsCoSpeechExpression(coordinator.owner('expression', settled)),
    true,
  )
})

test('cancel or unmount releases every performance lease, not someone else', () => {
  const coordinator = new RigMotionCoordinator()
  const panel = new PerformanceMotionLeases(coordinator)
  const home = new PerformanceMotionLeases(coordinator)
  panel.apply(directive({ cues: [cue('think')] }), 0)
  home.apply(directive({ cues: [cue('greet')] }), 0)
  assert.equal(coordinator.owner('expression', 0), 'performance')
  assert.equal(coordinator.owner('headBody', 0), 'performance')
  home.releaseAll()
  assert.equal(coordinator.owner('expression', 0), 'performance')
  assert.equal(coordinator.owner('headBody', 0), 'performance')
  panel.releaseAll()
  assert.equal(coordinator.owner('expression', 0), 'idle')
  assert.equal(coordinator.owner('headBody', 0), 'idle')
})

// Missing either class lets ambient drift keep full weight and pull against the directed look.
test('a cue that moves the eyes takes the gaze lease for as long as it plays', () => {
  for (const intent of [
    'think',
    'speechless',
    'maniac',
    'lovestruck',
  ] as const) {
    const windows = performanceLeaseWindows(
      directive({ cues: [cue(intent)] }),
      1_000,
    )
    assert.ok(windows.gazeCueUntilMs !== null, intent)
    assert.ok(windows.gazeCueUntilMs! > 1_000, intent)
    assert.equal(windows.gazeCueUntilMs, windows.expressionCueUntilMs, intent)
  }

  const facial = performanceLeaseWindows(
    directive({ cues: [cue('respond')] }),
    1_000,
  )
  assert.equal(facial.gazeCueUntilMs, null)
})
