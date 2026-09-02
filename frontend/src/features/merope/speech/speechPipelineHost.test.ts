import assert from 'node:assert/strict'
import test from 'node:test'
import { personaSpeechFlags, SpeechPipelineHost } from './speechPipelineHost'

test('persona speech is off unless the status flag is on', () => {
  assert.deepEqual(
    personaSpeechFlags({ available: true, tts_enabled: true }),
    { speechEnabled: false, ttsReady: true },
  )
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
