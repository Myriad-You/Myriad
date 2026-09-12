import type { WidgetConfig } from './widgetGridTypes'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isRedoKey,
  isUndoKey,
  pushWidgetHistory,
  redoWidgetHistory,
  undoWidgetHistory,
} from './widgetGridHistory'

const a: WidgetConfig[] = [
  { id: 'a', type: 'weather', size: '2x2', position: { x: 0, y: 0 } },
]
const b: WidgetConfig[] = [
  { id: 'b', type: 'weather', size: '2x2', position: { x: 2, y: 0 } },
]
const c: WidgetConfig[] = [
  { id: 'c', type: 'weather', size: '2x2', position: { x: 4, y: 0 } },
]

describe('pushWidgetHistory', () => {
  it('drops redo entries after a new edit', () => {
    const stacked = pushWidgetHistory([a, b], 0, c)
    assert.deepEqual(stacked.history, [a, c])
    assert.equal(stacked.index, 1)
  })

  it('drops the oldest snapshot once the limit is reached', () => {
    const history = Array.from({ length: 20 }, (_, index) => [
      {
        id: `w${index}`,
        type: 'weather',
        size: '2x2' as const,
        position: { x: 0, y: 0 },
      },
    ])
    const next = pushWidgetHistory(history, 19, c, 20)
    assert.equal(next.history.length, 20)
    assert.equal(next.index, 19)
    assert.equal(next.history[0][0].id, 'w1')
    assert.equal(next.history[19][0].id, 'c')
  })
})

describe('undo and redo', () => {
  it('walks the stack and stops at the ends', () => {
    const history = [a, b, c]
    assert.deepEqual(undoWidgetHistory(history, 2), { index: 1, widgets: b })
    assert.equal(undoWidgetHistory(history, 0), null)
    assert.deepEqual(redoWidgetHistory(history, 0), { index: 1, widgets: b })
    assert.equal(redoWidgetHistory(history, 2), null)
  })

  it('recognizes the edit-mode undo and redo chords', () => {
    assert.equal(
      isUndoKey({ ctrlKey: true, metaKey: false, shiftKey: false, key: 'z' }),
      true,
    )
    assert.equal(
      isRedoKey({ ctrlKey: true, metaKey: false, shiftKey: true, key: 'z' }),
      true,
    )
    assert.equal(
      isRedoKey({ ctrlKey: false, metaKey: true, shiftKey: false, key: 'y' }),
      true,
    )
    assert.equal(
      isUndoKey({ ctrlKey: true, metaKey: false, shiftKey: true, key: 'z' }),
      false,
    )
  })
})
