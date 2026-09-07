import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { isWallpaperMouseLeaveFromViewport } from './useEvocativeWallpaper.ts'

describe('isWallpaperMouseLeaveFromViewport', () => {
  it('treats a null relatedTarget as leaving the viewport', () => {
    assert.equal(isWallpaperMouseLeaveFromViewport(null), true)
  })

  it('ignores leave events that still point at a node in the document', () => {
    if (typeof document === 'undefined' || typeof Node === 'undefined') return
    const node = document.createElement('div')
    document.body.append(node)
    try {
      assert.equal(isWallpaperMouseLeaveFromViewport(node), false)
    } finally {
      node.remove()
    }
  })
})
