import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { getLibraryCanvasKeyboardAction, pinchCanvasTransform } from './useLibraryCanvasControls'

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

  describe('pinch zoom', () => {
    const start = { midX: 40, midY: -20, distance: 100, transform: { x: 10, y: 5, scale: 0.5 } }

    it('scales by the change in finger distance', () => {
      assert.equal(pinchCanvasTransform(start, 40, -20, 200, 0.45, 1.6).scale, 1)
      assert.equal(pinchCanvasTransform(start, 40, -20, 50, 0.45, 1.6).scale, 0.45)
      assert.equal(pinchCanvasTransform(start, 40, -20, 1000, 0.45, 1.6).scale, 1.6)
    })

    it('keeps the point under the fingers under the fingers while they move', () => {
      const world = {
        x: (start.midX - start.transform.x) / start.transform.scale,
        y: (start.midY - start.transform.y) / start.transform.scale,
      }
      const next = pinchCanvasTransform(start, -30, 60, 180, 0.45, 1.6)
      assert.ok(Math.abs(next.x + world.x * next.scale - -30) < 1e-9)
      assert.ok(Math.abs(next.y + world.y * next.scale - 60) < 1e-9)
    })

    it('pans with the midpoint when the fingers keep their distance', () => {
      const next = pinchCanvasTransform(start, 70, 10, 100, 0.45, 1.6)
      assert.deepEqual(next, { scale: 0.5, x: 40, y: 35 })
    })
  })
})
