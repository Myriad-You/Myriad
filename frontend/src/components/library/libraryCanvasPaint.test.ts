import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  CANVAS_FOCUS_WRITE_EPS,
  canvasCardPaintCacheIsCurrent,
  canvasFollowTargetsNeedPaint,
  createCanvasCardPaintCache,
  paintCanvasCardFocus,
  refreshCanvasCardPaintCache,
  sameLibraryItemIds,
} from './libraryCanvasPaint'

function fakeCard(
  cx: number,
  cy: number,
  lastFocus = Number.NaN,
  lastZ = -1,
) {
  return {
    el: { style: { transform: '', zIndex: '' } } as HTMLElement,
    cx,
    cy,
    lastFocus,
    lastZ,
  }
}

describe('canvasFollowTargetsNeedPaint', () => {
  const pose = { x: 10, y: -4, scale: 0.75 }
  const world = {} as HTMLElement

  it('repaints when the world node is remounted at the same pose', () => {
    assert.equal(canvasFollowTargetsNeedPaint(pose, pose, world, world), false)
    assert.equal(
      canvasFollowTargetsNeedPaint(pose, pose, world, {} as HTMLElement),
      true,
    )
    assert.equal(canvasFollowTargetsNeedPaint(pose, pose, world, null), true)
    assert.equal(canvasFollowTargetsNeedPaint(null, pose, null, world), true)
  })
})

describe('sameLibraryItemIds', () => {
  it('rejects a different membership without building a signature string', () => {
    const prev = [{ id: 'a' }, { id: 'b' }]
    assert.equal(sameLibraryItemIds(prev, prev), true)
    assert.equal(sameLibraryItemIds(prev, [{ id: 'a' }, { id: 'c' }]), false)
    assert.equal(sameLibraryItemIds(null, prev), false)
  })
})

describe('canvasCardPaintCacheIsCurrent', () => {
  it('treats a matching child list as current', () => {
    const first = {} as Element
    const last = {} as Element
    const world = {
      childElementCount: 2,
      firstElementChild: first,
      lastElementChild: last,
    } as HTMLElement
    const cache = createCanvasCardPaintCache()
    cache.world = world
    cache.childCount = 2
    cache.first = first
    cache.last = last
    cache.nodes = [fakeCard(0, 0), fakeCard(1, 1)]
    assert.equal(canvasCardPaintCacheIsCurrent(cache, world), true)
    cache.nodes = [fakeCard(0, 0)]
    assert.equal(canvasCardPaintCacheIsCurrent(cache, world), false)
  })
})

describe('refreshCanvasCardPaintCache', () => {
  it('keeps last focus when the same card node is still mounted', () => {
    const el = {
      hasAttribute: () => true,
      dataset: {
        layoutLeft: '0',
        layoutTop: '0',
        layoutWidth: '100',
        layoutHeight: '100',
      },
      style: { transform: '', zIndex: '' },
    } as unknown as HTMLElement
    const world = {
      children: [el],
      childElementCount: 1,
      firstElementChild: el,
      lastElementChild: el,
    } as unknown as HTMLElement
    const cache = createCanvasCardPaintCache()
    refreshCanvasCardPaintCache(cache, world)
    cache.nodes[0]!.lastFocus = 1.2
    cache.nodes[0]!.lastZ = 120
    refreshCanvasCardPaintCache(cache, world)
    assert.equal(cache.nodes[0]!.el, el)
    assert.equal(cache.nodes[0]!.lastFocus, 1.2)
    assert.equal(cache.nodes[0]!.lastZ, 120)
  })
})

describe('paintCanvasCardFocus', () => {
  const viewport = { width: 1000, height: 800 }
  const transform = { x: 0, y: 0, scale: 0.75 }

  it('writes focus once and skips a sub-pixel follow-up', () => {
    const node = fakeCard(0, 0)
    paintCanvasCardFocus([node], transform, viewport)
    assert.match(node.el.style.transform, /^scale\(/)
    assert.equal(node.lastZ > 0, true)
    const first = node.el.style.transform
    const z = node.el.style.zIndex
    node.lastFocus += CANVAS_FOCUS_WRITE_EPS / 2
    paintCanvasCardFocus([node], transform, viewport)
    assert.equal(node.el.style.transform, first)
    assert.equal(node.el.style.zIndex, z)
  })
})
