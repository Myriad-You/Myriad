import type { WidgetConfig } from './widgetGridTypes'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  commitWidgetResize,
  nearestResizeSize,
  resizeDraftAllowed,
  resizeRawSpan,
} from './widgetGridResize'

const tile: WidgetConfig = {
  id: 'a',
  type: 'weather',
  size: '2x2',
  position: { x: 0, y: 0 },
}

describe('nearestResizeSize', () => {
  it('picks the closest supported size to the pointer span', () => {
    assert.equal(
      nearestResizeSize({
        currentSize: '2x2',
        startSize: '2x2',
        direction: 'se',
        rawW: 3.8,
        rawH: 2.1,
        supportedSizes: ['2x2', '4x2', '4x4'],
      }),
      '4x2',
    )
  })

  it('keeps width when resizing from the south edge', () => {
    assert.equal(
      nearestResizeSize({
        currentSize: '2x2',
        startSize: '2x2',
        direction: 's',
        rawW: 8,
        rawH: 3.8,
        supportedSizes: ['2x2', '2x4', '4x4'],
      }),
      '2x4',
    )
  })
})

describe('resizeDraftAllowed', () => {
  it('rejects a draft that overlaps another tile or leaves the board', () => {
    const other: WidgetConfig = {
      id: 'b',
      type: 'weather',
      size: '2x2',
      position: { x: 2, y: 0 },
    }
    assert.equal(
      resizeDraftAllowed({
        widget: tile,
        draftSize: '4x2',
        widgets: [tile, other],
        gridWidth: 16,
        gridHeight: 4,
        isFreeLayout: false,
      }),
      false,
    )
    assert.equal(
      resizeDraftAllowed({
        widget: tile,
        draftSize: '4x2',
        widgets: [tile],
        gridWidth: 16,
        gridHeight: 4,
        isFreeLayout: false,
      }),
      true,
    )
  })
})

describe('commitWidgetResize', () => {
  it('returns null when the size did not change', () => {
    assert.equal(commitWidgetResize([tile], 'a', '2x2'), null)
    assert.deepEqual(commitWidgetResize([tile], 'a', '4x2')?.[0].size, '4x2')
  })
})

describe('resizeRawSpan', () => {
  it('converts the pointer into cell spans from the tile origin', () => {
    assert.deepEqual(
      resizeRawSpan({
        pointer: { x: 400, y: 200 },
        widget: tile,
        gridRect: { left: 0, top: 0, width: 1600, height: 400 },
        gridWidth: 16,
        gridHeight: 4,
        startSize: '2x2',
        direction: 'se',
      }),
      { rawW: 4, rawH: 2 },
    )
    assert.deepEqual(
      resizeRawSpan({
        pointer: { x: 800, y: 200 },
        widget: tile,
        gridRect: { left: 0, top: 0, width: 1600, height: 400 },
        gridWidth: 16,
        gridHeight: 4,
        startSize: '2x2',
        direction: 's',
      }),
      { rawW: 2, rawH: 2 },
    )
  })
})
