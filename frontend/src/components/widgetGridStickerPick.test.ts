import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  gridCellFromPointer,
  homeSlotAnchor,
  sameGridCell,
  stickerDragRect,
  stickerPickCandidate,
  stickerPickCollides,
} from './widgetGridStickerPick'

describe('gridCellFromPointer', () => {
  it('maps the pointer onto a cell without centering a tile', () => {
    assert.deepEqual(
      gridCellFromPointer({
        point: { x: 250, y: 80 },
        gridRect: { left: 0, top: 0, width: 1600, height: 320 },
        gridWidth: 16,
        gridHeight: 4,
      }),
      { x: 2, y: 1 },
    )
    assert.equal(
      gridCellFromPointer({
        point: { x: 10, y: 10 },
        gridRect: { left: 0, top: 0, width: 0, height: 0 },
        gridWidth: 16,
        gridHeight: 4,
      }),
      null,
    )
  })
})

describe('sticker drag placement', () => {
  it('normalizes a dragged rectangle and snaps it to a sticker size', () => {
    assert.deepEqual(
      stickerDragRect({ start: { x: 4, y: 2 }, end: { x: 1, y: 0 } }),
      { x: 1, y: 0, w: 4, h: 3 },
    )
    const candidate = stickerPickCandidate({
      start: { x: 0, y: 0 },
      end: { x: 1, y: 1 },
    })
    assert.equal(candidate.kind, 'sticker')
    assert.equal(candidate.type, 'sticker')
    assert.deepEqual(candidate.position, { x: 0, y: 0 })
    assert.equal(candidate.size, '2x2')
  })

  it('reports a collision against an occupied cell', () => {
    assert.equal(
      stickerPickCollides(
        { start: { x: 0, y: 0 }, end: { x: 1, y: 1 } },
        [
          {
            id: 'a',
            type: 'weather',
            size: '2x2',
            position: { x: 0, y: 0 },
          },
        ],
        16,
        8,
      ),
      true,
    )
    assert.equal(
      stickerPickCollides(
        { start: { x: 4, y: 0 }, end: { x: 5, y: 1 } },
        [
          {
            id: 'a',
            type: 'weather',
            size: '2x2',
            position: { x: 0, y: 0 },
          },
        ],
        16,
        8,
      ),
      false,
    )
  })
})

describe('homeSlotAnchor', () => {
  it('projects a slot into viewport pixels, or zeros without a rect', () => {
    assert.deepEqual(
      homeSlotAnchor(
        { left: 100, top: 50, width: 1600, height: 800 },
        { x: 2, y: 1 },
        '2x2',
        16,
        8,
      ),
      {
        left: 300,
        top: 150,
        width: 200,
        height: 200,
        right: 500,
        bottom: 350,
      },
    )
    assert.deepEqual(homeSlotAnchor(null, { x: 2, y: 1 }, '2x2', 16, 8), {
      left: 0,
      top: 0,
      width: 0,
      height: 0,
      right: 0,
      bottom: 0,
    })
  })

  it('treats an unchanged hover cell as the same cell', () => {
    assert.equal(sameGridCell({ x: 1, y: 2 }, { x: 1, y: 2 }), true)
    assert.equal(sameGridCell(null, { x: 1, y: 2 }), false)
  })
})
