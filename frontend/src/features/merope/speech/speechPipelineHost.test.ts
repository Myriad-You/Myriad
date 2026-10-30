import type { MeropeSpeechEventDetail } from '@myriad/figure/speech/speechEvents'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  liveMotionGeneration,
  setLiveMotionGeneration,
} from '@myriad/figure/motion/liveGeneration'
import { MEROPE_SPEECH_EVENT } from '@myriad/figure/speech/speechEvents'
import {
  createConfigDomain,
  executeConfigOperations,
} from '../../../components/config/form/configDomain'
import { invalidateSpeechStatusCache } from '../../../services/speechApi'
import { setKnownAuthState } from '../../../utils/authState'
import { personaSpeechFlags, SpeechPipelineHost } from './speechPipelineHost'
import { SpeechSegmenter } from './speechSegmenter'
import { getVoicePresence, patchVoicePresence } from './voicePresence'

test.beforeEach(() => {
  setKnownAuthState(true)
})

test('persona speech is off unless the status flag is on', () => {
  assert.deepEqual(personaSpeechFlags({ available: true, tts_enabled: true }), {
    speechEnabled: false,
    ttsReady: true,
  })
  assert.deepEqual(
    personaSpeechFlags({
      available: true,
      tts_enabled: true,
      persona_speech_enabled: false,
    }),
    { speechEnabled: false, ttsReady: true },
  )
  assert.deepEqual(
    personaSpeechFlags({
      available: true,
      tts_enabled: true,
      persona_speech_enabled: true,
    }),
    { speechEnabled: true, ttsReady: true },
  )
})

/** The shared audio context the host plays on, its decoding held in hand. */
let decoding: Promise<unknown> = Promise.resolve({ duration: 1 })
class FakeAudioContext {
  state: AudioContextState = 'running'
  currentTime = 0
  destination = {}
  decodeAudioData(): Promise<unknown> {
    return decoding
  }

  async resume(): Promise<void> {
    this.state = 'running'
  }

  async suspend(): Promise<void> {
    this.state = 'suspended'
  }

  createAnalyser(): unknown {
    return {
      fftSize: 256,
      getByteTimeDomainData: (bins: Uint8Array) => bins.fill(128),
      connect: () => {},
      disconnect: () => {},
    }
  }

  createBufferSource(): unknown {
    return {
      buffer: null,
      onended: null,
      connect: () => {},
      disconnect: () => {},
      start: () => {},
      stop: () => {},
    }
  }
}

function withAudio(): () => void {
  const saved = {
    AudioContext: globalThis.AudioContext,
    requestAnimationFrame: globalThis.requestAnimationFrame,
    cancelAnimationFrame: globalThis.cancelAnimationFrame,
  }
  globalThis.AudioContext = FakeAudioContext as unknown as typeof AudioContext
  globalThis.requestAnimationFrame = (() => 1) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = (() => {}) as typeof cancelAnimationFrame
  return () => {
    decoding = Promise.resolve({ duration: 1 })
    Object.assign(globalThis, saved)
  }
}

function settle(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve))
}

function capturedSpeech(): {
  events: MeropeSpeechEventDetail[]
  restore: () => void
} {
  const savedWindow = globalThis.window
  const events: MeropeSpeechEventDetail[] = []
  const target = new EventTarget()
  target.addEventListener(MEROPE_SPEECH_EVENT, (event) =>
    events.push((event as CustomEvent<MeropeSpeechEventDetail>).detail),
  )
  globalThis.window = target as unknown as Window & typeof globalThis
  return {
    events,
    restore: () => {
      globalThis.window = savedWindow
    },
  }
}

test('queued speech keeps its original source and generation through playback and cancellation', async () => {
  const savedGeneration = liveMotionGeneration()
  const { events, restore } = capturedSpeech()
  const restoreAudio = withAudio()
  try {
    const host = new SpeechPipelineHost()
    setLiveMotionGeneration(9)
    for (const [source, generation] of [
      ['reply', 7],
      ['proactive', 0],
    ] as const) {
      const segment = new SpeechSegmenter(
        `message-${source}`,
        generation,
        'zh-CN',
        source,
      ).push('你真的这么想吗？')[0]!
      // Test the real outlet without widening its production visibility.
      const handle = host.play(new ArrayBuffer(0), segment, () => {})
      await settle()
      handle.stop()
      const scoped = events.filter(
        (event) => event.messageId === segment.messageId,
      )
      assert.ok(scoped.some((event) => event.phase === 'start'))
      assert.ok(scoped.some((event) => event.phase === 'cancel'))
      assert.ok(
        scoped.every(
          (event) =>
            event.source === source && (event.generation ?? 0) === generation,
        ),
      )
    }
  } finally {
    restore()
    restoreAudio()
    setLiveMotionGeneration(savedGeneration)
    patchVoicePresence({ ttsPlaying: false })
  }
})

test('she begins speaking when the voice starts, not while it is decoded or held', async () => {
  const { events, restore } = capturedSpeech()
  const restoreAudio = withAudio()
  const decoded = Promise.withResolvers<unknown>()
  decoding = decoded.promise
  const segment = new SpeechSegmenter('decoding', 0, 'zh-CN').push('你真的这么想吗？')[0]!
  const began = (): MeropeSpeechEventDetail[] =>
    events.filter(
      (event) => event.messageId === segment.messageId && event.phase === 'start',
    )
  try {
    const host = new SpeechPipelineHost()
    const handle = host.play(new ArrayBuffer(0), segment, () => {})
    await settle()
    assert.deepEqual(began(), [], 'not while it is decoded')
    assert.equal(getVoicePresence().ttsPlaying, false)

    // Decoded while held: it waits for the release to begin.
    host.pipeline.enqueue = () => {}
    Object.defineProperty(host.pipeline, 'playing', { get: () => true })
    assert.equal(host.hold(), true)
    decoded.resolve({ duration: 1 })
    await settle()
    assert.deepEqual(began(), [], 'not while it is held')
    assert.equal(getVoicePresence().ttsPlaying, false)
    assert.ok(
      events.every((event) => event.messageId !== segment.messageId),
      'no mouth, no gestures before it begins',
    )

    host.resume()
    await settle()
    assert.equal(began().length, 1)
    assert.equal(getVoicePresence().ttsPlaying, true)
    const mine = events.filter((event) => event.messageId === segment.messageId)
    assert.equal(mine[0]?.phase, 'start', 'begun before its mouth and gestures')
    handle.stop()
  } finally {
    restore()
    restoreAudio()
    patchVoicePresence({ ttsPlaying: false })
  }
})

test('a voice that never starts is not reported as having begun or ended', async () => {
  const { events, restore } = capturedSpeech()
  const restoreAudio = withAudio()
  decoding = Promise.reject(new Error('bad audio'))
  decoding.catch(() => {})
  const segment = new SpeechSegmenter('broken', 0, 'zh-CN').push('你真的这么想吗？')[0]!
  try {
    let ended = 0
    new SpeechPipelineHost().play(new ArrayBuffer(0), segment, () => {
      ended += 1
    })
    await settle()
    assert.equal(ended, 1, 'the queue still moves on')
    assert.deepEqual(
      events.filter((event) => event.messageId === segment.messageId),
      [],
    )
  } finally {
    restore()
    restoreAudio()
  }
})

test('TTS is not ready without a provider even when speaking is on', () => {
  assert.deepEqual(
    personaSpeechFlags({
      available: false,
      tts_enabled: false,
      persona_speech_enabled: true,
    }),
    { speechEnabled: true, ttsReady: false },
  )
})

test('pipeline available requires both the speak switch and TTS', () => {
  const host = new SpeechPipelineHost()
  assert.equal(host.speechEnabled, false)
  assert.equal(host.available, false)

  host.applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: false,
  })
  assert.equal(host.speechEnabled, false)
  assert.equal(host.available, false)

  host.applyStatus({
    available: true,
    tts_enabled: false,
    persona_speech_enabled: true,
  })
  assert.equal(host.speechEnabled, true)
  assert.equal(host.available, false)

  host.applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: true,
  })
  assert.equal(host.speechEnabled, true)
  assert.equal(host.available, true)
})

test('barge-in closes the old utterance to later tokens and the final-response fallback', () => {
  const host = new SpeechPipelineHost()
  host.applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: true,
  })
  const enqueued: string[] = []
  host.pipeline.enqueue = (segments) => {
    enqueued.push(...segments.map((segment) => segment.messageId))
  }
  const old = new SpeechSegmenter('old')
  host.feed(old.push('This is the first sentence. '))
  host.cancel()
  host.feed(old.push('This is the late second sentence. '))
  assert.equal(
    host.speakLine({ messageId: 'old', text: 'Late final reply.' }),
    true,
  )
  assert.equal(host.alreadyFed('old'), true)
  host.speakLine({ messageId: 'new', text: 'Fresh reply.' })
  assert.deepEqual(enqueued, ['old', 'new'])
})

test('turning speech off cancels active synthesis and playback', () => {
  const host = new SpeechPipelineHost()
  host.applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: true,
  })
  let cancelled = 0
  host.pipeline.cancel = () => {
    cancelled += 1
    return false
  }
  host.applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: false,
  })
  assert.equal(cancelled, 1)
  assert.equal(host.available, false)
})

test('targeted cancel does not overwrite a successor started synchronously by the queue', () => {
  const host = new SpeechPipelineHost()
  host.pipeline.cancel = () => {
    patchVoicePresence({ ttsPlaying: false })
    patchVoicePresence({ ttsPlaying: true })
    return true
  }
  try {
    host.cancel('old')
    assert.equal(getVoicePresence().ttsPlaying, true)
  } finally {
    patchVoicePresence({ ttsPlaying: false })
  }
})

test('old status response cannot enable speech after subject reset', async (t) => {
  invalidateSpeechStatusCache()
  t.after(invalidateSpeechStatusCache)
  const result = Promise.withResolvers<Response>()
  t.mock.method(globalThis, 'fetch', () => result.promise)
  const host = new SpeechPipelineHost()
  const pending = host.refreshStatus()
  host.resetSubject()
  result.resolve(
    Response.json({
      available: true,
      tts_enabled: true,
      persona_speech_enabled: true,
    }),
  )
  await pending
  assert.equal(host.available, false)
  assert.equal(host.speechEnabled, false)
})

test('settings status failures remain retryable without repeating a saved write', async (t) => {
  invalidateSpeechStatusCache()
  t.after(invalidateSpeechStatusCache)
  let fail = true
  t.mock.method(globalThis, 'fetch', async () => {
    if (fail) throw new Error('status unavailable')
    return Response.json({
      available: true,
      tts_enabled: true,
      persona_speech_enabled: true,
    })
  })
  const host = new SpeechPipelineHost()
  // Startup remains best effort; an explicit settings refresh must report failures.
  assert.equal(await host.probe(), false)
  let writes = 0
  const domain = createConfigDomain(() => ({
    id: 'speech',
    initial: false,
    ready: true,
    persist: async (value: boolean) => {
      writes++
      return value
    },
    effects: () => [
      {
        id: 'speech',
        run: async () => {
          await host.refreshStatus()
        },
      },
    ],
  }))
  domain.setDraft(true)
  const failed = await executeConfigOperations(
    [domain],
    [domain.prepareSave()!],
  )
  assert.equal(failed.errors.length, 1)
  assert.equal(domain.getSnapshot().pendingSync, true)
  assert.equal(domain.getSnapshot().dirty, false)
  fail = false
  assert.equal((await executeConfigOperations([domain], [])).errors.length, 0)
  assert.equal(domain.getSnapshot().pendingSync, false)
  assert.equal(host.available, true)
  assert.equal(writes, 1)
})

test('an explicitly disabled speech status is a successful refresh', async (t) => {
  invalidateSpeechStatusCache()
  t.after(invalidateSpeechStatusCache)
  t.mock.method(globalThis, 'fetch', async () =>
    Response.json({
      available: true,
      tts_enabled: true,
      persona_speech_enabled: false,
    }),
  )
  const host = new SpeechPipelineHost()
  assert.equal(await host.refreshStatus(), false)
  assert.equal(host.speechEnabled, false)
})

test('a hold needs something of hers to hold, and cancelling lets it go', () => {
  const host = new SpeechPipelineHost()
  host.applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: true,
  })
  assert.equal(host.hold(), false)
  assert.equal(host.held, false)

  host.pipeline.enqueue = () => {}
  Object.defineProperty(host.pipeline, 'playing', { get: () => true })
  host.pipeline.cancel = () => true
  assert.equal(host.hold(), true)
  assert.equal(host.held, true)
  host.resume()
  assert.equal(host.held, false)

  host.hold()
  host.cancel()
  assert.equal(host.held, false, 'the next voice must not wait on a cut off')
})

test('cutting her off reports where; turning speech off does not', () => {
  const host = new SpeechPipelineHost()
  host.applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: true,
  })
  const cuts: unknown[] = []
  const stop = host.onCutOff((cut) => cuts.push(cut))
  host.pipeline.cutOffPoint = () => ({ messageId: 'm', heardChars: 3 })
  host.pipeline.cancel = () => true
  host.cancel()
  assert.deepEqual(cuts, [{ messageId: 'm', heardChars: 3 }])
  host.applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: false,
  })
  assert.equal(cuts.length, 1)
  stop()
})
