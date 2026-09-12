import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  computeThumbLayout,
  grabOffsetFromThumbPointer,
  scrollTopFromThumb,
  thumbTopFromPointer,
} from './customScrollbarMetrics'

describe('computeThumbLayout', () => {
  it('hides geometry when the page does not overflow', () => {
    const layout = computeThumbLayout({
      windowHeight: 800,
      documentHeight: 800,
      scrollTop: 0,
    })
    assert.equal(layout.scrollableHeight, 0)
    assert.equal(layout.availableTrackHeight, 0)
    assert.equal(layout.thumbTop, 0)
  })

  it('maps mid-page scroll to the middle of the track', () => {
    const layout = computeThumbLayout({
      windowHeight: 1000,
      documentHeight: 3000,
      scrollTop: 1000,
    })
    assert.equal(layout.scrollableHeight, 2000)
    assert.equal(layout.percentage, 0.5)
    assert.ok(Math.abs(layout.thumbTop - layout.availableTrackHeight / 2) < 0.001)
  })
})

describe('scrollbar drag mapping', () => {
  it('keeps the grabbed point under the pointer (1:1 follow)', () => {
    const metrics = {
      trackTop: 200,
      grabOffsetY: 20,
      availableTrackHeight: 260,
    }

    assert.equal(thumbTopFromPointer(220, metrics), 0)
    assert.equal(thumbTopFromPointer(350, metrics), 130)
    assert.equal(thumbTopFromPointer(900, metrics), 260)
  })

  it('does not slip when document height grows after lock (library infinite load)', () => {
    const locked = {
      trackTop: 350,
      grabOffsetY: 16,
      availableTrackHeight: 200,
      scrollableHeight: 4000,
    }

    const thumbTop = thumbTopFromPointer(466, locked)
    assert.equal(thumbTop, 100)

    const scrollTop = scrollTopFromThumb(
      thumbTop,
      locked.availableTrackHeight,
      locked.scrollableHeight,
    )
    assert.equal(scrollTop, 2000)

    // In-drag mapping must not follow live page growth.
    const liveScrollableHeight = 9000
    const stillLocked = scrollTopFromThumb(
      thumbTop,
      locked.availableTrackHeight,
      locked.scrollableHeight,
    )
    const ifRemapped = scrollTopFromThumb(
      thumbTop,
      locked.availableTrackHeight,
      liveScrollableHeight,
    )
    assert.equal(stillLocked, 2000)
    assert.notEqual(ifRemapped, stillLocked)
  })

  it('clamps a track-click grab to the thumb', () => {
    assert.equal(grabOffsetFromThumbPointer(120, 100, 40), 20)
    assert.equal(grabOffsetFromThumbPointer(80, 100, 40), 0)
    assert.equal(grabOffsetFromThumbPointer(200, 100, 40), 40)
  })
})
