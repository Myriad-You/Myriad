import type { SpeechArticulation } from './rig/articulation'
import type {
  SpeechLifecycleScheduler,
  SpeechLifecycleTarget,
} from './speechLifecycle'
import assert from 'node:assert/strict'
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
  const target: SpeechLifecycleTarget = {
    setSpeechActive: (value) => active.push(value),
    setAutoSpeech: (value) => auto.push(value),
    setSpeechEnergy: (value) => energy.push(value),
    setSpeechArticulation: (value) => articulation.push(value),
  }
  return { target, active, auto, energy, articulation }
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
  scheduler.advance(3_000)
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

test('bounds local duration estimates for short and very long replies', () => {
  assert.equal(estimateAutoSpeechDurationMs('好'), 600)
  assert.ok(estimateAutoSpeechDurationMs('This is a short answer.') >= 1_500)
  assert.equal(estimateAutoSpeechDurationMs('长'.repeat(2_000)), 12_000)
})
