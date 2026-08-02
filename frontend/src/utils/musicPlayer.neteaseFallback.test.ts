import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  getNeteaseAudioUrlImmediate,
  getNeteasePlayUrl,
  getNeteaseProxyAudioUrl,
  getNeteaseProxyFallbackUrl,
  isNeteaseDirectPlayUrl,
} from './musicPlayer'

describe('netease play-url / proxy fallback (plan B+C)', () => {
  it('detects play-url as direct and audio proxy as not', () => {
    const id = '12345'
    assert.equal(isNeteaseDirectPlayUrl(getNeteasePlayUrl(id)), true)
    assert.equal(isNeteaseDirectPlayUrl(getNeteaseProxyAudioUrl(id)), false)
  })

  it('detects legacy outer and CDN hosts as direct', () => {
    assert.equal(
      isNeteaseDirectPlayUrl(
        'https://music.163.com/song/media/outer/url?id=1.mp3',
      ),
      true,
    )
    assert.equal(
      isNeteaseDirectPlayUrl('https://m801.music.126.net/foo.mp3'),
      true,
    )
  })

  it('returns proxy fallback only for netease direct urls', () => {
    const id = '999'
    const direct = {
      id,
      source: 'netease' as const,
      url: getNeteasePlayUrl(id),
    }
    assert.equal(getNeteaseProxyFallbackUrl(direct), getNeteaseProxyAudioUrl(id))

    const alreadyProxy = {
      id,
      source: 'netease' as const,
      url: getNeteaseProxyAudioUrl(id),
    }
    assert.equal(getNeteaseProxyFallbackUrl(alreadyProxy), null)

    const qq = {
      id: 'mid',
      source: 'qq' as const,
      url: 'https://example/qq',
    }
    assert.equal(getNeteaseProxyFallbackUrl(qq), null)
  })

  it('getNeteaseAudioUrlImmediate is sync and returns a playable path', () => {
    const id = '4242'
    const url = getNeteaseAudioUrlImmediate(id)
    assert.equal(typeof url, 'string')
    assert.match(url, /netease/)
    assert.match(url, new RegExp(id))
  })
})
