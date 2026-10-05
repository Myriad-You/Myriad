import type { WidgetConfig } from './widgetGridTypes'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  homeGridPixelHeight,
  resolveHomeGridMetrics,
  widgetContentMaxRow,
  widgetsInReadingOrder,
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

describe('widgetsInReadingOrder', () => {
  // 数组顺序是添加先后，和摆放位置无关（#627）。
  const added = [
    tile('bottom-left', '2x2', 0, 2),
    tile('top-right', '2x2', 8, 0),
    tile('top-left', '2x2', 0, 0),
    tile('bottom-right', '2x2', 8, 2),
    tile('top-middle', '2x2', 4, 0),
  ]

  it('orders rows top to bottom, then left to right', () => {
    assert.deepEqual(
      widgetsInReadingOrder(added).map((w) => w.id),
      ['top-left', 'top-middle', 'top-right', 'bottom-left', 'bottom-right'],
    )
  })

  it('keeps a frozen order and appends new tiles in reading order', () => {
    const frozen = ['top-right', 'top-left']
    assert.deepEqual(
      widgetsInReadingOrder(added, frozen).map((w) => w.id),
      ['top-right', 'top-left', 'top-middle', 'bottom-left', 'bottom-right'],
    )
  })

  it('does not mutate the saved array', () => {
    const copy = added.map((w) => w.id)
    widgetsInReadingOrder(added)
    assert.deepEqual(added.map((w) => w.id), copy)
  })
})
