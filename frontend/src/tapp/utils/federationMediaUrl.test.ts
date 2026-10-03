import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  federationMediaUrlRejectionReason,
  isValidFederationMediaUrl,
} from './federationMediaUrl'

const ASSET = '3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708'

describe('isValidFederationMediaUrl', () => {
  it('accepts what the upload returns', () => {
    assert.equal(
      isValidFederationMediaUrl(`https://example.com/media/assets/${ASSET}/abc-def_01.jpg`),
      true,
    )
    assert.equal(
      isValidFederationMediaUrl(`http://localhost:8080/media/assets/${ASSET}/clip.mp4`),
      true,
    )
    assert.equal(isValidFederationMediaUrl('https://example.com/api/media/7/content'), true)
  })

  it('rejects empty and non-strings', () => {
    assert.equal(isValidFederationMediaUrl(''), false)
    assert.equal(isValidFederationMediaUrl('   '), false)
    assert.equal(isValidFederationMediaUrl(null), false)
    assert.equal(isValidFederationMediaUrl(undefined), false)
    assert.equal(isValidFederationMediaUrl(123), false)
  })

  it('rejects other path shapes', () => {
    for (const url of [
      'https://evil.com/uploads/1/abc.jpg',
      'https://example.com/media/federation/1/abc.jpg',
      `https://example.com/media/assets/${ASSET}/../x.jpg`,
      `https://example.com/media/assets/${ASSET}/`,
      `https://example.com/media/assets/${ASSET}/bad name.jpg`,
      'https://example.com/media/assets/not-a-uuid/x.jpg',
      'https://example.com/api/media/0/content',
      `/media/assets/${ASSET}/a.jpg`,
      'data:image/png;base64,aaaa',
    ]) {
      assert.equal(isValidFederationMediaUrl(url), false, url)
    }
  })
})

describe('federationMediaUrlRejectionReason', () => {
  it('returns null for valid urls and a reason otherwise', () => {
    assert.equal(
      federationMediaUrlRejectionReason(`https://example.com/media/assets/${ASSET}/a.jpg`),
      null,
    )
    assert.equal(federationMediaUrlRejectionReason(''), 'Invalid attachment URL')
    assert.equal(federationMediaUrlRejectionReason('https://x/y'), 'Invalid attachment URL')
  })
})
