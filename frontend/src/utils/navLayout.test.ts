import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  NAV_DESKTOP_MIN_WIDTH,
  NAV_MOBILE_MAX_WIDTH,
  resolveNavLayout,
} from './navLayout'

describe('resolveNavLayout', () => {
  it('uses mobile for phone widths regardless of pointer', () => {
    assert.equal(
      resolveNavLayout({
        width: NAV_MOBILE_MAX_WIDTH,
        coarsePointer: false,
        appleTouch: false,
      }),
      'mobile',
    )
    assert.equal(
      resolveNavLayout({
        width: 390,
        coarsePointer: true,
        appleTouch: true,
      }),
      'mobile',
    )
  })

  it('uses desktop for wide viewports regardless of pointer', () => {
    assert.equal(
      resolveNavLayout({
        width: NAV_DESKTOP_MIN_WIDTH,
        coarsePointer: true,
        appleTouch: true,
      }),
      'desktop',
    )
    assert.equal(
      resolveNavLayout({
        width: 1440,
        coarsePointer: false,
        appleTouch: false,
      }),
      'desktop',
    )
  })

  it('tablet band: touch / Apple → mobile bottom island', () => {
    // iPad portrait-ish
    assert.equal(
      resolveNavLayout({
        width: 820,
        coarsePointer: true,
        appleTouch: true,
      }),
      'mobile',
    )
    assert.equal(
      resolveNavLayout({
        width: 768,
        coarsePointer: true,
        appleTouch: false,
      }),
      'mobile',
    )
    // Apple touch without coarse (rare desktop-UA iPad) still mobile
    assert.equal(
      resolveNavLayout({
        width: 834,
        coarsePointer: false,
        appleTouch: true,
      }),
      'mobile',
    )
  })

  it('tablet band: fine pointer desktop window → side rail', () => {
    assert.equal(
      resolveNavLayout({
        width: 900,
        coarsePointer: false,
        appleTouch: false,
      }),
      'desktop',
    )
    assert.equal(
      resolveNavLayout({
        width: 1023,
        coarsePointer: false,
        appleTouch: false,
      }),
      'desktop',
    )
  })
})
