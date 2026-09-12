import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  brewMotionClaim,
  brewMotionLane,
  brewMotionOwns,
  brewMotionQuiet,
  brewMotionRelease,
  brewMotionReset,
} from './brewMotion.ts'

describe('brewMotion', () => {
  it('后来的占位作废前一次', () => {
    brewMotionReset()
    const intro = brewMotionClaim('intro')
    const flip = brewMotionClaim('flip')
    assert.equal(brewMotionOwns(intro), false)
    assert.equal(brewMotionOwns(flip), true)
    assert.equal(brewMotionLane(), 'flip')
    brewMotionRelease(intro)
    assert.equal(brewMotionLane(), 'flip')
    brewMotionRelease(flip)
    assert.equal(brewMotionLane(), 'idle')
  })

  it('栏上换波次不占 lane，开合仍持有', () => {
    brewMotionReset()
    const flip = brewMotionClaim('flip')
    assert.equal(brewMotionLane(), 'flip')
    assert.equal(brewMotionOwns(flip), true)
    brewMotionRelease(flip)
    assert.equal(brewMotionLane(), 'idle')
  })

  it('换树占开合之后开合不能再收尾', () => {
    brewMotionReset()
    const flip = brewMotionClaim('flip')
    const lane = brewMotionClaim('lane')
    assert.equal(brewMotionOwns(flip), false)
    assert.equal(brewMotionOwns(lane), true)
    assert.equal(brewMotionLane(), 'lane')
    brewMotionRelease(lane)
    assert.equal(brewMotionLane(), 'idle')
  })

  it('入场被开合作废后不能再收 DOM', () => {
    brewMotionReset()
    const intro = brewMotionClaim('intro')
    const flip = brewMotionClaim('flip')
    assert.equal(brewMotionOwns(intro), false)
    assert.equal(brewMotionOwns(flip), true)
    brewMotionRelease(intro)
    assert.equal(brewMotionOwns(flip), true)
    brewMotionRelease(flip)
  })

  it('无 window 当静音', () => {
    assert.equal(brewMotionQuiet(), true)
  })
})
