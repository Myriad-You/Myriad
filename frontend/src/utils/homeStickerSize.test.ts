import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  placeHomeStickerSelection,
  snapHomeStickerSize,
  stickerAspectKey,
  stickerPixelSize,
  stickerSizesSharingAspect,
} from './homeStickerSize.ts'

describe('placeHomeStickerSelection', () => {
  it('keeps the drawn cells', () => {
    assert.deepEqual(placeHomeStickerSelection(2, 1, 10, 6), {
      size: '10x6',
      x: 2,
      y: 1,
    })
    assert.deepEqual(placeHomeStickerSelection(0, 0, 5, 5), {
      size: '5x5',
      x: 0,
      y: 0,
    })
  })
})

describe('snapHomeStickerSize', () => {
  it('names the closest fitting preset for generate ratio only', () => {
    assert.equal(snapHomeStickerSize(4, 3), '4x3')
    assert.equal(snapHomeStickerSize(9, 5), '9x5')
    assert.equal(snapHomeStickerSize(10, 6), '9x5')
    assert.equal(snapHomeStickerSize(5, 5), '4x4')
  })
})

describe('stickerSizesSharingAspect', () => {
  it('scales the same reduced ratio only', () => {
    assert.deepEqual(stickerSizesSharingAspect('2x2'), [
      '1x1',
      '2x2',
      '3x3',
      '4x4',
      '5x5',
      '6x6',
      '7x7',
      '8x8',
    ])
    assert.deepEqual(stickerSizesSharingAspect('3x2'), [
      '3x2',
      '6x4',
      '9x6',
      '12x8',
    ])
    assert.deepEqual(stickerSizesSharingAspect('4x3'), ['4x3', '8x6'])
    assert.deepEqual(stickerSizesSharingAspect('10x6'), ['5x3', '10x6'])
  })
})

describe('stickerPixelSize', () => {
  it('picks the nearest 1:1 / 3:2 / 2:3 generate frame', () => {
    assert.deepEqual(stickerPixelSize('2x2'), { width: 1024, height: 1024 })
    assert.deepEqual(stickerPixelSize('3x2'), { width: 1536, height: 1024 })
    assert.deepEqual(stickerPixelSize('2x3'), { width: 1024, height: 1536 })
    assert.deepEqual(stickerPixelSize('10x6'), { width: 1536, height: 1024 })
    assert.deepEqual(stickerPixelSize('4x7'), { width: 1024, height: 1536 })
  })
})
