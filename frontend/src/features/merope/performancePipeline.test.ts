import type { PerformanceDirective } from '../../services/agent/types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { AgentFaceChannel } from './agentFaceChannel'
import { RigMotionCoordinator } from './motion/coordinator'
import { setLiveMotionGeneration } from './motion/liveGeneration'
import { MotionRuntime } from './motion/runtime'
import { meropePerformanceEventDetail } from './performanceEvents'
import {
  beginTurnTrace,
  resetTurnTraceForTest,
  snapshotTurnTrace,
} from './turnTrace'

const delivery: PerformanceDirective = {
  phase: 'delivery',
  moodRevision: 10,
  motionStyle: 'even',
  plan: {
    baseline: {
      expression: 'steady',
      posture: 'neutral',
      motionEnergy: 1,
      attention: 1,
    },
    cues: [
      {
        intent: 'respond',
        atMs: 0,
        intensity: 1,
        tempo: 1,
        fadeInMs: 100,
        fadeOutMs: 400,
        interrupt: 'if-lower',
      },
    ],
  },
}

function pipeline(runtime: MotionRuntime) {
  return new AgentFaceChannel({
    performance: (value) => {
      const event = meropePerformanceEventDetail(value)
      if (event) runtime.performance.handle(event)
    },
    speech: (event) => runtime.performance.handleSpeech(event),
    utterance: () => undefined,
    state: () => undefined,
  })
}

test('channel → event validation → lifecycle → scheduler preserves a delivery beat at landing', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const channel = pipeline(runtime)
  try {
    channel.deliver({ messageId: 'reply', performance: delivery })
    const before = runtime.frame().performance!
    assert.ok(
      before.behaviorPlan?.behaviors.some(
        (behavior) => behavior.form.id === 'respond',
      ),
    )
    channel.deliver({
      messageId: 'reply',
      performance: { ...delivery, plan: { ...delivery.plan, cues: [] } },
    })
    const after = runtime.frame().performance!
    assert.equal(after.motionIntentId, before.motionIntentId)
    assert.equal(after.startedAtMs, before.startedAtMs)
    assert.equal(after.behaviorPlan, before.behaviorPlan)
    assert.equal(after.directive?.plan.cues.length, 0)
  } finally {
    release()
  }
})

test('same-size model refinement reaches the scheduler; replay after speech end cannot roll it back', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const channel = pipeline(runtime)
  try {
    channel.deliver({ messageId: 'reply', performance: delivery })
    const refined: PerformanceDirective = {
      ...delivery,
      plan: {
        baseline: { ...delivery.plan.baseline!, expression: 'warm' },
        cues: [
          {
            ...delivery.plan.cues[0]!,
            intent: 'delight',
            interrupt: 'replace',
          },
        ],
      },
    }
    channel.deliver({ messageId: 'reply', performance: refined })
    const before = runtime.frame()
    assert.equal(before.bearing?.expression, 'warm')
    assert.ok(
      before.behaviors.some((behavior) => behavior.form.id === 'delight'),
    )
    assert.ok(
      before.behaviors.some(
        (behavior) =>
          behavior.form.id === 'respond' && behavior.phase === 'recovering',
      ),
    )
    const speech = channel.openReply('reply')
    speech.chunk('a completed spoken reply')
    speech.end()
    // deliver() assigns a new transport intent id to this replay.
    channel.deliver({ messageId: 'reply', performance: delivery })
    const after = runtime.frame()
    assert.equal(after.bearing?.expression, 'warm')
    assert.equal(
      after.performance?.motionIntentId,
      before.performance?.motionIntentId,
    )
    assert.equal(
      after.performance?.behaviorPlan,
      before.performance?.behaviorPlan,
    )
  } finally {
    release()
  }
})

test('interrupting a text-complete reply releases its body plan and standing bearing', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const channel = pipeline(runtime)
  try {
    channel.deliver({ messageId: 'reply', performance: delivery })
    const speech = channel.openReply('reply')
    speech.chunk('text finished, but playback may still be in progress')
    speech.end()
    assert.ok(runtime.frame().performance?.behaviorPlan)
    assert.ok(runtime.performance.currentBearing())
    channel.cancel('reply')
    assert.equal(runtime.performance.current().behaviorPlan, null)
    assert.equal(runtime.performance.currentBearing(), null)
    channel.deliver({ messageId: 'reply', performance: delivery })
    assert.equal(runtime.performance.current().behaviorPlan, null)
  } finally {
    release()
  }
})

test('same-run strengthening keeps the beat identity; next generation starts a fresh beat with recovery', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const channel = pipeline(runtime)
  try {
    setLiveMotionGeneration(1)
    channel.setGeneration(1)
    channel.deliver({
      messageId: 'reply-1',
      runId: 'run-1',
      performance: delivery,
    })
    const original = runtime.frame().performance!.behaviorPlan!.behaviors[0]!
    channel.deliver({
      messageId: 'reply-1',
      runId: 'run-1',
      performance: {
        ...delivery,
        moodRevision: 11,
        plan: {
          ...delivery.plan,
          cues: [{ ...delivery.plan.cues[0]!, intensity: 1.3 }],
        },
      },
    })
    const refined = runtime.frame().performance!.behaviorPlan!.behaviors[0]!
    assert.equal(refined.id, original.id)
    assert.equal(refined.intensity, 1.3)
    setLiveMotionGeneration(2)
    channel.setGeneration(2)
    channel.deliver({
      messageId: 'reply-2',
      runId: 'run-2',
      performance: delivery,
    })
    const frame = runtime.frame()
    const next = frame.performance!.behaviorPlan!.behaviors[0]!
    assert.notEqual(next.id, original.id)
    assert.ok(
      frame.behaviors.some(
        (item) => item.id === original.id && item.phase === 'recovering',
      ),
    )
    assert.ok(
      frame.behaviors.some(
        (item) => item.id === next.id && item.phase !== 'recovering',
      ),
    )
  } finally {
    release()
    setLiveMotionGeneration(0)
  }
})

test('decision trace distinguishes selected, habituated and resource-busy without recording reply text', () => {
  resetTurnTraceForTest()
  beginTurnTrace('trace-reply')
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const channel = pipeline(runtime)
  try {
    channel.deliver({
      messageId: 'trace-reply',
      runId: 'trace-run',
      text: 'private reply text',
      performance: delivery,
    })
    runtime.frame()
    channel.deliver({
      messageId: 'trace-reply',
      runId: 'trace-run',
      performance: { ...delivery, moodRevision: 11 },
    })
    channel.deliver({
      messageId: 'other-reply',
      runId: 'other-run',
      performance: delivery,
    })
    const marks = snapshotTurnTrace().marks.filter(
      (mark) => mark.span === 'performance_decision',
    )
    assert.deepEqual(
      marks.map((mark) => mark.extra?.reason),
      ['selected', 'habituated', 'resource-busy'],
    )
    assert.equal(marks[0]?.extra?.runId, 'trace-run')
    assert.doesNotMatch(JSON.stringify(marks), /private reply text/)
  } finally {
    release()
    resetTurnTraceForTest()
  }
})
