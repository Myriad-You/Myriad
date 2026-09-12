import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  ensureSpectrumSafePlaybackUrl,
  getNeteaseAudioUrlImmediate,
  getNeteaseGeoPlaybackUrl,
  getNeteasePlayUrl,
  getNeteaseProxyAudioUrl,
  getNeteaseProxyFallbackUrl,
  getQQAudioUrlImmediate,
  getQQGeoPlaybackUrl,
  getQQPlayUrl,
  getQQProxyAudioUrl,
  isNeteaseDirectPlayUrl,
  isQQDirectPlayUrl,
  isWebAudioUnsafeMediaUrl,
  prefersSameOriginMusicProxy,
  withSpectrumSafePlaybackUrl,
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
    assert.match(url, new RegExp(RegExp.escape(id)))
  })
})

describe('desktop Web Audio spectrum CORS (play-url → same-origin /audio/)', () => {
  it('prefers same-origin proxy in this test environment (desktop-like)', () => {
    assert.equal(prefersSameOriginMusicProxy(), true)
  })

  it('marks play-url and CDN as Web Audio unsafe; /audio/ as safe', () => {
    const id = '55'
    assert.equal(isWebAudioUnsafeMediaUrl(getNeteasePlayUrl(id)), true)
    assert.equal(isWebAudioUnsafeMediaUrl(getNeteaseProxyAudioUrl(id)), false)
    assert.equal(isWebAudioUnsafeMediaUrl(getQQPlayUrl(id)), true)
    assert.equal(isWebAudioUnsafeMediaUrl(getQQProxyAudioUrl(id)), false)
    assert.equal(
      isWebAudioUnsafeMediaUrl('https://m801.music.126.net/x.mp3'),
      true,
    )
    assert.equal(
      isWebAudioUnsafeMediaUrl('https://dl.stream.qqmusic.qq.com/x.m4a'),
      true,
    )
  })

  it('geo helpers use full proxy when same-origin is preferred (desktop), even in China', () => {
    const id = '777'
    assert.equal(
      getNeteaseGeoPlaybackUrl(id, true),
      getNeteaseProxyAudioUrl(id),
    )
    assert.equal(getQQGeoPlaybackUrl(id, true), getQQProxyAudioUrl(id))
    assert.equal(
      getNeteaseGeoPlaybackUrl(id, false),
      getNeteaseProxyAudioUrl(id),
    )
    assert.equal(getQQGeoPlaybackUrl(id, false), getQQProxyAudioUrl(id))
  })

  it('immediate URLs use /audio/ when same-origin is preferred', () => {
    const id = '888'
    assert.equal(getNeteaseAudioUrlImmediate(id), getNeteaseProxyAudioUrl(id))
    assert.equal(getQQAudioUrlImmediate(id), getQQProxyAudioUrl(id))
  })

  it('ensureSpectrumSafePlaybackUrl upgrades cached play-url to /audio/', () => {
    const id = '42'
    const neteaseDirect = {
      id,
      source: 'netease' as const,
      url: getNeteasePlayUrl(id),
    }
    assert.equal(
      ensureSpectrumSafePlaybackUrl(neteaseDirect),
      getNeteaseProxyAudioUrl(id),
    )

    const qqDirect = {
      id: 'mid42',
      source: 'qq' as const,
      url: getQQPlayUrl('mid42'),
    }
    assert.equal(
      ensureSpectrumSafePlaybackUrl(qqDirect),
      getQQProxyAudioUrl('mid42'),
    )

    const already = {
      id,
      source: 'netease' as const,
      url: getNeteaseProxyAudioUrl(id),
    }
    assert.equal(
      ensureSpectrumSafePlaybackUrl(already),
      getNeteaseProxyAudioUrl(id),
    )
  })

  it('withSpectrumSafePlaybackUrl rewrites song.url immutably', () => {
    const song = {
      id: '1',
      name: 't',
      artist: 'a',
      album: '',
      cover: '',
      url: getNeteasePlayUrl('1'),
      duration: 1,
      source: 'netease' as const,
    }
    const next = withSpectrumSafePlaybackUrl(song)
    assert.notEqual(next, song)
    assert.equal(next.url, getNeteaseProxyAudioUrl('1'))
    assert.equal(song.url, getNeteasePlayUrl('1'))
  })

  it('detects QQ play-url as direct and audio proxy as not', () => {
    const mid = '003xxx'
    assert.equal(isQQDirectPlayUrl(getQQPlayUrl(mid)), true)
    assert.equal(isQQDirectPlayUrl(getQQProxyAudioUrl(mid)), false)
  })
})
