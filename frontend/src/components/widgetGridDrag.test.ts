import type { WidgetType } from './widgetGridTypes'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  beginExistingWidgetDrag,
  beginLibraryWidgetDrag,
  idleWidgetDrag,
  libraryDragFitsBudget,
  resolveExistingWidgetDrop,
  resolveLibraryWidgetDrop,
  settleWidgetDrag,
  shouldClearDragOnEditExit,
} from './widgetGridDrag'

const weather = {
  id: 'weather',
  name: 'Weather',
  defaultSize: '2x2',
  component: () => null,
} as unknown as WidgetType

const occupied = [
  {
    id: 'a',
    type: 'weather',
    size: '2x2' as const,
    position: { x: 0, y: 0 },
  },
]

describe('widget drag session', () => {
  it('starts a move without leftover settle flags', () => {
    assert.deepEqual(beginExistingWidgetDrag('a', { x: 2, y: 0 }), {
      dragged: { type: 'existing', widgetId: 'a' },
      settling: false,
      previewUncovered: false,
      previewExiting: false,
      hoveredCell: { x: 2, y: 0 },
    })
    assert.deepEqual(beginLibraryWidgetDrag('weather', null), {
      dragged: { type: 'new', widgetTypeId: 'weather' },
      settling: false,
      previewUncovered: false,
      previewExiting: false,
      hoveredCell: null,
    })
  })

  it('clears the session only when leaving edit mode', () => {
    assert.equal(shouldClearDragOnEditExit(true, false), true)
    assert.equal(shouldClearDragOnEditExit(false, true), false)
    assert.equal(shouldClearDragOnEditExit(true, true), false)
    assert.deepEqual(idleWidgetDrag().dragged, null)
  })

  it('settles onto the committed cell and drops the hover', () => {
    const settled = settleWidgetDrag(
      { type: 'existing', widgetId: 'a' },
      'a',
      { x: 4, y: 1 },
    )
    assert.equal(settled.settling, true)
    assert.deepEqual(settled.hoveredCell, null)
    assert.equal(settled.dragged?.pendingId, 'a')
    assert.deepEqual(settled.dragged?.pendingCell, { x: 4, y: 1 })
  })
})

describe('widget drop', () => {
  it('moves an existing tile when the cell is free', () => {
    const drop = resolveExistingWidgetDrop({
      widget: occupied[0],
      widgets: occupied,
      cell: { x: 4, y: 0 },
      gridWidth: 16,
      gridHeight: 4,
    })
    assert.equal(drop.type, 'settle')
    if (drop.type === 'settle') {
      assert.deepEqual(drop.widgets[0].position, { x: 4, y: 0 })
      assert.equal(drop.pendingId, 'a')
    }
  })

  it('rejects an overlapping existing drop', () => {
    const drop = resolveExistingWidgetDrop({
      widget: {
        id: 'b',
        type: 'weather',
        size: '2x2',
        position: { x: 4, y: 0 },
      },
      widgets: occupied,
      cell: { x: 0, y: 0 },
      gridWidth: 16,
      gridHeight: 4,
    })
    assert.equal(drop.type, 'idle')
  })

  it('places a library tile when the free-layout budget still fits', () => {
    assert.equal(libraryDragFitsBudget(false, occupied, '2x2'), true)
    const drop = resolveLibraryWidgetDrop({
      widgetType: weather,
      widgets: occupied,
      cell: { x: 4, y: 0 },
      gridWidth: 16,
      gridHeight: 4,
      isFreeLayout: false,
      id: 'widget_new',
    })
    assert.equal(drop.type, 'settle')
    if (drop.type === 'settle') {
      assert.equal(drop.widgets.length, 2)
      assert.equal(drop.pendingId, 'widget_new')
    }
  })

  it('refuses a library drop that collides or blows the free-layout budget', () => {
    const collide = resolveLibraryWidgetDrop({
      widgetType: weather,
      widgets: occupied,
      cell: { x: 0, y: 0 },
      gridWidth: 16,
      gridHeight: 4,
      isFreeLayout: false,
      id: 'widget_new',
    })
    assert.equal(collide.type, 'idle')

    const filled = Array.from({ length: 24 }, (_, index) => ({
      id: `w${index}`,
      type: 'weather',
      size: '2x2' as const,
      position: { x: (index % 8) * 2, y: Math.floor(index / 8) * 2 },
    }))
    assert.equal(libraryDragFitsBudget(true, filled, '2x2'), false)
    const over = resolveLibraryWidgetDrop({
      widgetType: weather,
      widgets: filled,
      cell: { x: 14, y: 6 },
      gridWidth: 16,
      gridHeight: 8,
      isFreeLayout: true,
      id: 'widget_new',
    })
    assert.equal(over.type, 'idle')
  })
})
