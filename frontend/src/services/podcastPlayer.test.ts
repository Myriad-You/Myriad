import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { PodcastPlayer } from './brewliaApi.ts'

function voice(lang: string, name = lang): SpeechSynthesisVoice {
  return { lang, name } as SpeechSynthesisVoice
}

describe('PodcastPlayer.filterVoicesByLanguage', () => {
  const voices = [
    voice('zh-CN', 'Microsoft Huihui'),
    voice('zh-TW', 'Microsoft Hanhan'),
    voice('en-US', 'Samantha'),
  ]

  it('prefers Traditional voices for zh-TW', () => {
    assert.deepEqual(
      PodcastPlayer.filterVoicesByLanguage(voices, 'zh-TW').map((item) => item.lang),
      ['zh-TW'],
    )
  })

  it('falls back to other zh voices when no Traditional pack exists', () => {
    assert.deepEqual(
      PodcastPlayer.filterVoicesByLanguage(
        [voice('zh-CN', 'Microsoft Huihui'), voice('en-US', 'Samantha')],
        'zh-TW',
      ).map((item) => item.lang),
      ['zh-CN'],
    )
  })

  it('still matches the language prefix for zh-CN', () => {
    assert.deepEqual(
      PodcastPlayer.filterVoicesByLanguage(voices, 'zh-CN').map((item) => item.lang),
      ['zh-CN', 'zh-TW'],
    )
  })
})
