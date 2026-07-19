/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  federationMediaUrlRejectionReason,
  isValidFederationMediaUrl,
  parseFederationMediaUrl,
} from './federationMediaUrl'

describe('isValidFederationMediaUrl', () => {
  it('accepts local federation media paths', () => {
    assert.equal(
      isValidFederationMediaUrl(
        'https://example.com/media/federation/1/abc-def_01.jpg',
      ),
      true,
    )
    assert.equal(
      isValidFederationMediaUrl(
        'http://localhost:8080/media/federation/42/uuid.mp4',
      ),
      true,
    )
  })

  it('rejects empty and non-strings', () => {
    assert.equal(isValidFederationMediaUrl(''), false)
    assert.equal(isValidFederationMediaUrl('   '), false)
    assert.equal(isValidFederationMediaUrl(null), false)
    assert.equal(isValidFederationMediaUrl(undefined), false)
    assert.equal(isValidFederationMediaUrl(123), false)
  })

  it('rejects wrong host path shapes', () => {
    assert.equal(
      isValidFederationMediaUrl('https://evil.com/uploads/1/abc.jpg'),
      false,
    )
    assert.equal(
      isValidFederationMediaUrl(
        'https://example.com/media/federation/1/../2/x.jpg',
      ),
      false,
    )
    assert.equal(
      isValidFederationMediaUrl('https://example.com/media/federation/1/'),
      false,
    )
    assert.equal(
      isValidFederationMediaUrl(
        'https://example.com/media/federation/1/bad name.jpg',
      ),
      false,
    )
    assert.equal(
      isValidFederationMediaUrl(
        'https://example.com/media/federation/not-a-number/x.jpg',
      ),
      false,
    )
    assert.equal(
      isValidFederationMediaUrl('data:image/png;base64,aaaa'),
      false,
    )
  })
})

describe('parseFederationMediaUrl', () => {
  it('extracts userId and filename', () => {
    const parts = parseFederationMediaUrl(
      'https://example.com/media/federation/7/photo.webp',
    )
    assert.deepEqual(parts, {
      userId: 7,
      filename: 'photo.webp',
      origin: 'https://example.com',
    })
  })
})

describe('federationMediaUrlRejectionReason', () => {
  it('returns null for valid urls and a reason otherwise', () => {
    assert.equal(
      federationMediaUrlRejectionReason(
        'https://example.com/media/federation/1/a.jpg',
      ),
      null,
    )
    assert.equal(
      federationMediaUrlRejectionReason(''),
      'Attachment URL is empty',
    )
    assert.match(
      federationMediaUrlRejectionReason('https://x/y') || '',
      /media\/federation/,
    )
  })
})
