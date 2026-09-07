import type { MeropeSpeechEventDetail } from '../speechEvents'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  liveMotionGeneration,
  setLiveMotionGeneration,
} from '../motion/liveGeneration'
import { MEROPE_SPEECH_EVENT } from '../speechEvents'
import { personaSpeechFlags, SpeechPipelineHost } from './speechPipelineHost'
import { SpeechSegmenter } from './speechSegmenter'
import { getVoicePresence, patchVoicePresence } from './voicePresence'

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

test('queued speech keeps its original source and generation through playback and cancellation', () => {
  const savedWindow = globalThis.window
  const savedGeneration = liveMotionGeneration()
  const events: MeropeSpeechEventDetail[] = []
  const target = new EventTarget()
  target.addEventListener(MEROPE_SPEECH_EVENT, (event) =>
    events.push((event as CustomEvent<MeropeSpeechEventDetail>).detail),
  )
  globalThis.window = target as unknown as Window & typeof globalThis
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
      // eslint-disable-next-line dot-notation
      const handle = host['play'](new ArrayBuffer(0), segment, () => {})
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
    globalThis.window = savedWindow
    setLiveMotionGeneration(savedGeneration)
    patchVoicePresence({ ttsPlaying: false })
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
