import type { MusicMotionSource, SingingFrame } from './musicSource'
import assert from 'node:assert/strict'
import test from 'node:test'
import { RigMotionCoordinator } from './coordinator'
import {
  createLiveMotionRuntime,
  createPreviewMotionRuntime,
  MotionRuntime,
} from './runtime'

function stubMusic(): MusicMotionSource & { listeners: number } {
  const listeners = new Set<(frame: SingingFrame) => void>()
  return {
    listeners: 0,
    subscribe(listener: (frame: SingingFrame) => void) {
      listeners.add(listener)
      this.listeners = listeners.size
      return () => {
        listeners.delete(listener)
        this.listeners = listeners.size
      }
    },
  } as MusicMotionSource & { listeners: number }
}

test('stopping speech drops queued viseme text so a remount does not replay it', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.speech.handleForTest({
    phase: 'start',
    messageId: 'message-1',
    utteranceId: 'stream-1',
    source: 'reply',
  })
  runtime.speech.handleForTest({
    phase: 'chunk',
    messageId: 'message-1',
    utteranceId: 'stream-1',
    source: 'reply',
    text: '你好',
  })
  assert.ok((runtime.frame().speech?.queuedText.length ?? 0) > 0)
  runtime.speech.stop()
  assert.deepEqual(runtime.frame().speech?.queuedText ?? [], [])
  release()
})

test('two consumers see the same speech intent; one unmount does not stop the source', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const releaseA = runtime.retain()
  const releaseB = runtime.retain()
  runtime.speech.start()
  runtime.speech.handleForTest({
    phase: 'start',
    messageId: 'message-1',
    utteranceId: 'stream-1',
    source: 'reply',
  })
  const frames: string[] = []
  const unsubA = runtime.subscribe((frame) => {
    frames.push(`a:${frame.snapshot.owners.mouth}`)
  })
  const unsubB = runtime.subscribe((frame) => {
    frames.push(`b:${frame.snapshot.owners.mouth}`)
  })
  assert.equal(runtime.frame().snapshot.owners.mouth, 'speech')
  assert.equal(runtime.frame().snapshot.owners.headBody, 'coSpeech')
  unsubA()
  releaseA()
  assert.equal(runtime.frame().snapshot.owners.mouth, 'speech')
  unsubB()
  releaseB()
  assert.equal(runtime.frame().snapshot.owners.mouth, 'idle')
})

test('visible face consumers merge capabilities and select one mood authority', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const widget = runtime.attachLiveFaceConsumer({
    ready: true,
    mood: 35,
    arousal: 72,
    activity: 'idle',
    capabilities: ['blink', 'head-body'],
    priority: 1,
  })
  const panel = runtime.attachLiveFaceConsumer({
    ready: false,
    mood: 80,
    arousal: 40,
    activity: 'thinking',
    capabilities: ['speech-viseme'],
    priority: 2,
  })

  assert.equal(runtime.summaryFacts().faceVisible, true)
  assert.deepEqual(runtime.summaryFacts().capabilities, ['blink', 'head-body'])
  assert.equal(runtime.frame().mood?.mood, 35)

  panel.update({
    ready: true,
    mood: 80,
    arousal: 40,
    activity: 'thinking',
    capabilities: ['speech-viseme'],
    priority: 2,
  })
  assert.deepEqual(runtime.summaryFacts().capabilities, [
    'blink',
    'head-body',
    'speech-viseme',
  ])
  assert.equal(runtime.frame().mood?.mood, 80)
  assert.equal(runtime.frame().mood?.activity, 'thinking')

  panel.release()
  widget.release()
  assert.equal(runtime.summaryFacts().faceVisible, true)
  release()
  assert.equal(runtime.summaryFacts().faceVisible, false)
})

test('preview runtime ticks timed leases without a music sampler', async () => {
  const runtime = createPreviewMotionRuntime()
  const release = runtime.retain()
  const now = performance.now()
  runtime.coordinator.claim('performance', ['headBody'], {
    nowMs: now,
    ttlMs: 40,
  })
  assert.equal(runtime.coordinator.owner('headBody', now), 'performance')
  await new Promise((resolve) => setTimeout(resolve, 80))
  const after = runtime.coordinator.owner('headBody')
  assert.notEqual(after, 'performance')
  assert.ok(after === 'idle' || after === 'ambient')
  release()
})

test('live runtime attaches music without inventing semantic idle reactions', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('music', ['mouth', 'headBody'], { nowMs: 0 })
  const music = stubMusic()
  const live = createLiveMotionRuntime(coordinator, music)
  const release = live.retain()
  assert.equal(music.listeners, 1)
  assert.equal(live.frame().snapshot.owners.expression, 'idle')
  assert.equal(live.frame().snapshot.owners.mouth, 'music')
  assert.equal(live.frame().snapshot.owners.headBody, 'music')
  release()
  assert.equal(music.listeners, 0)

  const preview = createPreviewMotionRuntime()
  const previewRelease = preview.retain()
  assert.equal(preview.frame().snapshot.owners.expression, 'idle')
  previewRelease()
})

test('mood claims expression below co-speech', () => {
  const coordinator = new RigMotionCoordinator()
  const runtime = new MotionRuntime(coordinator)
  const release = runtime.retain()
  runtime.mood.set(80, 'idle')
  assert.equal(runtime.frame().snapshot.owners.expression, 'mood')
  runtime.speech.handleForTest({
    phase: 'start',
    messageId: 'message-1',
    utteranceId: 'stream-1',
    source: 'reply',
  })
  assert.equal(runtime.frame().snapshot.owners.expression, 'coSpeech')
  assert.equal(runtime.frame().mood?.mood, 80)
  release()
})

test('realizer feedback reaches the behavior lifecycle', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.performance.handleForTest({
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: {
      cues: [
        {
          intent: 'respond',
          atMs: 0,
          intensity: 1,
          tempo: 1,
          fadeInMs: 80,
          fadeOutMs: 120,
          interrupt: 'replace',
        },
      ],
    },
  })
  const intent = runtime.frame().performance
  assert.ok(intent?.motionIntentId)
  const behaviorId = intent?.behaviorPlan?.behaviors[0]?.id
  assert.ok(behaviorId)
  runtime.reportBehaviorRealizer(
    runtime.frame().behaviorPlan!.id,
    behaviorId!,
    'rejected',
  )
  assert.ok(
    runtime
      .frame()
      .performance?.behaviors?.some(
        (behavior) => behavior.phase === 'rejected',
      ),
  )
  release()
})

test('a same-strength refinement updates bearing without replaying the reaction', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const first = {
    phase: 'delivery' as const,
    moodRevision: 3,
    motionStyle: 'even' as const,
    plan: {
      baseline: {
        expression: 'steady' as const,
        posture: 'neutral' as const,
        motionEnergy: 0.8,
        attention: 0.7,
      },
      cues: [
        {
          intent: 'respond' as const,
          atMs: 0,
          intensity: 1,
          tempo: 1,
          fadeInMs: 80,
          fadeOutMs: 120,
          interrupt: 'if-lower' as const,
        },
      ],
    },
  }
  runtime.performance.handleForTest(first)
  const planId = runtime.frame().performance?.behaviorPlan?.id
  runtime.performance.handleForTest({
    ...first,
    plan: {
      ...first.plan,
      baseline: { ...first.plan.baseline, expression: 'warm' as const },
    },
  })
  const refined = runtime.frame()
  assert.equal(refined.performance?.behaviorPlan?.id, planId)
  assert.equal(refined.performance?.directive?.plan.cues.length, 0)
  assert.equal(refined.bearing?.expression, 'warm')
  release()
})

test('each round motion style reaches the shared behavior quality layer', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const directive = {
    phase: 'delivery' as const,
    moodRevision: 1,
    motionStyle: 'open' as const,
    plan: {
      cues: [
        {
          intent: 'respond' as const,
          atMs: 0,
          intensity: 1,
          tempo: 1,
          fadeInMs: 80,
          fadeOutMs: 120,
          interrupt: 'replace' as const,
        },
      ],
    },
  }
  runtime.performance.handleForTest(directive)
  const openExtent = runtime.frame().behaviorPlan?.behaviors[0]?.quality?.extent
  assert.equal(runtime.summaryFacts().motionStyle, 'open')

  runtime.performance.handleForTest({
    ...directive,
    moodRevision: 2,
    motionStyle: 'restrained',
  })
  const restrainedExtent =
    runtime.frame().behaviorPlan?.behaviors[0]?.quality?.extent
  assert.equal(runtime.summaryFacts().motionStyle, 'restrained')
  assert.ok(openExtent != null && restrainedExtent != null)
  assert.ok(openExtent > restrainedExtent)
  release()
})

test('irritation wears the tense standing face before any performance round', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.mood.set(30, 'idle', 70)
  assert.equal(runtime.frame().bearing?.expression, 'tense')
  runtime.mood.set(30, 'idle', 40)
  assert.equal(runtime.frame().bearing?.expression, 'subdued')
  release()
})

test('a mood-band change drops a stale performance bearing', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.mood.set(70, 'idle', 48)
  runtime.performance.handleForTest({
    phase: 'mood',
    moodRevision: 1,
    motionStyle: 'even',
    plan: {
      baseline: {
        expression: 'warm',
        posture: 'open',
        motionEnergy: 1,
        attention: 0.5,
      },
      cues: [],
    },
  })
  assert.equal(runtime.frame().bearing?.expression, 'warm')
  runtime.mood.set(30, 'idle', 70)
  assert.equal(runtime.frame().bearing?.expression, 'tense')
  release()
})
