import type { PlaybackDirectionSnapshot } from './playbackDirection'
import assert from 'node:assert/strict'
import test from 'node:test'
import { sanitizePerformanceDirective } from '../performanceEvents'
import { TtsPipeline } from '../speech/ttsPipeline'
import { RigMotionCoordinator } from './coordinator'
import {
  PlaybackDirectionClient,
  sendPlaybackObservation,
} from './playbackDirection'
import { MotionRuntime } from './runtime'

const scope = { runId: 'run', messageId: 'message', generation: 1 }
const performance = {
  phase: 'delivery',
  moodRevision: 1,
  motionStyle: 'even',
  plan: { baseline: null, cues: [] },
  phrases: [{ text: '你觉得呢？', intent: 'check-in' }],
}
const result = { version: 1, closed: false, performance }
async function flush() {
  for (let i = 0; i < 5; i++) await Promise.resolve()
}

test('production feedback sends CSRF, captures fresh evidence and never sends after cancellation during auth', async () => {
  for (const cancelled of [false, true]) {
    let token!: (value: string) => void
    let text = '旧句子'
    let sent = 0
    let captured = 0
    let refreshed = 0
    const controller = new AbortController()
    const task = sendPlaybackObservation('/performance', controller.signal, {
      token: () => {
        const deferred = Promise.withResolvers<string>()
        token = deferred.resolve
        return deferred.promise
      },
      clearToken: () => {
        refreshed++
      },
      capture: () => {
        captured++
        return { upcomingText: text, rig: {} }
      },
      fetch: async (_url, init) => {
        sent++
        assert.equal(
          new Headers(init?.headers).get('X-CSRF-Token'),
          'synthetic-token',
        )
        assert.equal(init?.method, 'PUT')
        assert.equal(init?.credentials, 'include')
        assert.equal(init?.signal, controller.signal)
        assert.equal(JSON.parse(String(init?.body)).upcomingText, '新句子')
        return new Response(null, { status: 403 })
      },
    })
    text = '新句子'
    if (cancelled) controller.abort()
    token('synthetic-token')
    await task
    assert.equal(sent, cancelled ? 0 : 1)
    assert.equal(captured, sent)
    assert.equal(refreshed, sent)
  }
})

test('feedback is single-flight, bounded, nonblocking and cancelled with its round', async () => {
  let finish!: () => void
  let count = 0
  let signal!: AbortSignal
  const client = new PlaybackDirectionClient({
    observe: (_scope, next) => {
      count++
      signal = next
      const deferred = Promise.withResolvers<void>()
      finish = deferred.resolve
      return deferred.promise
    },
    read: () => new Promise(() => {}),
    close: () => {},
    current: () => true,
    playing: () => true,
    sanitize: sanitizePerformanceDirective,
    deliver: () => {},
    note: () => {},
  })
  client.start(scope)
  client.check(10000)
  assert.equal(count, 1)
  finish()
  await flush()
  client.check(20000)
  assert.equal(count, 2)
  finish()
  await flush()
  client.check(20001)
  assert.equal(count, 2)
  client.stop()
  assert.equal(signal.aborted, true)
})

function fixture(isPlaying?: () => boolean) {
  let resolve!: (value: PlaybackDirectionSnapshot) => void
  let current = true
  let playing = true
  const delivered: unknown[] = []
  const closed: string[] = []
  const notes: string[] = []
  const signals: AbortSignal[] = []
  const client = new PlaybackDirectionClient({
    observe: async () => {},
    read: (_runId, _after, signal) => {
      signals.push(signal)
      const deferred = Promise.withResolvers<PlaybackDirectionSnapshot>()
      resolve = deferred.resolve
      return deferred.promise
    },
    close: (id) => closed.push(id),
    current: () => current,
    playing: () => (isPlaying ? isPlaying() : playing),
    sanitize: sanitizePerformanceDirective,
    deliver: (_scope, value) => delivered.push(value),
    note: (reason) => notes.push(reason),
  })
  return {
    client,
    delivered,
    closed,
    notes,
    signals,
    resolve: (value = result) => resolve(value),
    playing: (value: boolean) => {
      playing = value
    },
    current: (value: boolean) => {
      current = value
    },
  }
}

test('text completion does not abort direction while text or queued TTS is still playing', async () => {
  for (const mode of ['text-tail', 'tts-playing', 'tts-queued']) {
    const f = fixture()
    f.client.start(scope)
    f.client.textEnded(scope.messageId)
    assert.equal(f.signals[0]!.aborted, false, mode)
    f.resolve()
    await flush()
    assert.equal(f.delivered.length, 1, mode)
    f.playing(false)
    f.client.check()
    assert.equal(f.signals.at(-1)!.aborted, true, mode)
    assert.deepEqual(f.closed, ['run'])
  }
})

test('a late response is rejected against current playback, without waiting for the next check tick', async () => {
  const f = fixture()
  f.client.start(scope)
  f.client.textEnded(scope.messageId)
  f.playing(false)
  f.resolve()
  await flush()
  assert.equal(f.delivered.length, 0)
  assert.ok(f.notes.includes('expired'))
})

test('real TTS synthesis and queue keep the window open until the last segment finishes', async () => {
  const synth: Array<(audio: ArrayBuffer) => void> = []
  const ends: Array<() => void> = []
  const pipeline = new TtsPipeline({
    synthesize: () => {
      const deferred = Promise.withResolvers<ArrayBuffer>()
      synth.push(deferred.resolve)
      return deferred.promise
    },
    play: (_audio, _segment, onEnded) => {
      ends.push(onEnded)
      return { stop: () => {} }
    },
  })
  pipeline.enqueue(
    [1, 2].map((sequence) => ({
      segmentId: `message:${sequence}`,
      sequence,
      text: '你觉得呢？',
      messageId: scope.messageId,
      generation: 1,
      interrupt: 'queue' as const,
    })),
  )
  const f = fixture(() => pipeline.isBusyWith(scope.messageId))
  f.client.start(scope)
  f.client.textEnded(scope.messageId)
  assert.equal(pipeline.playing, false)
  assert.equal(f.signals[0]!.aborted, false)
  f.resolve()
  await flush()
  assert.equal(f.delivered.length, 1)
  synth[0]!(new ArrayBuffer(1))
  synth[1]!(new ArrayBuffer(1))
  await flush()
  ends[0]!()
  f.client.check()
  assert.equal(f.signals.at(-1)!.aborted, false)
  ends[1]!()
  f.client.check()
  assert.equal(f.signals.at(-1)!.aborted, true)
})

test('replacement and explicit interruption cannot revive the old message even if abort is ignored', async () => {
  for (const reason of ['cancel', 'generation']) {
    const f = fixture()
    f.client.start(scope)
    if (reason === 'cancel') f.client.cancel(scope.messageId)
    else f.current(false)
    f.resolve()
    await flush()
    assert.equal(f.delivered.length, 0)
    assert.deepEqual(f.closed, ['run'])
  }
})

test('the closed producer can deliver its last result without clearing ongoing body playback', async () => {
  const f = fixture()
  f.client.start(scope)
  f.resolve({ ...result, closed: true })
  await flush()
  assert.equal(f.delivered.length, 1)
  assert.deepEqual(f.closed, ['run'])
})

test('timeline: model result after text completion revises an upcoming real speech beat, not a past beat', async (t) => {
  for (const [label, arrival] of [
    ['early', 200],
    ['after-text-before-beat', 800],
    ['after-playback', 20_000],
  ] as const) {
    t.mock.timers.enable({ apis: ['setTimeout'] })
    let now = 1_000
    t.mock.method(globalThis.performance, 'now', () => now)
    const runtime = new MotionRuntime(new RigMotionCoordinator())
    const release = runtime.retain()
    const event = {
      source: 'reply' as const,
      messageId: scope.messageId,
      utteranceId: 'line',
      generation: 1,
    }
    const local = { ...event, generation: undefined }
    let resolve!: (value: PlaybackDirectionSnapshot) => void
    let received = 0
    const client = new PlaybackDirectionClient({
      observe: async () => {},
      read: () => {
        const deferred = Promise.withResolvers<PlaybackDirectionSnapshot>()
        resolve = deferred.resolve
        return deferred.promise
      },
      close: () => {},
      current: () => true,
      playing: () => runtime.speech.hasPlayback(local),
      sanitize: sanitizePerformanceDirective,
      deliver: (_scope, value) => {
        received++
        runtime.performance.handleForTest(value, { ...local, text: '' })
      },
      note: () => {},
    })
    try {
      runtime.speech.handleForTest({ ...local, phase: 'start' })
      runtime.speech.handleForTest({
        ...local,
        phase: 'chunk',
        text: '我们先慢慢把这件事情说清楚，再一起考虑一下。你觉得呢？',
      })
      runtime.frame(now)
      const before = runtime.speech.current().prosody!
      client.start(scope)
      now += 100
      t.mock.timers.tick(100)
      runtime.speech.handleForTest({ ...local, phase: 'end' })
      client.textEnded(scope.messageId)
      now = 1_000 + arrival
      t.mock.timers.tick(arrival - 100)
      runtime.frame(now)
      resolve({ ...result, closed: true })
      await flush()
      const changed =
        runtime.speech
          .current()
          .prosody?.accents.filter(
            (accent, i) => accent.gesture !== before.accents[i]?.gesture,
          ).length ?? 0
      assert.equal(received, arrival < 15_000 ? 1 : 0, label)
      assert.equal(changed > 0, arrival < 15_000, label)
      t.diagnostic(
        `${label}: received=${received}, changedFutureAccents=${changed}; synthetic clock, not provider latency`,
      )
    } finally {
      client.stop()
      release()
      t.mock.timers.reset()
    }
  }
})
