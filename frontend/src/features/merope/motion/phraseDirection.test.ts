import assert from 'node:assert/strict'
import test from 'node:test'
import { sanitizePerformanceDirective } from '../performanceEvents'
import { predictTextProsody } from '../speech/textProsody'
import { meropeSpeechEventDetail } from '../speechEvents'
import { RigMotionCoordinator } from './coordinator'
import { liveMotionGeneration, setLiveMotionGeneration } from './liveGeneration'
import { MotionRuntime } from './runtime'

const text = '你真的这么想吗？'
const directive = sanitizePerformanceDirective({
  phase: 'delivery',
  moodRevision: 999,
  motionStyle: 'even',
  plan: {
    baseline: {
      expression: 'warm',
      posture: 'open',
      motionEnergy: 1,
      attention: 0.7,
    },
    cues: [],
  },
  phrases: [{ text, intent: 'tease' }],
})!

test('accepted director evidence reaches upcoming TTS segments and stays message-scoped', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  try {
    const event = { source: 'reply' as const, messageId: 'message', text: '' }
    runtime.performance.handleForTest(directive, event)
    const speak = (messageId: string, utteranceId: string) => {
      runtime.speech.handleForTest({
        ...event,
        messageId,
        utteranceId,
        phase: 'start',
      })
      const prosody = predictTextProsody({
        text,
        utteranceId,
        startedAtMs: performance.now() + 5_000,
      })
      const detail = meropeSpeechEventDetail({
        ...event,
        messageId,
        utteranceId,
        phase: 'prosody',
        text,
        prosody,
      })!
      runtime.speech.handleForTest(detail)
      return runtime
        .frame(prosody.startedAtMs)
        .speech!.behaviorPlan!.behaviors.map((item) => item.form.id)
    }
    assert.ok(speak('message', 'tts-first').includes('tease'))
    assert.ok(speak('message', 'tts-second').includes('tease'))
    assert.ok(speak('foreign-message', 'tts-foreign').includes('question'))
    assert.ok(
      !runtime
        .frame()
        .speech!.behaviorPlan!.behaviors.some(
          (item) => item.form.id === 'tease',
        ),
    )
  } finally {
    release()
  }
})

test('matching message ids cannot borrow phrases across sources or turn generations', () => {
  const saved = liveMotionGeneration()
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  try {
    setLiveMotionGeneration(7)
    runtime.performance.handleForTest(directive, {
      source: 'reply',
      messageId: 'same-id',
      generation: 7,
      text: '',
    })
    const speak = (source: 'reply' | 'proactive', generation: number) => {
      const event = {
        source,
        messageId: 'same-id',
        generation,
        utteranceId: `${source}-${generation}`,
      }
      runtime.speech.handleForTest({ ...event, phase: 'start' })
      const startedAtMs = performance.now() + 5_000
      runtime.speech.handleForTest({
        ...event,
        phase: 'prosody',
        text,
        prosody: predictTextProsody({
          text,
          utteranceId: event.utteranceId,
          startedAtMs,
        }),
      })
      return runtime
        .frame(startedAtMs)
        .speech!.behaviorPlan!.behaviors.map((item) => item.form.id)
    }
    assert.ok(!speak('proactive', 0).includes('tease'))
    assert.ok(speak('reply', 7).includes('tease'))
    setLiveMotionGeneration(8)
    assert.ok(!speak('reply', 8).includes('tease'))
  } finally {
    release()
    setLiveMotionGeneration(saved)
  }
})

test('late director can revise future text beats, while cancel removes cached direction', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  try {
    const event = { source: 'reply' as const, messageId: 'late', text: '' }
    const start = { ...event, utteranceId: 'text', phase: 'start' as const }
    runtime.speech.handleForTest(start)
    runtime.speech.handleForTest({ ...start, phase: 'chunk', text })
    assert.ok(
      runtime
        .frame()
        .speech!.behaviorPlan!.behaviors.some(
          (item) => item.form.id === 'question',
        ),
    )
    runtime.performance.handleForTest(directive, event)
    assert.ok(
      runtime
        .frame()
        .speech!.behaviorPlan!.behaviors.some(
          (item) => item.form.id === 'tease',
        ),
    )
    runtime.speech.handleForTest({ ...start, phase: 'cancel' })
    runtime.speech.handleForTest({ ...start, utteranceId: 'new' })
    runtime.speech.handleForTest({
      ...start,
      utteranceId: 'new',
      phase: 'chunk',
      text,
    })
    assert.ok(
      runtime
        .frame()
        .speech!.behaviorPlan!.behaviors.some(
          (item) => item.form.id === 'question',
        ),
    )
  } finally {
    release()
  }
})

test('text completion preserves playback; late direction changes only uncommitted beats', (t) => {
  let now = 1_000
  t.mock.method(performance, 'now', () => now)
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const event = {
    source: 'reply' as const,
    messageId: 'completed-text',
    utteranceId: 'completed-text',
  }
  const tail = '不过先把原因说清楚。你觉得呢？'
  try {
    runtime.speech.handleForTest({ ...event, phase: 'start' })
    runtime.speech.handleForTest({
      ...event,
      phase: 'chunk',
      text: text + tail,
    })
    const original = runtime.speech.current().prosody!
    runtime.frame(now)
    now = original.startedAtMs + original.accents[0]!.offsetMs + 1
    runtime.frame(now)
    runtime.speech.handleForTest({ ...event, phase: 'end' })
    assert.ok(runtime.speech.current().behaviorPlan)
    runtime.performance.handleForTest(
      {
        ...directive,
        phrases: [
          { text, intent: 'tease' },
          { text: '你觉得呢？', intent: 'check-in' },
        ],
      },
      { ...event, text: '' },
    )
    const revised = runtime.speech.current().prosody!
    assert.deepEqual(revised.accents[0], original.accents[0])
    assert.ok(revised.accents.some((accent) => accent.gesture === 'check-in'))
    runtime.speech.handleForTest({ ...event, phase: 'cancel' })
    assert.equal(runtime.speech.current().behaviorPlan, null)
  } finally {
    release()
  }
})
