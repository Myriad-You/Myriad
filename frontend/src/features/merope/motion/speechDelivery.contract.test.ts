import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { Anime25DBehaviorMotionController } from '../anime25drig/behaviorMotion'
import { realizeAnime25DBehaviorPlan } from '../anime25drig/behaviorRealizer'
import { meropePerformanceEventDetail } from '../performanceEvents'
import { RigMotionCoordinator } from './coordinator'
import { MotionRuntime } from './runtime'

const wire = process.env.MEROPE_DELIVERY_WIRE_PATH

test(
  'backend phrase-only updates become distinct body gestures without replacing the standing face',
  { skip: !wire },
  (t) => {
    const data = JSON.parse(readFileSync(wire!, 'utf8'))
    assert.equal(typeof data.text, 'string')
    assert.ok(!data.text.includes('[[delivery:'))
    assert.equal(data.events.length, 4)
    let now = 1_000
    t.mock.method(performance, 'now', () => now)
    const runtime = new MotionRuntime(new RigMotionCoordinator())
    const release = runtime.retain()
    const body = new Anime25DBehaviorMotionController()
    const base = {
      source: 'reply' as const,
      messageId: 'wire',
      utteranceId: 'wire',
    }
    const deliver = (performance: unknown) => {
      const detail = meropePerformanceEventDetail({
        ...base,
        text: '',
        performance,
      })
      assert.ok(
        detail?.performance,
        'the production event boundary must accept phrase-only plans',
      )
      runtime.performance.handle(detail)
    }
    try {
      deliver({
        phase: 'delivery',
        moodRevision: 1,
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
      })
      const standing = runtime.performance.currentBearing()
      for (const event of data.events) {
        assert.equal(event.type, 'performance_plan')
        deliver(event.performance)
        assert.deepEqual(runtime.performance.currentBearing(), standing)
      }
      runtime.speech.handleForTest({ ...base, phase: 'start' })
      runtime.speech.handleForTest({ ...base, phase: 'chunk', text: data.text })
      const end = now + runtime.speech.current().prosody!.durationMs + 1_500
      const peaks = { hesitate: 0, contrast: 0, 'check-in': 0, tease: 0 }
      let revision = -1
      for (; now < end; now += 16) {
        const frame = runtime.frame(now)
        if (frame.behaviorRevision !== revision) {
          revision = frame.behaviorRevision
          const realized = realizeAnime25DBehaviorPlan(frame.behaviorPlan!, now)
          assert.ok(
            realized.reports.every((report) => report.result === 'accepted'),
          )
          body.replace(realized.units, now, now / 1_000)
        }
        const gestures = body.sample(now / 1_000).coSpeechGesture
        for (const key of Object.keys(peaks) as Array<keyof typeof peaks>) {
          peaks[key] = Math.max(peaks[key], gestures[key])
        }
      }
      for (const [intent, peak] of Object.entries(peaks))
        assert.ok(peak > 0.5, intent)
      runtime.speech.handleForTest({ ...base, phase: 'cancel' })
      runtime.performance.handleSpeech({ ...base, phase: 'cancel' })
      deliver({
        ...data.events[0].performance,
        phrases: [{ text: '迟到的句子。', intent: 'laugh' }],
      })
      assert.equal(runtime.speech.current().behaviorPlan, null)
      assert.equal(runtime.performance.currentBearing(), null)
    } finally {
      release()
    }
  },
)
