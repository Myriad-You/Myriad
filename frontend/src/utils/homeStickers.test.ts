import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  normalizeHomeStickerPrompt,
  parseGenerateHomeStickerResponse,
} from './homeStickers.ts'

describe('homeStickers', () => {
  it('trims the prompt', () => {
    assert.equal(normalizeHomeStickerPrompt('  cat  '), 'cat')
    assert.equal(normalizeHomeStickerPrompt('\n'), '')
  })

  it('reads camelCase generate payload', () => {
    const parsed = parseGenerateHomeStickerResponse({
      imageUrl: '/api/brew/image-cache/aa/abcd.png',
      width: 512,
      height: 512,
    })
    assert.equal(parsed.imageUrl, '/api/brew/image-cache/aa/abcd.png')
    assert.equal(parsed.width, 512)
    assert.equal(parsed.height, 512)
  })

  it('rejects a payload without imageUrl', () => {
    assert.throws(() => parseGenerateHomeStickerResponse({ width: 512 }))
    assert.throws(() => parseGenerateHomeStickerResponse(null))
    assert.throws(() => parseGenerateHomeStickerResponse({ imageUrl: '  ' }))
  })

  it('reads an uploaded sticker the same way', () => {
    const parsed = parseGenerateHomeStickerResponse({
      imageUrl: '/api/brew/image-cache/bb/upload.png',
    })
    assert.equal(parsed.imageUrl, '/api/brew/image-cache/bb/upload.png')
  })
})
