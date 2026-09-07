import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  getWidgetDragActive,
  getWidgetDragCursor,
  setWidgetDragCursor,
  subscribeWidgetDragActive,
  subscribeWidgetDragCursor,
} from './widgetDragCursor'

describe('widgetDragCursor', () => {
  it('notifies subscribers on move and clears', () => {
    setWidgetDragCursor(null)
    const seen: Array<{ x: number; y: number } | null> = []
    const stop = subscribeWidgetDragCursor(() => {
      seen.push(getWidgetDragCursor())
    })
    setWidgetDragCursor({ x: 10, y: 20 })
    setWidgetDragCursor({ x: 10, y: 20 })
    setWidgetDragCursor({ x: 11, y: 20 })
    setWidgetDragCursor(null)
    stop()
    assert.deepEqual(seen, [
      { x: 10, y: 20 },
      { x: 11, y: 20 },
      null,
    ])
    assert.equal(getWidgetDragCursor(), null)
  })

  it('notifies active listeners only when drag starts or ends', () => {
    setWidgetDragCursor(null)
    const seen: boolean[] = []
    const stop = subscribeWidgetDragActive(() => {
      seen.push(getWidgetDragActive())
    })
    setWidgetDragCursor({ x: 1, y: 1 })
    setWidgetDragCursor({ x: 2, y: 2 })
    setWidgetDragCursor({ x: 3, y: 3 })
    setWidgetDragCursor(null)
    stop()
    assert.deepEqual(seen, [true, false])
    assert.equal(getWidgetDragActive(), false)
  })
})
