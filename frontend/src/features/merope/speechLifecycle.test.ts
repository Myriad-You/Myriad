import type { SpeechArticulation } from './rig/articulation'
import type { SpeechProsodyPlan } from './speech/prosody'
import type {
  SpeechLifecycleScheduler,
  SpeechLifecycleTarget,
} from './speechLifecycle'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import {
  estimateAutoSpeechDurationMs,
  SpeechLifecycleController,
} from './speechLifecycle'

class FakeScheduler implements SpeechLifecycleScheduler {
  time = 0
  private nextId = 1
  private timers = new Map<number, { at: number; callback: () => void }>()

  now = () => this.time

  setTimeout = (callback: () => void, delayMs: number): unknown => {
    const id = this.nextId++
    this.timers.set(id, { at: this.time + delayMs, callback })
    return id
  }

  clearTimeout = (timer: unknown): void => {
    this.timers.delete(timer as number)
  }

  get activeTimerCount(): number {
    return this.timers.size
  }

  advance(ms: number): void {
    const destination = this.time + ms
    while (true) {
      const next = [...this.timers.entries()]
        .filter(([, timer]) => timer.at <= destination)
        .sort((left, right) => left[1].at - right[1].at)[0]
      if (!next) break
      this.time = next[1].at
      this.timers.delete(next[0])
      next[1].callback()
    }
    this.time = destination
  }
}

function fakeTarget() {
  const active: boolean[] = []
  const auto: boolean[] = []
  const energy: Array<number | null> = []
  const articulation: SpeechArticulation[] = []
  const prosody: Array<SpeechProsodyPlan | null> = []
  const text: Array<{ text: string; locale?: string }> = []
  const target: SpeechLifecycleTarget = {
    setSpeechActive: (value) => active.push(value),
    setAutoSpeech: (value) => auto.push(value),
    setSpeechEnergy: (value) => energy.push(value),
    setSpeechArticulation: (value) => articulation.push(value),
    setSpeechProsody: (value) => prosody.push(value),
    enqueueSpeechText: (value, locale) =>
      text.push({ text: value, ...(locale ? { locale } : {}) }),
  }
  return { target, active, auto, energy, articulation, prosody, text }
}

test('keeps fallback prosody alive for a complete non-streamed reply', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  const base = {
    messageId: 'message-1',
    utteranceId: 'message-1:final',
    source: 'reply' as const,
  }
  controller.handle({ ...base, phase: 'start' })
  controller.handle({ ...base, phase: 'chunk', text: '你好，这是一段回答。' })
  controller.handle({ ...base, phase: 'end' })

  assert.deepEqual(rig.auto, [true])
  assert.deepEqual(rig.active, [true])
  scheduler.advance(500)
  assert.deepEqual(rig.auto, [true])
  // Advance past the estimate rather than a fixed number: the tail is sized to
  // outlast the slowest realized cadence, and that budget is allowed to change.
  scheduler.advance(estimateAutoSpeechDurationMs('你好，这是一段回答。') + 500)
  assert.deepEqual(rig.auto, [true, false])
  assert.deepEqual(rig.active, [true, false])
})

test('keeps only one watchdog while streaming many chunks', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  const base = {
    messageId: 'message-1',
    utteranceId: 'message-1:stream',
    source: 'reply' as const,
  }
  controller.handle({ ...base, phase: 'start' })
  for (let token = 0; token < 100; token += 1) {
    controller.handle({ ...base, phase: 'chunk', text: '字' })
  }
  assert.equal(scheduler.activeTimerCount, 1)
  controller.handle({ ...base, phase: 'end' })
  assert.equal(scheduler.activeTimerCount, 1)
})

test('authored audio energy takes priority over auto prosody', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  const base = {
    messageId: 'message-1',
    utteranceId: 'message-1:audio',
    source: 'reply' as const,
  }
  controller.handle({ ...base, phase: 'start' })
  controller.handle({ ...base, phase: 'energy', energy: 0.72 })
  controller.handle({ ...base, phase: 'chunk', text: 'audio transcript' })
  controller.handle({ ...base, phase: 'end' })

  assert.deepEqual(rig.auto, [true, false])
  assert.deepEqual(rig.energy, [0.72])
  assert.deepEqual(rig.articulation.at(-1), {
    energy: 0,
    viseme: 'rest',
    amount: 0,
  })
  assert.deepEqual(rig.active, [true, false])
})

test('forwards future accent anchors and clears them with the utterance', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  const base = {
    messageId: 'message-1',
    utteranceId: 'message-1:audio',
    source: 'reply' as const,
  }
  const prosody: SpeechProsodyPlan = {
    utteranceId: base.utteranceId,
    startedAtMs: 100,
    durationMs: 800,
    accents: [{ offsetMs: 300, intensity: 0.8 }],
  }
  controller.handle({ ...base, phase: 'start' })
  controller.handle({ ...base, phase: 'prosody', prosody })
  controller.handle({ ...base, phase: 'cancel' })
  assert.deepEqual(rig.prosody, [prosody, null])
})

test('cancellation is scoped to its active message', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  controller.handle({
    phase: 'start',
    messageId: 'message-1',
    utteranceId: 'stream-1',
    source: 'reply',
  })
  controller.handle({
    phase: 'cancel',
    messageId: 'message-2',
    source: 'reply',
  })
  assert.deepEqual(rig.auto, [true])
  assert.deepEqual(rig.active, [true])
  controller.handle({
    phase: 'cancel',
    messageId: 'message-1',
    source: 'reply',
  })
  assert.deepEqual(rig.auto, [true, false])
  assert.deepEqual(rig.active, [true, false])
})

test('busy changes follow occupancy so the motion coordinator can claim the mouth', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const occupancy = { current: false }
  const busy: boolean[] = []
  const controller = new SpeechLifecycleController(
    rig.target,
    scheduler,
    occupancy,
    (value) => busy.push(value),
  )
  const base = {
    messageId: 'message-1',
    utteranceId: 'message-1:final',
    source: 'reply' as const,
  }
  controller.handle({ ...base, phase: 'start' })
  assert.deepEqual(busy, [true])
  controller.handle({ ...base, phase: 'chunk', text: '好。' })
  controller.handle({ ...base, phase: 'end' })
  scheduler.advance(3_000)
  assert.deepEqual(busy, [true, false])
})

test('occupancy stays busy after end until the auto-speech tail finishes', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const occupancy = { current: false }
  const controller = new SpeechLifecycleController(
    rig.target,
    scheduler,
    occupancy,
  )
  const base = {
    messageId: 'message-1',
    utteranceId: 'message-1:final',
    source: 'reply' as const,
  }
  controller.handle({ ...base, phase: 'start' })
  assert.equal(occupancy.current, true)
  controller.handle({ ...base, phase: 'chunk', text: '你好，这是一段回答。' })
  controller.handle({ ...base, phase: 'end' })
  assert.equal(occupancy.current, true)
  scheduler.advance(estimateAutoSpeechDurationMs('你好，这是一段回答。') + 500)
  assert.equal(occupancy.current, false)
})

test('bounds local duration estimates for short and very long replies', () => {
  assert.equal(estimateAutoSpeechDurationMs('好'), 650)
  assert.ok(estimateAutoSpeechDurationMs('This is a short answer.') >= 1_500)
  assert.ok(estimateAutoSpeechDurationMs('长'.repeat(2_000)) > 12_000)
  assert.equal(
    estimateAutoSpeechDurationMs('长'.repeat(20_000)),
    estimateAutoSpeechDurationMs('长'.repeat(2_000)),
  )
})

test('a long visual reply finishes its bounded text instead of hitting the stalled-stream timeout', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  const base = {
    messageId: 'long',
    utteranceId: 'long',
    source: 'reply' as const,
  }
  const text = '这句已经说完了。'.repeat(20)
  controller.handle({ ...base, phase: 'start' })
  controller.handle({ ...base, phase: 'chunk', text })
  // Even when end has not arrived, already queued words are not a dead stream.
  scheduler.advance(12_001)
  assert.equal(rig.active.at(-1), true)
  controller.handle({ ...base, phase: 'end' })
  scheduler.advance(estimateAutoSpeechDurationMs(text) - scheduler.time - 1)
  assert.equal(rig.active.at(-1), true)
  scheduler.advance(2)
  assert.equal(rig.active.at(-1), false)
  assert.equal(scheduler.activeTimerCount, 0)
})

test('late streamed words get their own tail while silence and cancellation still release ownership', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  const base = {
    messageId: 'late',
    utteranceId: 'late',
    source: 'reply' as const,
  }
  controller.handle({ ...base, phase: 'start' })
  controller.handle({ ...base, phase: 'chunk', text: '好的。' })
  scheduler.advance(10_000)
  controller.handle({
    ...base,
    phase: 'chunk',
    text: '不过这句话现在才到，我们还得说完。',
  })
  controller.handle({ ...base, phase: 'end' })
  scheduler.advance(500)
  assert.equal(rig.active.at(-1), true)
  controller.handle({ ...base, phase: 'cancel' })
  assert.equal(rig.active.at(-1), false)
  assert.equal(scheduler.activeTimerCount, 0)
  controller.handle({ ...base, phase: 'start' })
  scheduler.advance(12_001)
  assert.equal(rig.active.at(-1), false)
})

test('the same text budget reaches the mouth and the lifecycle estimate', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  const base = {
    messageId: 'budget',
    utteranceId: 'budget',
    source: 'reply' as const,
  }
  controller.handle({ ...base, phase: 'start' })
  controller.handle({ ...base, phase: 'chunk', text: '字'.repeat(1_999) })
  controller.handle({ ...base, phase: 'chunk', text: '你好。' })
  controller.handle({ ...base, phase: 'chunk', text: '后续不应延长。' })
  assert.equal(
    rig.text.map((item) => item.text).join(''),
    `${'字'.repeat(1_999)}你`,
  )
  controller.dispose()
})

test('uses punctuation and locale to estimate natural visual speech phrasing', () => {
  const plain = estimateAutoSpeechDurationMs('你好世界再见', 'zh-CN')
  const phrased = estimateAutoSpeechDurationMs('你好，世界。再见', 'zh-CN')
  assert.ok(phrased >= plain + 450)

  const han = '今天天气很好我们出去走走'
  assert.ok(
    estimateAutoSpeechDurationMs(han, 'zh-CN') >
      estimateAutoSpeechDurationMs(han, 'ja-JP'),
  )
})

test('ignores speech from an older generation without cancelling the live one', async () => {
  const { setLiveMotionGeneration } = await import('./motion/liveGeneration')
  setLiveMotionGeneration(3)
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  controller.handle({
    phase: 'start',
    messageId: 'message-new',
    utteranceId: 'u-new',
    source: 'reply',
    generation: 3,
  })
  controller.handle({
    phase: 'start',
    messageId: 'message-old',
    utteranceId: 'u-old',
    source: 'reply',
    generation: 2,
  })
  assert.deepEqual(rig.active, [true])
  setLiveMotionGeneration(0)
})

test('a live-conversation energy frame cannot evict a reply that is speaking', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  controller.handle({
    phase: 'start',
    messageId: 'reply-1',
    utteranceId: 'u-1',
    source: 'reply',
  })
  controller.handle({
    phase: 'chunk',
    messageId: 'reply-1',
    utteranceId: 'u-1',
    source: 'reply',
    text: '在的',
  })
  // Agora samples the remote track at 20Hz under its own conversation id.
  controller.handle({
    phase: 'energy',
    messageId: 'convo-agent-7',
    utteranceId: 'convo-agent-7',
    source: 'reply',
    energy: 0.4,
  })
  assert.deepEqual(rig.active, [true])
  assert.deepEqual(rig.energy, [])
  assert.deepEqual(rig.text, [{ text: '在的' }])
})

test('a live-conversation energy frame still opens an idle mouth', () => {
  const scheduler = new FakeScheduler()
  const rig = fakeTarget()
  const controller = new SpeechLifecycleController(rig.target, scheduler)
  controller.handle({
    phase: 'energy',
    messageId: 'convo-agent-7',
    utteranceId: 'convo-agent-7',
    source: 'reply',
    energy: 0.4,
  })
  assert.deepEqual(rig.active, [true])
  assert.deepEqual(rig.energy, [0.4])
})

test('live-conversation frames carry their adopted run generation', () => {
  const source = readFileSync(
    new URL('./speech/agoraConversation.ts', import.meta.url),
    'utf8',
  )
  assert.match(source, /generation: identity\.generation/)
})
