import type { WidgetConfig } from './widgetGridTypes'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  homeGridPixelHeight,
  resolveHomeGridMetrics,
  widgetContentMaxRow,
} from './widgetGridMetrics'

function tile(
  id: string,
  size: WidgetConfig['size'],
  x: number,
  y: number,
): WidgetConfig {
  return { id, type: 'weather', size, position: { x, y } }
}

describe('widgetContentMaxRow', () => {
  it('uses the bottom edge of the lowest tile', () => {
    assert.equal(widgetContentMaxRow([]), 0)
    assert.equal(
      widgetContentMaxRow([tile('a', '2x2', 0, 0), tile('b', '4x2', 2, 3)]),
      5,
    )
  })
})

describe('resolveHomeGridMetrics', () => {
  const widgets = [tile('a', '2x2', 0, 0), tile('b', '4x2', 8, 0)]

  it('keeps the desktop board at 16×4', () => {
    assert.deepEqual(
      resolveHomeGridMetrics({
        widgets,
        layoutMode: 'standard',
        gridColumns: 16,
      }),
      {
        isCompact: false,
        currentWidgets: widgets,
        currentGridWidth: 16,
        currentGridHeight: 4,
      },
    )
  })

  it('packs compact boards and grows height to the packed shelf', () => {
    const compact = resolveHomeGridMetrics({
      widgets,
      layoutMode: 'standard',
      gridColumns: 8,
    })
    assert.equal(compact.isCompact, true)
    assert.equal(compact.currentGridWidth, 8)
    assert.ok(compact.currentGridHeight >= 4)
    assert.equal(compact.currentWidgets.length, 2)
  })

  it('uses the free board 16×8 regardless of window columns', () => {
    const free = resolveHomeGridMetrics({
      widgets,
      layoutMode: 'free',
      gridColumns: 8,
    })
    assert.deepEqual(free, {
      isCompact: false,
      currentWidgets: widgets,
      currentGridWidth: 16,
      currentGridHeight: 8,
    })
  })

  it('grows autoHeight to the content bottom, not below a custom floor', () => {
    const tall = resolveHomeGridMetrics({
      widgets: [tile('a', '2x2', 0, 6)],
      layoutMode: 'standard',
      gridColumns: 16,
      autoHeight: true,
      customGridRows: 3,
    })
    assert.equal(tall.currentGridHeight, 8)
    const floored = resolveHomeGridMetrics({
      widgets: [tile('a', '2x2', 0, 0)],
      layoutMode: 'standard',
      gridColumns: 16,
      autoHeight: true,
      customGridRows: 6,
    })
    assert.equal(floored.currentGridHeight, 6)
  })
})

describe('homeGridPixelHeight', () => {
  it('is square cells from container width, and skipped on free layout', () => {
    assert.equal(homeGridPixelHeight(1600, 16, 4, false), 400)
    assert.equal(homeGridPixelHeight(0, 16, 4, false), undefined)
    assert.equal(homeGridPixelHeight(1600, 16, 8, true), undefined)
  })
})
