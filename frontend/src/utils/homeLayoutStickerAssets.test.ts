import type { HomeLayoutAssetMap } from './homeLayoutTransfer'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createHomeStickerItem } from './homeLayout'
import {
  fetchStickerAssets,
  restoreStickerAssets,
} from './homeLayoutStickerAssets'
import {
  encodeBase64,

} from './homeLayoutTransfer'

const PNG_1X1 = Uint8Array.from([
  0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
  0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
  0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
  0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
  0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
  0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
])
const STICKER_HASH = `ab${'a'.repeat(62)}`
const STICKER_PATH = `/api/brew/image-cache/ab/${STICKER_HASH}.png`

function layoutsWith(url: string) {
  return {
    standard: [],
    free: [
      createHomeStickerItem({
        size: '2x2',
        position: { x: 0, y: 0 },
        imageUrl: url,
        prompt: 'cat',
      }),
    ],
  }
}

describe('fetchStickerAssets', () => {
  it('embeds PNG bytes from the image-cache path', async () => {
    const fetched: string[] = []
    const { assets, missing } = await fetchStickerAssets(
      layoutsWith(STICKER_PATH),
      async (input) => {
        fetched.push(String(input))
        return {
          ok: true,
          arrayBuffer: async () =>
            PNG_1X1.buffer.slice(
              PNG_1X1.byteOffset,
              PNG_1X1.byteOffset + PNG_1X1.byteLength,
            ),
        }
      },
    )
    assert.deepEqual(fetched, [STICKER_PATH])
    assert.deepEqual(missing, [])
    assert.equal(assets[STICKER_PATH]?.mime, 'image/png')
    assert.equal(assets[STICKER_PATH]?.data, encodeBase64(PNG_1X1))
  })

  it('embeds WebP bytes from the image-cache path', async () => {
    const webp = Uint8Array.from([
      0x52, 0x49, 0x46, 0x46, 0x08, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50,
    ])
    const path = `/api/brew/image-cache/ab/${STICKER_HASH}.webp`
    const { assets, missing } = await fetchStickerAssets(
      layoutsWith(path),
      async () => ({
        ok: true,
        arrayBuffer: async () =>
          webp.buffer.slice(webp.byteOffset, webp.byteOffset + webp.byteLength),
      }),
    )
    assert.deepEqual(missing, [])
    assert.equal(assets[path]?.mime, 'image/webp')
    assert.equal(assets[path]?.data, encodeBase64(webp))
  })

  it('skips non-cache URLs and non-image bytes', async () => {
    const skipped = await fetchStickerAssets(layoutsWith('/media/sticker.png'))
    assert.deepEqual(skipped.assets, {})
    assert.deepEqual(skipped.missing, ['/media/sticker.png'])

    const { assets, missing } = await fetchStickerAssets(
      layoutsWith(STICKER_PATH),
      async () => ({
        ok: true,
        arrayBuffer: async () => new Uint8Array([0x00, 0x01, 0x02]).buffer,
      }),
    )
    assert.deepEqual(assets, {})
    assert.deepEqual(missing, [STICKER_PATH])
  })
})

describe('restoreStickerAssets', () => {
  it('re-stores once per URL and rewrites tiles', async () => {
    const assets: HomeLayoutAssetMap = {
      [STICKER_PATH]: { mime: 'image/png', data: encodeBase64(PNG_1X1) },
    }
    const uploaded: string[] = []
    const first = createHomeStickerItem({
      size: '2x2',
      position: { x: 0, y: 0 },
      imageUrl: STICKER_PATH,
      prompt: 'a',
    })
    const second = createHomeStickerItem({
      size: '2x2',
      position: { x: 2, y: 0 },
      imageUrl: STICKER_PATH,
      prompt: 'b',
    })
    const result = await restoreStickerAssets(
      { standard: [], free: [first, second] },
      assets,
      async (dataUrl) => {
        uploaded.push(dataUrl.slice(0, 22))
        return '/api/brew/image-cache/cd/restored.png'
      },
    )
    assert.equal(uploaded.length, 1)
    assert.equal(uploaded[0], 'data:image/png;base64,')
    assert.equal(result.restored, 1)
    assert.deepEqual(result.failed, [])
    assert.equal(
      result.layouts.free[0]?.config?.imageUrl,
      '/api/brew/image-cache/cd/restored.png',
    )
    assert.equal(
      result.layouts.free[1]?.config?.imageUrl,
      '/api/brew/image-cache/cd/restored.png',
    )
  })

  it('strips unrestored inline images and keeps ordinary paths', async () => {
    const inline = createHomeStickerItem({
      size: '2x2',
      position: { x: 0, y: 0 },
      imageUrl: 'inline:sticker_1',
      prompt: 'x',
    })
    const remote = createHomeStickerItem({
      size: '2x2',
      position: { x: 2, y: 0 },
      imageUrl: '/media/keep.png',
      prompt: 'y',
    })
    const result = await restoreStickerAssets(
      { standard: [], free: [inline, remote] },
      {},
      async () => {
        throw new Error('should not upload')
      },
    )
    assert.equal(result.restored, 0)
    assert.equal(result.layouts.free[0]?.config?.imageUrl, undefined)
    assert.equal(result.layouts.free[1]?.config?.imageUrl, '/media/keep.png')
  })
})
