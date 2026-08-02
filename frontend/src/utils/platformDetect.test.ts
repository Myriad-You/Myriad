import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  detectIsWebKitEngine,
  detectOsKind,
  isAppleTouchDevice,
  isIpadOsDesktopUa,
  parseIosMajorVersion,
} from './platformDetect'

describe('detectOsKind', () => {
  it('detects android before linux', () => {
    assert.equal(
      detectOsKind(
        'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36',
        null,
      ),
      'android',
    )
  })

  it('detects iPhone', () => {
    assert.equal(
      detectOsKind(
        'Mozilla/5.0 (iPhone; CPU iPhone OS 18_2 like Mac OS X) AppleWebKit/605.1.15',
        null,
      ),
      'ios',
    )
  })

  it('detects windows', () => {
    assert.equal(
      detectOsKind('Mozilla/5.0 (Windows NT 10.0; Win64; x64)', null),
      'windows',
    )
  })
})

describe('parseIosMajorVersion', () => {
  it('parses iPhone OS version', () => {
    assert.equal(
      parseIosMajorVersion(
        'Mozilla/5.0 (iPhone; CPU iPhone OS 18_3_1 like Mac OS X)',
      ),
      18,
    )
  })

  it('parses older iOS', () => {
    assert.equal(
      parseIosMajorVersion(
        'Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X)',
      ),
      16,
    )
  })
})

describe('isAppleTouchDevice / iPad desktop UA', () => {
  it('detects classic iPhone UA', () => {
    assert.equal(
      isAppleTouchDevice(
        'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)',
        null,
      ),
      true,
    )
  })

  it('detects iPadOS desktop-style via touch points', () => {
    const fakeNav = {
      userAgent:
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15',
      platform: 'MacIntel',
      maxTouchPoints: 5,
      vendor: 'Apple Computer, Inc.',
    } as unknown as Navigator
    assert.equal(isIpadOsDesktopUa(undefined, fakeNav), true)
    assert.equal(isAppleTouchDevice(undefined, fakeNav), true)
    assert.equal(detectOsKind(undefined, fakeNav), 'ios')
  })
})

describe('detectIsWebKitEngine', () => {
  it('true for Safari-like UA + Apple vendor', () => {
    const nav = {
      userAgent:
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15',
      vendor: 'Apple Computer, Inc.',
    } as unknown as Navigator
    assert.equal(detectIsWebKitEngine(undefined, nav), true)
  })

  it('false for Chrome (has Chrome token)', () => {
    const nav = {
      userAgent:
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
      vendor: 'Google Inc.',
    } as unknown as Navigator
    assert.equal(detectIsWebKitEngine(undefined, nav), false)
  })
})
