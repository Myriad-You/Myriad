import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  getPreviewCanvas,
  previewTransform,
  STORE_PREVIEW_HEIGHT,
  STORE_PREVIEW_WIDTH,
} from './tappStorePreview.ts'

describe('tappStorePreview helpers', () => {
  it('defaults canvas when preview missing', () => {
    assert.deepEqual(getPreviewCanvas(null), {
      width: STORE_PREVIEW_WIDTH,
      height: STORE_PREVIEW_HEIGHT,
      fit: 'cover',
      focus: { x: 0.5, y: 0.5 },
      theme: 'auto',
    })
  })

  it('cover scale fills host; contain fits inside', () => {
    const canvas = {
      width: 1280,
      height: 720,
      fit: 'cover' as const,
      focus: { x: 0.5, y: 0.5 },
      theme: 'auto' as const,
    }
    const cover = previewTransform(640, 360, canvas)
    assert.equal(cover.scale, 0.5)

    const contain = previewTransform(640, 200, {
      ...canvas,
      fit: 'contain',
    })
    assert.ok(Math.abs(contain.scale - 200 / 720) < 1e-9)
  })
})
