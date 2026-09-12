import type { WidgetType } from './widgetGridTypes'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  coveringWidgetId,
  dragGhostContentSize,
  dragGhostExitMs,
  dragGhostHandoffDelays,
  dragGhostSettleWaitMs,
  dragGhostSitMs,
  gridCellFromPoint,
  heldWidgetId,
  placementHasCommitted,
  resolveDragGhostWidget,
  shouldSkipWidgetEntrance,
  widgetDragGhostAnchor,
  widgetDragGhostBox,
  widgetPlacementCollides,
} from './widgetPlacementPreview'

describe('library placement flags', () => {
  it('skips entrance only while the grid is being edited', () => {
    assert.equal(shouldSkipWidgetEntrance(true), true)
    assert.equal(shouldSkipWidgetEntrance(false), false)
  })
})

describe('gridCellFromPoint', () => {
  it('centers the tile on the pointer and clamps to the board', () => {
    const cell = gridCellFromPoint({
      point: { x: 250, y: 80 },
      gridRect: { left: 0, top: 0, width: 1600, height: 320 },
      gridWidth: 16,
      gridHeight: 4,
      size: { w: 2, h: 2 },
    })
    assert.deepEqual(cell, { x: 1, y: 0 })
  })

  it('does not let a 4x2 tile hang off the right or bottom', () => {
    const cell = gridCellFromPoint({
      point: { x: 2000, y: 400 },
      gridRect: { left: 0, top: 0, width: 1600, height: 320 },
      gridWidth: 16,
      gridHeight: 4,
      size: { w: 4, h: 2 },
    })
    assert.deepEqual(cell, { x: 12, y: 2 })
  })
})

describe('widgetDragGhostAnchor', () => {
  it('pins the ghost to the cell center', () => {
    assert.deepEqual(
      widgetDragGhostAnchor(
        { left: 100, top: 50, width: 1600, height: 320 },
        { x: 2, y: 1 },
        { w: 2, h: 2 },
        16,
        4,
      ),
      { x: 400, y: 210 },
    )
  })
})

describe('drag ghost settle', () => {
  it('insets the ghost to the p-1 tile, not the raw cell', () => {
    assert.deepEqual(dragGhostContentSize(100, 80, { w: 2, h: 2 }), {
      width: 192,
      height: 152,
    })
  })

  it('slides from the pointer to the padded cell box', () => {
    const box = widgetDragGhostBox({
      gridRect: { left: 100, top: 50, width: 1600, height: 320 },
      cell: { x: 2, y: 1 },
      size: { w: 2, h: 2 },
      gridWidth: 16,
      gridHeight: 4,
    })
    assert.deepEqual(box, { x: 400, y: 210, width: 192, height: 152 })
  })

  it('waits out the settle motion unless motion is reduced', () => {
    assert.equal(dragGhostSettleWaitMs(false, 40), 200)
    assert.equal(dragGhostSettleWaitMs(false, 400), 0)
    assert.equal(dragGhostSettleWaitMs(true, 0), 0)
    assert.equal(dragGhostSitMs(false), 90)
    assert.equal(dragGhostSitMs(true), 0)
    assert.equal(dragGhostExitMs(false), 260)
    assert.equal(dragGhostExitMs(true), 0)
    assert.deepEqual(dragGhostHandoffDelays(false, 40), {
      uncoverMs: 200,
      exitMs: 290,
      clearMs: 550,
    })
    assert.deepEqual(dragGhostHandoffDelays(true, 0), {
      uncoverMs: 0,
      exitMs: 0,
      clearMs: 0,
    })
  })

  it('holds the tile only until the live layer uncovers under the preview', () => {
    assert.equal(
      heldWidgetId({
        dragged: { type: 'existing', widgetId: 'a' },
        settling: false,
      }),
      'a',
    )
    assert.equal(
      heldWidgetId({
        dragged: { type: 'new', widgetTypeId: 'weather', pendingId: 'b' },
        settling: true,
      }),
      'b',
    )
    assert.equal(
      heldWidgetId({
        dragged: { type: 'new', widgetTypeId: 'weather', pendingId: 'b' },
        settling: true,
        uncovered: true,
      }),
      undefined,
    )
    assert.equal(
      heldWidgetId({
        dragged: { type: 'new', widgetTypeId: 'weather' },
        settling: false,
      }),
      undefined,
    )
    assert.equal(
      coveringWidgetId({
        dragged: { type: 'new', widgetTypeId: 'weather', pendingId: 'b' },
        settling: true,
      }),
      'b',
    )
    assert.equal(
      coveringWidgetId({
        dragged: { type: 'existing', widgetId: 'a' },
        settling: false,
      }),
      undefined,
    )
  })

  it('waits for the committed cell, not just the id', () => {
    const widgets = [
      {
        id: 'widget_9',
        type: 'weather',
        size: '2x2' as const,
        position: { x: 0, y: 0 },
      },
    ]
    assert.equal(
      placementHasCommitted(widgets, {
        type: 'existing',
        widgetId: 'widget_9',
        pendingId: 'widget_9',
        pendingCell: { x: 4, y: 1 },
      }),
      false,
    )
    assert.equal(
      placementHasCommitted(
        [{ ...widgets[0], position: { x: 4, y: 1 } }],
        {
          type: 'existing',
          widgetId: 'widget_9',
          pendingId: 'widget_9',
          pendingCell: { x: 4, y: 1 },
        },
      ),
      true,
    )
  })
})

describe('resolveDragGhostWidget', () => {
  const weather = {
    id: 'weather',
    name: 'Weather',
    defaultSize: '2x2',
    component: () => null,
  } as unknown as WidgetType
  const types = new Map<string, WidgetType>([['weather', weather]])

  it('builds a catalog preview for a library drag without a hover cell', () => {
    const ghost = resolveDragGhostWidget({
      dragged: { type: 'new', widgetTypeId: 'weather' },
      widgets: [],
      widgetTypeById: types,
    })
    assert.ok(ghost)
    assert.equal(ghost?.widgetType, weather)
    assert.equal(ghost?.widgetConfig?.id, 'preview-weather')
    assert.deepEqual(ghost?.size, { w: 2, h: 2 })
    assert.equal(ghost?.fromLibrary, true)
  })

  it('reuses the live instance when moving an existing tile', () => {
    const widget = {
      id: 'widget_9',
      type: 'weather',
      size: '4x2' as const,
      position: { x: 0, y: 0 },
    }
    const ghost = resolveDragGhostWidget({
      dragged: { type: 'existing', widgetId: 'widget_9' },
      widgets: [widget],
      widgetTypeById: types,
    })
    assert.equal(ghost?.widgetConfig, widget)
    assert.deepEqual(ghost?.size, { w: 4, h: 2 })
    assert.equal(ghost?.fromLibrary, false)
  })
})

describe('widgetPlacementCollides', () => {
  const left = {
    id: 'a',
    type: 'weather',
    size: '2x2' as const,
    position: { x: 0, y: 0 },
  }

  it('treats overflow and overlap as a collision, and ignores the moving id', () => {
    assert.equal(widgetPlacementCollides(left, [], 16, 4), false)
    assert.equal(
      widgetPlacementCollides({ ...left, position: { x: 15, y: 0 } }, [], 16, 4),
      true,
    )
    assert.equal(
      widgetPlacementCollides(
        { ...left, id: 'b', position: { x: 1, y: 0 } },
        [left],
        16,
        4,
      ),
      true,
    )
    assert.equal(
      widgetPlacementCollides(
        { ...left, position: { x: 2, y: 0 } },
        [left],
        16,
        4,
        'a',
      ),
      false,
    )
  })
})
