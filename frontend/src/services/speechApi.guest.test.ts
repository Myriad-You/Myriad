import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { setKnownAuthState } from '../utils/authState.ts'
import {
  getSpeechStatus,
  getVoiceList,
  invalidateSpeechStatusCache,
  subscribeSpeechStatus,
} from './speechApi.ts'

describe('speech guest contract', () => {
  const originalFetch = globalThis.fetch

  afterEach(() => {
    globalThis.fetch = originalFetch
    setKnownAuthState(true)
    invalidateSpeechStatusCache()
  })

  it('does not fetch /api/speech/status or /voices for a known guest', async () => {
    setKnownAuthState(false)
    let called = 0
    globalThis.fetch = (async () => {
      called++
      return new Response('{}', { status: 200 })
    }) as typeof fetch

    const status = await getSpeechStatus()
    const voices = await getVoiceList()
    assert.equal(status.available, false)
    assert.equal(status.tts_enabled, false)
    assert.equal(status.asr_enabled, false)
    assert.deepEqual(voices, { voices: [] })
    assert.equal(called, 0)
  })

  it('still fetches when auth state is not a known guest', async () => {
    setKnownAuthState(true)
    let called = 0
    globalThis.fetch = (async () => {
      called++
      return new Response(
        JSON.stringify({
          available: true,
          tts_enabled: true,
          asr_enabled: false,
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      )
    }) as typeof fetch

    const status = await getSpeechStatus()
    assert.equal(status.available, true)
    assert.equal(called, 1)
  })
})

describe('speech status readers', () => {
  it('hears when settings drop the cached status, until they stop listening', () => {
    let heard = 0
    const stop = subscribeSpeechStatus(() => heard++)
    invalidateSpeechStatusCache()
    assert.equal(heard, 1)
    stop()
    invalidateSpeechStatusCache()
    assert.equal(heard, 1)
  })
})
