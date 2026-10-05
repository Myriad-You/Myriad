import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { placeReaderTooltip } from './tooltipPlacement'

const phone = { width: 390, height: 844 }
const box = { width: 320, height: 90 }

describe('placeReaderTooltip', () => {
  it('keeps the whole box on screen for an annotation near the left edge', () => {
    // #625：390px 宽的手机，批注在左侧，原来框有约 90px 落在屏幕左边外。
    const p = placeReaderTooltip({ x: 60, top: 400, bottom: 420 }, box, phone, 8)
    assert.equal(p.left, 16)
    assert.ok(p.left + box.width <= phone.width - 16)
    assert.equal(p.side, 'above')
    assert.equal(p.top, 400 - 8 - 90)
    assert.equal(p.arrowX, 60 - 16)
  })

  it('keeps the box on screen for an annotation near the right edge', () => {
    const p = placeReaderTooltip({ x: 380, top: 400, bottom: 420 }, box, phone, 8)
    assert.equal(p.left, phone.width - 16 - box.width)
    assert.equal(p.arrowX, box.width - 12)
  })

  it('centers on the anchor when there is room', () => {
    const p = placeReaderTooltip(
      { x: 600, top: 400, bottom: 420 },
      box,
      { width: 1280, height: 800 },
      8,
    )
    assert.equal(p.left, 600 - 160)
    assert.equal(p.arrowX, 160)
  })

  it('flips below when the anchor is near the top', () => {
    const p = placeReaderTooltip({ x: 200, top: 40, bottom: 60 }, box, phone, 8)
    assert.equal(p.side, 'below')
    assert.equal(p.top, 68)
  })

  it('picks the roomier side when neither fits', () => {
    const tall = { width: 320, height: 500 }
    const nearTop = placeReaderTooltip({ x: 200, top: 300, bottom: 320 }, tall, phone, 8)
    assert.equal(nearTop.side, 'below')
    assert.ok(nearTop.top + tall.height <= phone.height - 16)
    const nearBottom = placeReaderTooltip({ x: 200, top: 520, bottom: 540 }, tall, phone, 8)
    assert.equal(nearBottom.side, 'above')
    assert.ok(nearBottom.top >= 16)
  })
})
