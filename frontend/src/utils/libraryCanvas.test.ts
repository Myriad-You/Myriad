import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  buildCenterOutCanvasLayout,
  getLibraryCanvasFocusScale,
  getLibraryCanvasFocusScaleAt,
  getLibraryCanvasViewportBinKey,
  getLibraryCanvasViewportBounds,
  LIBRARY_CANVAS_FOCUS_MAX_SCALE,
  LIBRARY_CANVAS_FOCUS_MIN_SCALE,
  libraryCanvasLayoutIntersects,
} from './libraryCanvas'

describe('library canvas geometry', () => {
  it('places every item and anchors the first card at the world center', () => {
    const items = Array.from({ length: 500 }, (_, index) => ({
      id: String(index),
      size: index % 3 === 0 ? { w: 2, h: 1 } : { w: 1, h: 2 },
    }))
    const layouts = buildCenterOutCanvasLayout(items, (item) => item.size)

    assert.equal(layouts.size, items.length)
    const center = layouts.get('0')!
    assert.equal(center.left + center.width / 2, 0)
    assert.equal(center.top + center.height / 2, 0)
  })

  it('keeps occupied grid cells disjoint', () => {
    const items = Array.from({ length: 80 }, (_, index) => ({
      id: String(index),
      size: index % 4 === 0 ? { w: 2, h: 1 } : { w: 1, h: 1 },
    }))
    const layouts = buildCenterOutCanvasLayout(items, (item) => item.size)
    const cells = new Set<string>()

    layouts.forEach((layout) => {
      for (let x = 0; x < layout.gridW; x++) {
        for (let y = 0; y < layout.gridH; y++) {
          const key = `${layout.gridX + x},${layout.gridY + y}`
          assert.equal(cells.has(key), false, `overlapping cell ${key}`)
          cells.add(key)
        }
      }
    })
  })

  it('makes the viewport center largest and clamps distant cards', () => {
    const viewport = { width: 1000, height: 800 }
    const transform = { x: 0, y: 0, scale: 0.75 }
    const centered = {
      left: -100,
      top: -100,
      width: 200,
      height: 200,
      gridX: 0,
      gridY: 0,
      gridW: 1,
      gridH: 1,
    }
    const distant = { ...centered, left: 10000 }

    assert.equal(
      getLibraryCanvasFocusScale(centered, transform, viewport),
      LIBRARY_CANVAS_FOCUS_MAX_SCALE,
    )
    assert.equal(
      getLibraryCanvasFocusScale(distant, transform, viewport),
      LIBRARY_CANVAS_FOCUS_MIN_SCALE,
    )
    assert.equal(
      getLibraryCanvasFocusScaleAt(0, 0, transform, viewport),
      getLibraryCanvasFocusScale(centered, transform, viewport),
    )
  })

  it('computes viewport AABB and exact card intersection', () => {
    const bounds = getLibraryCanvasViewportBounds(
      { x: 0, y: 0, scale: 1 },
      { width: 1000, height: 800 },
      0,
    )
    const base = {
      top: -20,
      width: 40,
      height: 40,
      gridX: 0,
      gridY: 0,
      gridW: 1,
      gridH: 1,
    }
    assert.equal(
      libraryCanvasLayoutIntersects({ ...base, left: 490 }, bounds),
      true,
    )
    assert.equal(
      libraryCanvasLayoutIntersects({ ...base, left: 501 }, bounds),
      false,
    )
  })

  it('keeps viewport bin key stable for small pans inside the same bins', () => {
    const viewport = { width: 1000, height: 800 }
    const binSize = 800
    const a = getLibraryCanvasViewportBinKey(
      { x: 0, y: 0, scale: 1 },
      viewport,
      binSize,
      0,
    )
    const b = getLibraryCanvasViewportBinKey(
      { x: 40, y: -30, scale: 1 },
      viewport,
      binSize,
      0,
    )
    assert.equal(a, b)
    const c = getLibraryCanvasViewportBinKey(
      { x: 900, y: 0, scale: 1 },
      viewport,
      binSize,
      0,
    )
    assert.notEqual(a, c)
  })
})
