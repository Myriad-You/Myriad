import assert from 'node:assert/strict'
import test from 'node:test'
import { Anime25DBehaviorMotionController } from '../anime25drig/behaviorMotion'
import { realizeAnime25DBehaviorPlan } from '../anime25drig/behaviorRealizer'
import { sanitizePerformanceDirective } from '../performanceEvents'
import { predictTextProsody } from '../speech/textProsody'
import { meropeSpeechEventDetail } from '../speechEvents'
import { RigMotionCoordinator } from './coordinator'
import { HumanPerformanceRuntime } from './humanPerformanceRuntime'
import { SpeechMotionSource } from './speechSource'

test('incremental hesitate → explain → check-in preserves the drawn body and produces all three motions', () => {
  let now = 1000
  const human = new HumanPerformanceRuntime()
  const body = new Anime25DBehaviorMotionController()
  const source = new SpeechMotionSource(
    new RigMotionCoordinator(),
    () => {},
    () => human.snapshots(now),
    { now: () => now, setTimeout: () => 1, clearTimeout: () => {} },
  )
  const event = {
    source: 'reply' as const,
    messageId: 'continuation',
    utteranceId: 'continuation',
  }
  const direct = (phrases: Array<{ text: string; intent: string }>) =>
    source.applyDirector(
      sanitizePerformanceDirective({
        phase: 'delivery',
        moodRevision: 1,
        motionStyle: 'even',
        plan: { baseline: null, cues: [] },
        phrases,
      })!,
      { ...event, text: '' },
      null,
    )
  source.start()
  try {
    source.handleForTest({ ...event, phase: 'start' })
    source.handleForTest({
      ...event,
      phase: 'chunk',
      text: '也许我们可以试试。其实可以先把原因说清楚。你觉得呢？',
    })
    direct([{ text: '也许我们可以试试。', intent: 'hesitate' }])
    const original = source.current().prosody!
    const firstPeak = original.startedAtMs + original.accents[0]!.offsetMs
    const end = original.startedAtMs + original.durationMs + 1500
    const peaks = { hesitate: 0, contrast: 0, 'check-in': 0 }
    let updated = false
    let revision = -1
    for (; now < end; now += 16) {
      if (!updated && now > firstPeak + 50) {
        updated = true
        direct([
          { text: '其实可以先把原因说清楚。', intent: 'explain' },
          { text: '你觉得呢？', intent: 'check-in' },
        ])
        assert.deepEqual(
          source.current().prosody!.accents[0],
          original.accents[0],
        )
      }
      const frame = human.frame([source.current().behaviorPlan], now)
      if (revision !== frame.revision) {
        revision = frame.revision
        const before = body.sample(now / 1000)
        const drawn = Object.fromEntries(
          Object.keys(peaks).map((key) => [
            key,
            before.coSpeech * before.coSpeechGesture[key as keyof typeof peaks],
          ]),
        )
        const realized = realizeAnime25DBehaviorPlan(frame.plan!, now)
        body.replace(realized.units, now, now / 1000)
        const after = body.sample(now / 1000)
        for (const key of Object.keys(peaks) as Array<keyof typeof peaks>) {
          assert.ok(
            Math.abs(
              after.coSpeech * after.coSpeechGesture[key] - drawn[key]!,
            ) < 0.001,
            `${key} jumped at plan revision ${revision}`,
          )
        }
      }
      const sample = body.sample(now / 1000)
      for (const key of Object.keys(peaks) as Array<keyof typeof peaks>) {
        peaks[key] = Math.max(
          peaks[key],
          sample.coSpeech * sample.coSpeechGesture[key],
        )
      }
    }
    assert.ok(updated)
    for (const [intent, amplitude] of Object.entries(peaks)) {
      assert.ok(
        amplitude > 0.3,
        `${intent} never reached visible body output: ${amplitude}`,
      )
    }
  } finally {
    source.stop()
  }
})

test('a rolling speech window reaches the final question through the real scheduler and body adapter', () => {
  let now = 1_000
  const scheduler = {
    now: () => now,
    setTimeout: () => 1,
    clearTimeout: () => {},
  }
  const human = new HumanPerformanceRuntime()
  const body = new Anime25DBehaviorMotionController()
  const source = new SpeechMotionSource(
    new RigMotionCoordinator(),
    () => {},
    () => human.snapshots(now),
    scheduler,
  )
  source.start()
  const event = {
    source: 'reply' as const,
    messageId: 'long',
    utteranceId: 'long',
  }
  try {
    source.handleForTest({ ...event, phase: 'start' })
    source.handleForTest({
      ...event,
      phase: 'chunk',
      text: `${'这一句说完了。'.repeat(20)}不过还有个办法。你觉得呢？`,
    })
    const prosody = source.current().prosody!
    const last = prosody.accents.at(-1)!
    assert.equal(last.gesture, 'question')
    assert.ok(prosody.accents.length > 12)
    assert.ok(
      source.current().behaviorPlan!.behaviors.length < prosody.accents.length,
    )
    const identities = new Map<string, number>()
    let question = 0
    let revision = -1
    for (; now <= prosody.startedAtMs + last.offsetMs + 700; now += 16) {
      const intent = source.current()
      assert.strictEqual(
        source.current().behaviorPlan,
        intent.behaviorPlan,
        'unchanged windows must reuse their plan',
      )
      // A 4s lookahead + 1.1s recovery and 380ms minimum spacing bound live work.
      assert.ok(intent.behaviorPlan!.behaviors.length <= 16)
      const frame = human.frame([intent.behaviorPlan], now)
      for (const behavior of frame.plan!.behaviors) {
        const peak = frame.plan!.pegs.find(
          (peg) => peg.id === behavior.timing.strokePeak,
        )!.atMs
        if (identities.has(behavior.id))
          assert.equal(peak, identities.get(behavior.id))
        identities.set(behavior.id, peak)
      }
      if (frame.revision !== revision) {
        revision = frame.revision
        const realized = realizeAnime25DBehaviorPlan(frame.plan!, now)
        assert.ok(
          realized.reports.every((report) => report.result === 'accepted'),
        )
        body.replace(realized.units, now, now / 1_000)
      }
      question = Math.max(
        question,
        body.sample(now / 1_000).coSpeechGesture.question,
      )
    }
    assert.ok(identities.has(`speech:long:text-${last.textOffset}`))
    assert.ok(
      question > 0.5,
      'the final semantic gesture must produce body output, not only a plan entry',
    )
    source.handleForTest({ ...event, phase: 'cancel' })
    assert.equal(source.current().behaviorPlan, null)
    assert.equal(source.current().prosody, null)
    human.frame([], now)
    const stopped = human.frame([], now + 1_000).behaviors
    assert.ok(
      stopped.every((behavior) => behavior.phase === 'complete'),
      JSON.stringify(stopped),
    )
  } finally {
    source.stop()
  }
})

test('queued TTS fragments survive later direction updates, corrections, and quoted evidence', () => {
  const scheduler = {
    now: () => 1_000,
    setTimeout: () => 1,
    clearTimeout: () => {},
  }
  const source = new SpeechMotionSource(
    new RigMotionCoordinator(),
    () => {},
    undefined,
    scheduler,
  )
  source.start()
  const event = { source: 'reply' as const, messageId: 'queued', text: '' }
  const direct = (text: string, intent: string) =>
    source.applyDirector(
      sanitizePerformanceDirective({
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
        phrases: [{ text, intent }],
      })!,
      event,
      null,
    )
  const speak = (text: string, id: string) => {
    const base = { ...event, utteranceId: id }
    source.handleForTest({ ...base, phase: 'start' })
    source.handleForTest(
      meropeSpeechEventDetail({
        ...base,
        phase: 'prosody',
        text,
        prosody: predictTextProsody({
          text,
          utteranceId: id,
          startedAtMs: 1_000,
        }),
      })!,
    )
    return source.current().prosody!.accents.map((accent) => accent.gesture)
  }
  try {
    direct('你真的这么想吗？', 'tease')
    direct('你觉得呢？', 'check-in')
    assert.ok(speak('你真的这么想吗？', 'first').includes('tease'))
    assert.ok(speak('你觉得呢？', 'second').includes('check-in'))
    assert.ok(!speak('他说：“你真的这么想吗？”', 'quoted').includes('tease'))
    direct('你真的这么想吗？', 'none')
    assert.ok(speak('你真的这么想吗？', 'corrected').includes('none'))
    source.handleForTest({ ...event, phase: 'cancel' })
    assert.ok(speak('你真的这么想吗？', 'after-cancel').includes('question'))
  } finally {
    source.stop()
  }
})

test('text increments reach the source without renaming earlier beats, and cancel clears them', () => {
  const source = new SpeechMotionSource(new RigMotionCoordinator(), () => {})
  source.start()
  try {
    const base = {
      messageId: 'message',
      utteranceId: 'stream',
      source: 'reply' as const,
    }
    source.handleForTest({ ...base, phase: 'start' })
    source.handleForTest({
      ...base,
      phase: 'chunk',
      text: '其实我们可以试试。',
    })
    const first = source.current().prosody!
    assert.ok(first.accents.length > 0)
    source.handleForTest({
      ...base,
      phase: 'chunk',
      text: '不过后面的结果呢？',
    })
    const later = source.current().prosody!
    assert.ok(later.accents.length > first.accents.length)
    assert.deepEqual(
      later.accents.slice(0, first.accents.length),
      first.accents,
    )
    source.handleForTest({ ...base, phase: 'end' })
    assert.deepEqual(source.current().prosody!.accents, later.accents)
    source.handleForTest({ ...base, phase: 'cancel' })
    assert.equal(source.current().behaviorPlan, null)
    assert.equal(source.current().prosody, null)
    source.handleForTest({ ...base, utteranceId: 'next', phase: 'start' })
    assert.deepEqual(source.current().prosody!.accents, [])
  } finally {
    source.stop()
  }
})
