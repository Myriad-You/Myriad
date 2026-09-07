import type { MeropePerformanceEventDetail } from './performanceEvents'
import type { PerformanceLifecycleTarget } from './performanceLifecycle'
import assert from 'node:assert/strict'
import test from 'node:test'
import { PerformanceLifecycleController } from './performanceLifecycle'

const performance = {
  phase: 'delivery' as const,
  moodRevision: 8,
  motionStyle: 'even' as const,
  plan: {
    baseline: {
      expression: 'warm' as const,
      posture: 'open' as const,
      motionEnergy: 1,
      attention: 1,
    },
    cues: [],
  },
}

test('forwards only semantic performance data and leaves event text speech-owned', () => {
  const played: (typeof performance)[] = []
  let stopped = 0
  const target: PerformanceLifecycleTarget = {
    applyPerformanceDirective: (value) => {
      played.push(value as typeof performance)
      return true
    },
    clearPerformanceDirective: () => {
      stopped += 1
    },
  }
  const controller = new PerformanceLifecycleController(target)

  controller.handle({ text: 'plain speech', source: 'reply' })
  assert.deepEqual(played, [])
  controller.handle({
    text: 'warm reply',
    source: 'reply',
    performance,
  })
  assert.deepEqual(played, [performance])
  assert.equal(stopped, 0)
})

test('stops the mounted rig when the lifecycle owner is disposed', () => {
  let stopped = 0
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: () => true,
    clearPerformanceDirective: () => {
      stopped += 1
    },
  })
  controller.dispose()
  assert.equal(stopped, 1)
})

test('a normal speech end keeps the landing plan; cancel dumps it', () => {
  let stopped = 0
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: () => true,
    clearPerformanceDirective: () => {
      stopped += 1
    },
  })
  controller.handle({
    text: 'reply',
    source: 'reply',
    messageId: 'message-1',
    performance,
  })
  controller.handleSpeech({
    phase: 'end',
    source: 'reply',
    messageId: 'message-1',
    utteranceId: 'utt-1',
  })
  assert.equal(stopped, 0)
  controller.handle({
    text: 'next',
    source: 'reply',
    messageId: 'message-2',
    performance,
  })
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'message-2',
  })
  assert.equal(stopped, 1)
})

test('cancels only the transient plan owned by the interrupted message', () => {
  const played: (typeof performance)[] = []
  let stopped = 0
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: (value) => {
      played.push(value as typeof performance)
      return true
    },
    clearPerformanceDirective: () => {
      stopped += 1
    },
  })
  controller.handle({
    text: 'reply',
    source: 'reply',
    messageId: 'message-1',
    performance,
  })
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'message-2',
  })
  assert.equal(stopped, 0)
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'message-1',
  })
  assert.equal(stopped, 1)
  assert.deepEqual(played, [performance])
})

test('drops a performance plan that arrives after its message was cancelled', () => {
  const played: (typeof performance)[] = []
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: (value) => {
      played.push(value as typeof performance)
      return true
    },
    clearPerformanceDirective: () => undefined,
  })
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'message-late',
  })
  controller.handle({
    text: 'late reply',
    source: 'reply',
    messageId: 'message-late',
    performance,
  })
  assert.deepEqual(played, [])
})

test('does not transfer cancellation ownership to a plan rejected by the rig', () => {
  let stopped = 0
  let accepted = true
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: () => accepted,
    clearPerformanceDirective: () => {
      stopped += 1
    },
  })
  controller.handle({
    text: 'current',
    source: 'reply',
    messageId: 'message-current',
    performance,
  })
  accepted = false
  controller.handle({
    text: 'stale',
    source: 'reply',
    messageId: 'message-stale',
    performance,
  })
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'message-stale',
  })
  assert.equal(stopped, 0)
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'message-current',
  })
  assert.equal(stopped, 1)
})

test('drops an older generation plan and ignores a replay of the active plan', async () => {
  const { setLiveMotionGeneration } = await import('./motion/liveGeneration')
  setLiveMotionGeneration(2)
  const played: (typeof performance)[] = []
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: (value) => {
      played.push(value as typeof performance)
      return true
    },
    clearPerformanceDirective: () => undefined,
  })
  controller.handle({
    text: 'stale',
    source: 'reply',
    messageId: 'message-old',
    generation: 1,
    performance,
  })
  assert.equal(played.length, 0)
  controller.handle({
    text: 'now',
    source: 'reply',
    messageId: 'message-now',
    generation: 2,
    motionIntentId: 'motion-1',
    performance,
  })
  controller.handle({
    text: 'now',
    source: 'reply',
    messageId: 'message-now',
    generation: 2,
    motionIntentId: 'motion-1',
    performance,
  })
  assert.equal(played.length, 1)
  setLiveMotionGeneration(0)
})

function recordingController() {
  const played: unknown[] = []
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: (value) => {
      played.push(value)
      return true
    },
    clearPerformanceDirective: () => undefined,
  })
  return { controller, played }
}

const firstPlan: MeropePerformanceEventDetail = {
  text: '',
  source: 'reply',
  messageId: 'delivery',
  performance,
}
const revisedPlan: MeropePerformanceEventDetail = {
  ...firstPlan,
  performance: {
    ...performance,
    plan: {
      ...performance.plan,
      baseline: { ...performance.plan.baseline, expression: 'sad' },
    },
  },
}

test('same cue count does not suppress a changed expression or body refinement', () => {
  const { controller, played } = recordingController()
  controller.handle(firstPlan)
  controller.handle(revisedPlan)
  assert.deepEqual(played, [firstPlan.performance, revisedPlan.performance])
})

test('a replay cannot undo a newer plan, even with a fresh transport intent id', () => {
  const { controller, played } = recordingController()
  controller.handle({ ...firstPlan, motionIntentId: 'first' })
  controller.handle({ ...revisedPlan, motionIntentId: 'second' })
  controller.handle({ ...firstPlan, motionIntentId: 'first' })
  controller.handle({ ...firstPlan, motionIntentId: 'reconnected' })
  assert.equal(played.length, 2)
})

test('normal speech end retains replay protection but permits a new message', () => {
  const { controller, played } = recordingController()
  controller.handle(firstPlan)
  controller.handleSpeech({
    phase: 'end',
    source: 'reply',
    messageId: 'delivery',
  })
  controller.handle({ ...firstPlan, motionIntentId: 'final-response' })
  controller.handle({ ...firstPlan, messageId: 'next-message' })
  assert.equal(played.length, 2)
})

test('a rejected plan can be retried and is remembered only after acceptance', () => {
  let attempts = 0
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: () => ++attempts > 1,
    clearPerformanceDirective: () => undefined,
  })
  controller.handle(firstPlan)
  controller.handle(firstPlan)
  controller.handle(firstPlan)
  assert.equal(attempts, 2)
})

test('identical plans in distinct live generations or sources are independent', async () => {
  const { setLiveMotionGeneration } = await import('./motion/liveGeneration')
  const { controller, played } = recordingController()
  try {
    setLiveMotionGeneration(1)
    controller.handle({ ...firstPlan, generation: 1 })
    setLiveMotionGeneration(2)
    controller.handle({ ...firstPlan, generation: 2 })
    controller.handle({ ...firstPlan, generation: 2, source: 'proactive' })
    assert.equal(played.length, 3)
  } finally {
    setLiveMotionGeneration(0)
  }
})

test('replay history is bounded and dispose clears it', () => {
  const { controller, played } = recordingController()
  controller.handle(firstPlan)
  for (let index = 0; index < 128; index += 1) {
    controller.handle({ ...firstPlan, messageId: `message-${index}` })
  }
  controller.handle(firstPlan)
  assert.equal(played.length, 130)
  controller.dispose()
  controller.handle(firstPlan)
  assert.equal(played.length, 131)
})

test('explicit preview and interaction intents may intentionally repeat the same pose', () => {
  for (const source of ['preview', 'interaction'] as const) {
    const { controller, played } = recordingController()
    controller.handle({ ...firstPlan, source, motionIntentId: 'first-click' })
    controller.handle({ ...firstPlan, source, motionIntentId: 'second-click' })
    controller.handle({ ...firstPlan, source, motionIntentId: 'second-click' })
    assert.equal(played.length, 2)
  }
})

test('cancel after text completion still clears the retained landing exactly once', () => {
  let stopped = 0
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: () => true,
    clearPerformanceDirective: () => {
      stopped += 1
    },
  })
  controller.handle(firstPlan)
  controller.handleSpeech({
    phase: 'end',
    source: 'reply',
    messageId: 'delivery',
  })
  assert.equal(stopped, 0)
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'delivery',
  })
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'delivery',
  })
  assert.equal(stopped, 1)
})

test('cancel of a completed old reply cannot clear a newer reply', () => {
  let stopped = 0
  const controller = new PerformanceLifecycleController({
    applyPerformanceDirective: () => true,
    clearPerformanceDirective: () => {
      stopped += 1
    },
  })
  controller.handle(firstPlan)
  controller.handleSpeech({
    phase: 'end',
    source: 'reply',
    messageId: 'delivery',
  })
  controller.handle({ ...firstPlan, messageId: 'new-reply' })
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'delivery',
  })
  assert.equal(stopped, 0)
  controller.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'new-reply',
  })
  assert.equal(stopped, 1)
})

test('final response without stream run metadata cannot replay the same message plan', () => {
  const { controller, played } = recordingController()
  controller.handle({ ...firstPlan, runId: 'run-1', motionIntentId: 'stream' })
  controller.handle({ ...firstPlan, motionIntentId: 'final' })
  assert.equal(played.length, 1)
})
