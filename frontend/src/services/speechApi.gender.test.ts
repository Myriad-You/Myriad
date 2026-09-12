import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isFemaleVoice,
  isMaleVoice,
  localizedVoiceDescription,
} from './speechApi.ts'

describe('voice gender contract', () => {
  it('accepts machine values and leftover Chinese labels', () => {
    assert.equal(isMaleVoice('male'), true)
    assert.equal(isMaleVoice('boy'), true)
    assert.equal(isMaleVoice('男'), true)
    assert.equal(isMaleVoice('男童'), true)
    assert.equal(isMaleVoice('female'), false)
    assert.equal(isFemaleVoice('female'), true)
    assert.equal(isFemaleVoice('girl'), true)
    assert.equal(isFemaleVoice('女'), true)
    assert.equal(isFemaleVoice('女童'), true)
    assert.equal(isFemaleVoice('male'), false)
  })

  it('uses the catalog description when the voice id is present', () => {
    assert.equal(
      localizedVoiceDescription(
        { 'voiceDesc.502006': 'Natural male chat voice' },
        { id: 502006, description: '聊天男声，自然流畅' },
      ),
      'Natural male chat voice',
    )
    assert.equal(
      localizedVoiceDescription(
        {},
        { id: 1, description: 'fallback' },
      ),
      'fallback',
    )
  })
})
