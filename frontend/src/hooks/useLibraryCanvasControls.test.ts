import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { getLibraryCanvasKeyboardAction } from './useLibraryCanvasControls'

describe('library canvas controls', () => {
  it('maps keyboard pan, zoom, and reset controls', () => {
    assert.deepEqual(getLibraryCanvasKeyboardAction('ArrowLeft'), {
      kind: 'pan',
      x: 1,
      y: 0,
    })
    assert.deepEqual(getLibraryCanvasKeyboardAction('+'), {
      kind: 'zoom',
      factor: 1.16,
    })
    assert.deepEqual(getLibraryCanvasKeyboardAction('0'), { kind: 'reset' })
    assert.equal(getLibraryCanvasKeyboardAction('Enter'), null)
  })

  it('uses a native non-passive wheel listener instead of React onWheel', () => {
    const hook = readFileSync(
      new URL('./useLibraryCanvasControls.ts', import.meta.url),
      'utf8',
    )
    const grid = readFileSync(
      new URL('../components/LibraryGrid.tsx', import.meta.url),
      'utf8',
    )
    assert.match(
      hook,
      /addEventListener\('wheel', handleWheel, \{ passive: false \}\)/,
    )
    assert.match(hook, /focus\(\{ preventScroll: true \}\)/)
    assert.match(hook, /onPaintRef\.current\?\.\(next\)/)
    assert.match(hook, /shouldCommitRef/)
    assert.match(hook, /flushCommit/)
    assert.match(hook, /transformRef/)
    assert.match(hook, /KEYBOARD_PAN_ACCELERATION/)
    assert.match(hook, /KEYBOARD_PAN_DECELERATION/)
    assert.match(hook, /motion\.frame = requestAnimationFrame\(tick\)/)
    assert.doesNotMatch(hook, /FOCUS_SETTLE|settleFocus/)
    assert.doesNotMatch(grid, /\bonWheel=/)
    assert.match(grid, /paintCanvasTransform/)
    assert.match(grid, /shouldCommitCanvasTransform/)
    assert.match(grid, /data-canvas-card/)
    assert.match(grid, /paintCanvasCardFocus/)
    assert.doesNotMatch(
      grid,
      /querySelectorAll<HTMLElement>\('\[data-canvas-card\]'\)/,
    )
  })
})
