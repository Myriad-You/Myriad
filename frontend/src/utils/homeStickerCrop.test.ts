import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  clampStickerCrop,
  defaultStickerCrop,
  parseStickerCrop,
  stickerCropForSlot,
  stickerImageNeedsCrop,
  stickerSlotAspect,
} from './homeStickerCrop.ts'

describe('sticker crop', () => {
  it('clamps focal point and zoom', () => {
    assert.deepEqual(clampStickerCrop({ x: -1, y: 2, zoom: 0.2 }), {
      x: 0,
      y: 1,
      zoom: 1,
    })
    assert.equal(clampStickerCrop({ x: 0.5, y: 0.5, zoom: 9 }).zoom, 4)
  })

  it('parses stored crop and rejects junk', () => {
    assert.deepEqual(parseStickerCrop({ x: 0.2, y: 0.8, zoom: 1.5 }), {
      x: 0.2,
      y: 0.8,
      zoom: 1.5,
    })
    assert.equal(parseStickerCrop(null), null)
    assert.equal(parseStickerCrop({ x: 'a' }), null)
    assert.deepEqual(defaultStickerCrop(), { x: 0.5, y: 0.5, zoom: 1 })
  })

  it('flags aspect mismatch against the slot', () => {
    assert.equal(stickerSlotAspect('2x2'), 1)
    assert.equal(stickerSlotAspect('4x2'), 2)
    assert.equal(stickerImageNeedsCrop(100, 100, 1), false)
    assert.equal(stickerImageNeedsCrop(200, 100, 2), false)
    assert.equal(stickerImageNeedsCrop(100, 200, 1), true)
    assert.equal(stickerImageNeedsCrop(0, 10, 1), false)
    assert.equal(stickerCropForSlot(1024, 1024, '2x2'), undefined)
    assert.deepEqual(stickerCropForSlot(1536, 1024, '9x5'), defaultStickerCrop())
  })
})
