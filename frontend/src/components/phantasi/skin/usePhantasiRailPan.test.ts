import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  dropStoryDomShells,
  eagerStoryCovers,
  eagerStoryDelta,
  ensureStoryShells,
  followRailScroll,
  isDiscreteWheel,
  paintStoryAway,
  paintStoryLiveCols,
  RAIL_FLING_MIN_PX_S,
  RAIL_MOUNT_BOOT_TO,
  RAIL_MOUNT_GROW_AHEAD,
  RAIL_MOUNT_LIVE_PAD,
  RAIL_MOUNT_PAN_EXTRA,
  RAIL_MOUNT_RESERVE,
  RAIL_MOUNT_SETTLE_EXTRA,
  railCardKeepsPaint,
  railCardOffset,
  railCoastStep,
  railColumnSlots,
  railLeadColumn,
  railLeadIndex,
  railLiveTo,
  railMaxScroll,
  railMountColumns,
  railMountColumnsCovered,
  railMountColumnsGrow,
  railMountColumnsPan,
  railMountColumnsSettle,
  railMountColumnsSticky,
  railOverflowLeft,
  railSeatScroll,
  railSettleTau,
  railSlotOffsets,
  railTrackScroll,
  recycleStoryDomShellsOutside,
  scrollFromTrackTransform,
  sourceAtScroll,
  storyMountWindow,
  storyRailTrackSize,
} from './railPan.ts'

const cards = [0, 320, 640, 960].map((left) => ({ left, width: 304 }))
const slots = railSlotOffsets(cards)

function wheel(
  deltaY: number,
  deltaMode = 0,
): WheelEvent {
  return { deltaMode, deltaX: 0, deltaY } as WheelEvent
}

describe('railLeadIndex', () => {
  it('起点是第一张', () => {
    assert.equal(railLeadIndex(cards, 0, 800, 96), 0)
  })

  it('第一张几乎走完后切到第二张', () => {
    assert.equal(railLeadIndex(cards, 220, 800, 96), 1)
  })

  it('第一张还剩实心宽度时仍是第一张', () => {
    assert.equal(railLeadIndex(cards, 200, 800, 96), 0)
  })

  it('滑过两张后是第三张', () => {
    assert.equal(railLeadIndex(cards, 680, 800, 96), 2)
  })

  it('上一张坐在左缘时焦点是左缘卡', () => {
    assert.equal(railLeadIndex(cards, 320, 800, 96), 1)
  })

  it('前一张只溢出一截时焦点仍是选中卡', () => {
    assert.equal(railLeadIndex(cards, 596, 1280, 96), 2)
  })
})

describe('railSeatScroll', () => {
  it('选中卡坐到左缘', () => {
    assert.equal(railSeatScroll(cards, 0), 0)
    assert.equal(railSeatScroll(cards, 2), 640)
  })

  it('网站轨选中卡让出左缘一截，前一张溢出', () => {
    assert.equal(railSeatScroll(cards, 0, 44), 0)
    assert.equal(railSeatScroll(cards, 2, 44), 596)
    assert.equal(railCardOffset(cards, 2), 640)
    assert.equal(railCardOffset(cards, -1), 0)
  })
})

describe('railCardKeepsPaint / railMountColumns', () => {
  it('右侧预显示和进画的卡不退场', () => {
    assert.equal(railCardKeepsPaint(200, true, 96), true)
    assert.equal(railCardKeepsPaint(900, true, 96), true)
    assert.equal(railCardKeepsPaint(-40, true, 96), true)
    assert.equal(railCardKeepsPaint(20, false, 96), false)
  })

  it('实装视口列并在右边多留几列', () => {
    assert.deepEqual(railMountColumns(0, 800, 280, 20, 1, 3), { from: 1, to: 6 })
    assert.deepEqual(railMountColumns(840, 800, 280, 20, 1, 3), { from: 3, to: 9 })
  })

  it('列槽按列宽铺满整轨', () => {
    assert.deepEqual(railColumnSlots(4, 280), [0, 280, 560, 840])
  })

  it('窗口还盖住视口和右侧预显就沿用，不够才扩', () => {
    const prev = { from: 1, to: 8 }
    assert.equal(
      railMountColumnsSticky(prev, 0, 800, 280, 20, 1, 3),
      prev,
    )
    const moved = railMountColumnsSticky(prev, 840, 800, 280, 20, 1, 3)
    const ideal = railMountColumns(840, 800, 280, 20, 1, 3)
    assert.ok(moved.to >= ideal.to)
    assert.ok(moved.from <= ideal.from)
    assert.notDeepEqual(moved, prev)
  })

  it('手势窗口只扩不缩，仍盖住右侧预显', () => {
    const prev = { from: 1, to: 8 }
    assert.equal(
      railMountColumnsGrow(prev, 0, 800, 280, 20, 1, 3),
      prev,
    )
    const grown = railMountColumnsGrow(prev, 840, 800, 280, 20, 1, 3)
    const ideal = railMountColumns(840, 800, 280, 20, 1, 3)
    assert.equal(grown.from, 1)
    assert.ok(grown.to >= ideal.to)
    assert.ok(grown.to >= prev.to)
  })

  it('长滑时手势窗口只扩不拆左边，停稳再收，右侧预显仍在', () => {
    const prev = { from: 1, to: 8 }
    assert.equal(
      railMountColumnsPan(prev, 0, 800, 280, 40, 1, 3),
      prev,
    )
    const mid = railMountColumnsPan(prev, 2800, 800, 280, 40, 1, 3)
    const midIdeal = railMountColumns(2800, 800, 280, 40, 1, 3)
    assert.ok(mid.to >= midIdeal.to)
    assert.equal(mid.from, 1)
    const far = railMountColumnsPan(prev, 5600, 800, 280, 40, 1, 3)
    const grown = railMountColumnsGrow(prev, 5600, 800, 280, 40, 1, 3)
    const ideal = railMountColumns(5600, 800, 280, 40, 1, 3)
    assert.ok(far.to >= ideal.to)
    assert.equal(far.from, 1)
    assert.deepEqual(far, grown)
    const settled = railMountColumnsSettle(far, 5600, 800, 280, 40, 1, 3)
    assert.ok(settled.to >= ideal.to)
    assert.ok(settled.from <= ideal.from)
    assert.ok(settled.to - settled.from < far.to - far.from)
    assert.equal(settled.from, Math.max(1, ideal.from - RAIL_MOUNT_SETTLE_EXTRA))
    assert.ok(settled.from < ideal.from)
  })

  it('扩窗一次多挂几列，下一列仍盖住预显', () => {
    assert.equal(RAIL_MOUNT_BOOT_TO, RAIL_MOUNT_PAN_EXTRA + RAIL_MOUNT_GROW_AHEAD)
    const start = { from: 1, to: 12 }
    const grown = railMountColumnsPan(start, 1960, 800, 280, 40, 1, 3)
    assert.ok(grown.to - start.to >= RAIL_MOUNT_GROW_AHEAD)
    const nextNeed = railMountColumns(1960 + 280, 800, 280, 40, 1, 3)
    assert.ok(grown.from <= nextNeed.from)
    assert.ok(grown.to >= nextNeed.to + RAIL_MOUNT_RESERVE)
    assert.equal(
      railMountColumnsPan(grown, 1960 + 280, 800, 280, 40, 1, 3),
      grown,
    )
    const far = railMountColumnsPan(start, 5600, 800, 280, 40, 1, 3)
    const farNext = railMountColumns(5600 + 280, 800, 280, 40, 1, 3)
    assert.ok(far.from <= farNext.from)
    assert.ok(far.to >= farNext.to + RAIL_MOUNT_RESERVE)
    assert.equal(railMountColumnsPan(far, 5600 + 280, 800, 280, 40, 1, 3), far)
    assert.equal(RAIL_MOUNT_LIVE_PAD, RAIL_MOUNT_RESERVE + 1)
    assert.equal(railLiveTo(8, 16, 8), 11)
    assert.ok(railLiveTo(8, 16, 8) < 16)
    assert.ok(railLiveTo(8, 16, 8) >= 8 + 3)
    assert.equal(railLiveTo(12, 16, 13), 15)
  })

  it('窗口还盖住预显且不太大就不用扩', () => {
    const prev = { from: 1, to: 12 }
    const need = railMountColumns(0, 800, 280, 40, 1, 3)
    assert.equal(
      railMountColumnsCovered(prev, need, 40, RAIL_MOUNT_PAN_EXTRA),
      true,
    )
    assert.equal(railMountColumnsPan(prev, 0, 800, 280, 40, 1, 3), prev)
    const tight = { from: 1, to: 4 }
    assert.equal(
      railMountColumnsCovered(tight, need, 40, RAIL_MOUNT_PAN_EXTRA),
      false,
    )
    assert.ok(need.to >= 6)
    const exact = { from: need.from, to: need.to }
    assert.equal(
      railMountColumnsCovered(exact, need, 40, RAIL_MOUNT_PAN_EXTRA, 0),
      true,
    )
    assert.equal(
      railMountColumnsCovered(exact, need, 40, RAIL_MOUNT_PAN_EXTRA, RAIL_MOUNT_RESERVE),
      false,
    )
  })

  it('停稳时窗口只大了一点就留下，仍盖住右侧预显', () => {
    const grown = { from: 1, to: 12 }
    const kept = railMountColumnsSettle(grown, 840, 800, 280, 20, 1, 3)
    const ideal = railMountColumns(840, 800, 280, 20, 1, 3)
    assert.equal(kept, grown)
    assert.ok(kept.to >= ideal.to)
    const boot = { from: 1, to: 20 }
    assert.equal(railMountColumnsSettle(boot, 840, 800, 280, 20, 1, 3), boot)
    const huge = { from: 1, to: 40 }
    const settled = railMountColumnsSettle(huge, 840, 800, 280, 40, 1, 3)
    assert.ok(settled.to >= ideal.to)
    assert.ok(settled.to - settled.from < huge.to - huge.from)
    assert.ok(settled.to >= ideal.to + (RAIL_MOUNT_SETTLE_EXTRA - 2))
  })

  it('实装窗夹在全列里，总宽按全列算', () => {
    assert.deepEqual(storyMountWindow(1, 12, 5), { from: 1, to: 5 })
    assert.deepEqual(storyMountWindow(10, 20, 40), { from: 10, to: 20 })
    assert.equal(storyRailTrackSize(1), 'var(--phantasi-story-w)')
    assert.equal(
      storyRailTrackSize(40),
      'calc(40 * (var(--phantasi-story-w) + 0.75rem) - 0.75rem)',
    )
  })

  it('只预热视口和右侧预显的封面，扩窗空列不解码', () => {
    const loads = [
      { col: 1, loading: 'lazy' },
      { col: 8, loading: 'lazy' },
      { col: 12, loading: 'lazy' },
    ]
    const nodes = loads.map((item) => ({
      dataset: { railCol: String(item.col) },
      querySelector: () => item,
    }))
    let firstQueried = 0
    const track = {
      children: nodes,
      querySelectorAll: (selector: string) => {
        firstQueried += 1
        const col = Number(/data-rail-col="(\d+)"/.exec(selector)?.[1])
        return nodes.filter((node) => Number(node.dataset.railCol) === col)
      },
    }
    eagerStoryCovers(track as unknown as ParentNode, 1, 8)
    assert.equal(firstQueried, 0)
    assert.equal(loads[0].loading, 'eager')
    assert.equal(loads[1].loading, 'eager')
    assert.equal(loads[2].loading, 'lazy')
    loads[0].loading = 'eager'
    eagerStoryCovers(track as unknown as ParentNode, 2, 9, { from: 1, to: 8 })
    assert.equal(firstQueried, 0)
    assert.equal(loads[2].loading, 'lazy')
    const manyLoads = Array.from({ length: 26 }, (_, index) => ({
      col: index + 1,
      loading: 'lazy' as string,
    }))
    const manyNodes = manyLoads.map((item) => ({
      dataset: { railCol: String(item.col) },
      querySelector: () => item,
    }))
    let queried = 0
    const many = {
      children: manyNodes,
      querySelectorAll: (selector: string) => {
        queried += 1
        const col = Number(/data-rail-col="(\d+)"/.exec(selector)?.[1])
        return manyLoads
          .filter((item) => item.col === col)
          .map((item) => ({
            querySelector: () => item,
          }))
      },
    }
    eagerStoryCovers(many as unknown as ParentNode, 2, 10, { from: 1, to: 8 })
    assert.equal(queried, 0)
    assert.equal(manyLoads[0].loading, 'lazy')
    assert.equal(manyLoads[9].loading, 'eager')
    const fallbackLoads = [
      { col: 1, loading: 'lazy' },
      { col: 8, loading: 'lazy' },
    ]
    const fallback = {
      querySelectorAll: (selector: string) => {
        const col = Number(/data-rail-col="(\d+)"/.exec(selector)?.[1])
        return fallbackLoads
          .filter((item) => item.col === col)
          .map((item) => ({
            querySelector: () => item,
          }))
      },
    }
    eagerStoryCovers(fallback as unknown as ParentNode, 1, 8)
    assert.equal(fallbackLoads[0].loading, 'eager')
    assert.equal(fallbackLoads[1].loading, 'eager')
    fallbackLoads.push({ col: 9, loading: 'lazy' })
    eagerStoryCovers(fallback as unknown as ParentNode, 2, 9, { from: 1, to: 8 })
    assert.equal(fallbackLoads[0].loading, 'eager')
    assert.equal(fallbackLoads[2].loading, 'eager')
    const deferred = {
      col: 4,
      loading: 'lazy' as string,
      src: '',
      getAttribute(name: string) {
        if (name === 'data-src') return '/cover.jpg'
        if (name === 'src') return this.src
        return null
      },
    }
    const deferredTrack = {
      children: [
        {
          dataset: { railCol: '4' },
          querySelector: () => deferred,
        },
      ],
    }
    eagerStoryCovers(deferredTrack as unknown as ParentNode, 4, 4)
    assert.equal(deferred.loading, 'eager')
    assert.equal(deferred.src, '/cover.jpg')
    const slotLoad = { col: 5, loading: 'lazy' as string }
    eagerStoryCovers(
      {
        children: [
          {
            dataset: { railCol: '5' },
            classList: {
              contains: (name: string) => name === 'phantasi-story--slot',
              remove() {},
            },
            querySelector: () => slotLoad,
          },
        ],
      } as unknown as ParentNode,
      5,
      5,
    )
    assert.equal(slotLoad.loading, 'lazy')
    assert.deepEqual(eagerStoryDelta(4, 10, 3, 9), [{ from: 10, to: 10 }])
    assert.deepEqual(eagerStoryDelta(3, 9, 4, 10), [{ from: 3, to: 3 }])
    assert.deepEqual(eagerStoryDelta(1, 8, 1, 8), [])
    assert.deepEqual(eagerStoryDelta(20, 26, 1, 8), [{ from: 20, to: 26 }])
    const awayOf = (col: number) => {
      const classes = new Set<string>()
      return {
        dataset: { railCol: String(col) },
        classList: {
          toggle(name: string, on?: boolean) {
            if (on) classes.add(name)
            else classes.delete(name)
          },
          add(name: string) {
            classes.add(name)
          },
          remove(name: string) {
            classes.delete(name)
          },
          has: (name: string) => classes.has(name),
        },
      }
    }
    const awayNodes = [1, 2, 8, 12].map(awayOf)
    let awayQueried = 0
    const awayTrack = {
      children: awayNodes,
      querySelectorAll: (selector: string) => {
        awayQueried += 1
        const col = Number(/data-rail-col="(\d+)"/.exec(selector)?.[1])
        return awayNodes.filter((node) => Number(node.dataset.railCol) === col)
      },
    }
    paintStoryAway(awayTrack as unknown as ParentNode, 1, 8)
    assert.equal(awayQueried, 0)
    assert.equal(awayNodes[0]?.classList.has('is-away'), false)
    assert.equal(awayNodes[2]?.classList.has('is-away'), false)
    assert.equal(awayNodes[3]?.classList.has('is-away'), true)
    paintStoryAway(awayTrack as unknown as ParentNode, 2, 9, { from: 1, to: 8 })
    assert.equal(awayQueried, 0)
    assert.equal(awayNodes[0]?.classList.has('is-away'), true)
    assert.equal(awayNodes[3]?.classList.has('is-away'), true)
    awayNodes[3]?.classList.add('is-hold')
    paintStoryAway(awayTrack as unknown as ParentNode, 9, 16, { from: 2, to: 9 })
    assert.equal(awayNodes[3]?.classList.has('is-hold'), false)
    assert.equal(awayNodes[3]?.classList.has('is-away'), false)
    const holdNodes = [1, 2, 8].map(awayOf)
    const holdTrack = { children: holdNodes }
    paintStoryAway(holdTrack as unknown as ParentNode, 1, 8)
    paintStoryAway(
      holdTrack as unknown as ParentNode,
      2,
      9,
      { from: 1, to: 8 },
      false,
    )
    assert.equal(holdNodes[0]?.classList.has('is-away'), false)
    assert.equal(holdNodes[2]?.classList.has('is-away'), false)
    const tailLoads = Array.from({ length: 40 }, (_, index) => ({
      col: index + 1,
      loading: 'lazy' as string,
    }))
    const tailNodes = tailLoads.map((item) => ({
      dataset: { railCol: String(item.col) },
      querySelector: () => item,
    }))
    const moved = tailNodes[8]
    if (moved) {
      tailNodes.splice(8, 1)
      tailNodes.unshift(moved)
    }
    let tailQueried = 0
    const tailTrack = {
      children: tailNodes,
      querySelectorAll: () => {
        tailQueried += 1
        return []
      },
    }
    eagerStoryCovers(tailTrack as unknown as ParentNode, 1, 8)
    assert.equal(tailQueried, 0)
    eagerStoryCovers(tailTrack as unknown as ParentNode, 2, 9, { from: 1, to: 8 })
    assert.equal(tailQueried, 0)
    assert.equal(tailLoads[8]?.loading, 'eager')
    assert.equal(tailLoads[39]?.loading, 'lazy')
    const slot = {
      className: 'phantasi-story phantasi-story--slot',
      dataset: { railCol: '4' } as Record<string, string>,
      classList: {
        contains: (name: string) => name === 'phantasi-story--slot',
      },
      style: { setProperty() {} },
      innerHTML: '',
      removeAttribute() {},
    }
    paintStoryLiveCols(
      { children: [slot] } as unknown as ParentNode,
      4,
      4,
      [{
        story: {
          id: 9,
          title: 'Hello',
          summary: '',
          image: null,
          published_at: Date.now(),
          is_read: true,
          is_starred: false,
          source_name: 'A',
        },
        column: 4,
        row: 1,
      }],
      {
        justNow: 'now',
        minutesAgo: '{minutes}m',
        hoursAgo: '{hours}h',
        daysAgo: '{days}d',
      },
      'en',
      { unread: 'new', starred: 'star', unstar: 'unstar' },
      8,
      true,
    )
    assert.match(slot.className, /phantasi-story__hit/)
    assert.match(slot.innerHTML, /Hello/)
    assert.equal(slot.dataset.railId, '9')
    const held = {
      className: 'phantasi-story phantasi-story--slot',
      dataset: { railCol: '5' } as Record<string, string>,
      classList: {
        contains: (name: string) => name === 'phantasi-story--slot',
      },
      style: { setProperty() {} },
      innerHTML: '',
      removeAttribute() {},
    }
    paintStoryLiveCols(
      { children: [held] } as unknown as ParentNode,
      5,
      5,
      [{
        story: {
          id: 10,
          title: 'Covered',
          summary: '',
          image: '/cover.jpg',
          published_at: Date.now(),
          is_read: true,
          is_starred: false,
          source_name: 'A',
        },
        column: 5,
        row: 1,
      }],
      {
        justNow: 'now',
        minutesAgo: '{minutes}m',
        hoursAgo: '{hours}h',
        daysAgo: '{days}d',
      },
      'en',
      { unread: 'new', starred: 'star', unstar: 'unstar' },
      8,
      true,
      true,
    )
    assert.match(held.innerHTML, /Covered/)
    assert.match(held.innerHTML, /data-src=/)
    assert.doesNotMatch(held.innerHTML, /<img /)
    const kids: Array<{ dataset: Record<string, string>; remove?: () => void }> = []
    const host = {
      children: kids,
      appendChild(el: { dataset: Record<string, string> }) {
        kids.push(el)
        return el
      },
    }
    const created: Array<{ dataset: Record<string, string> }> = []
    const prevDoc = globalThis.document
    globalThis.document = {
      createElement() {
        const el = {
          type: '',
          className: '',
          dataset: {} as Record<string, string>,
          style: { cssText: '' },
          tabIndex: 0,
          setAttribute() {},
          remove() {
            const at = kids.indexOf(el)
            if (at >= 0) kids.splice(at, 1)
          },
        }
        created.push(el)
        return el
      },
    } as unknown as Document
    try {
      ensureStoryShells(
        host as unknown as ParentNode,
        5,
        5,
        [{ column: 5, row: 1 }, { column: 5, row: 2 }],
      )
      assert.equal(created.length, 2)
      assert.equal(created[0]?.dataset.phantasiDomShell, '1')
      dropStoryDomShells(host as unknown as ParentNode)
      assert.equal(kids.length, 0)
      ensureStoryShells(
        host as unknown as ParentNode,
        6,
        6,
        [{ column: 6, row: 1 }, { column: 6, row: 2 }],
      )
      assert.equal(created.length, 2)
      assert.equal(kids.length, 2)
      ensureStoryShells(
        host as unknown as ParentNode,
        5,
        5,
        [{ column: 5, row: 1 }, { column: 5, row: 2 }],
      )
      assert.equal(created.length, 4)
      assert.equal(kids.length, 4)
      recycleStoryDomShellsOutside(host as unknown as ParentNode, 6, 6)
      assert.equal(kids.length, 2)
      assert.equal(kids[0]?.dataset.railCol, '6')
      ensureStoryShells(
        host as unknown as ParentNode,
        7,
        7,
        [{ column: 7, row: 1 }, { column: 7, row: 2 }],
      )
      assert.equal(created.length, 4)
      assert.equal(kids.length, 4)
      ensureStoryShells(
        host as unknown as ParentNode,
        8,
        8,
        [{ column: 8, row: 1 }],
      )
      assert.equal(created.length, 5)
      assert.equal(kids.length, 5)
    } finally {
      globalThis.document = prevDoc
    }
  })
})

describe('railOverflowLeft', () => {
  it('网站轨左边越界的卡不退场', () => {
    assert.equal(railOverflowLeft(-120, true), true)
    assert.equal(railOverflowLeft(40, true), true)
    assert.equal(railOverflowLeft(120, true), false)
    assert.equal(railOverflowLeft(-120, false), false)
  })
})

describe('railSlotOffsets', () => {
  it('两列同左缘只记一槽', () => {
    assert.deepEqual(
      railSlotOffsets([
        { left: 12 },
        { left: 12 },
        { left: 236 },
        { left: 236 },
        { left: 460 },
      ]),
      [0, 224, 448],
    )
  })
})

describe('railMaxScroll', () => {
  it('最后一张也能对齐到左缘', () => {
    assert.equal(railMaxScroll([0]), 0)
    assert.equal(railMaxScroll(slots), 960)
    assert.equal(railMaxScroll(slots, 44), 916)
  })
})

describe('railSettleTau / isDiscreteWheel', () => {
  it('坐槽时近处更慢', () => {
    assert.ok(railSettleTau(24, true) > railSettleTau(200, true))
    assert.ok(railSettleTau(80, false) < railSettleTau(24, true))
    assert.ok(railSettleTau(200, true, true) > railSettleTau(200, true))
  })

  it('只有刻度滚轮当行距，触控板跟像素', () => {
    assert.equal(isDiscreteWheel(wheel(120)), false)
    assert.equal(isDiscreteWheel(wheel(3, 1)), true)
    assert.equal(isDiscreteWheel(wheel(12.4)), false)
  })
})

describe('railCoastStep', () => {
  it('惯性直接积分，出界就停', () => {
    const mid = railCoastStep(200, 800, 0.016, 2000)
    assert.ok(mid.scroll > 200)
    assert.ok(mid.scroll < 220)
    assert.ok(mid.velocity > RAIL_FLING_MIN_PX_S)
    assert.ok(mid.velocity < 800)
    assert.deepEqual(railCoastStep(4, -800, 0.016, 2000), {
      scroll: 0,
      velocity: 0,
    })
    assert.deepEqual(railCoastStep(1990, 800, 0.016, 2000), {
      scroll: 2000,
      velocity: 0,
    })
  })
})

describe('followRailScroll', () => {
  it('文章滚过一个源时，网站卡从该源滑到下一源', () => {
    assert.equal(followRailScroll(0, [0, 400], [0, 200]), 0)
    assert.equal(followRailScroll(200, [0, 400], [0, 200]), 100)
    assert.equal(followRailScroll(400, [0, 400], [0, 200]), 200)
    assert.equal(followRailScroll(500, [0, 400], [0, 200]), 200)
    assert.equal(followRailScroll(150, [0, 100, 200, 300], [0, 10, 20, 30]), 15)
    assert.equal(followRailScroll(250, [0, 100, 200, 300], [0, 10, 20, 30]), 25)
    assert.equal(followRailScroll(400, [0, 100, 200, 300], [0, 10, 20, 30]), 30)
    const hint = { i: 0 }
    const drive = [0, 400, 800]
    const follow = [0, 200, 400]
    assert.equal(followRailScroll(0, drive, follow, hint), 0)
    assert.equal(hint.i, 0)
    assert.equal(followRailScroll(200, drive, follow, hint), 100)
    assert.equal(hint.i, 0)
    assert.equal(followRailScroll(400, drive, follow, hint), 200)
    assert.equal(followRailScroll(600, drive, follow, hint), 300)
    assert.equal(hint.i, 1)
    assert.equal(followRailScroll(100, drive, follow, hint), 50)
    assert.equal(hint.i, 0)
    assert.equal(followRailScroll(0.1, [0, 0.2, 400], [0, 120, 240]), 0)
  })

  it('按文章卡的源起点去跟网站卡座位', () => {
    const storyStarts = [
      { id: 8, start: 0 },
      { id: 9, start: 400 },
    ]
    assert.equal(sourceAtScroll(199, storyStarts), 8)
    assert.equal(sourceAtScroll(400, storyStarts), 9)
    assert.equal(
      sourceAtScroll(
        559,
        [
          { id: 1, start: 0 },
          { id: 2, start: 280 },
          { id: 3, start: 560 },
        ],
      ),
      2,
    )
    assert.equal(
      sourceAtScroll(
        560,
        [
          { id: 1, start: 0 },
          { id: 2, start: 280 },
          { id: 3, start: 560 },
        ],
      ),
      3,
    )
    assert.equal(
      followRailScroll(200, [0, 400], [0, 200]),
      100,
    )
  })
})

describe('usePhantasiRailPan 热路', () => {
  it('保绘轨不每帧排退场，跟轨不扫领头卡', () => {
    const src = readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), 'usePhantasiRailPan.ts'),
      'utf8',
    )
    assert.doesNotMatch(src, /rubberband/)
    assert.match(src, /到边即停/)
    assert.match(src, /不越界回弹/)
    assert.match(src, /clampConversationScroll\(home \+ wheelAcc/)
    assert.match(src, /clampConversationScroll\(target \+ dx/)
    assert.match(src, /if \(overflowLeft\) return/)
    assert.match(src, /if \(!overflowLeft\) scheduleExit/)
    assert.match(src, /if \(onLeadChangeRef\.current\) reportLead/)
    assert.match(src, /if \(!onLeadChangeRef\.current\) return/)
    assert.match(src, /pushSample/)
    assert.match(src, /sampleView/)
    assert.doesNotMatch(src, /samples\.shift/)
    assert.match(src, /onIdleRef/)
    assert.match(src, /beginGrab/)
    assert.match(
      src.slice(src.indexOf('const beginGrab'), src.indexOf('const releaseGrab')),
      /willChange/,
    )
    assert.match(src, /releaseGrab/)
    assert.match(src, /if \(grabOn\) return/)
    assert.match(src, /persistScroll/)
    const stop = src.slice(src.indexOf('const stop ='), src.indexOf('const tick ='))
    assert.match(stop, /onIdleRef\.current/)
    assert.match(stop, /persistScroll/)
    assert.match(stop, /RAIL_WHEEL_SETTLE_MS/)
    const writeTransform = src.slice(
      src.indexOf('const writeTransform ='),
      src.indexOf('const leadElAt'),
    )
    assert.doesNotMatch(writeTransform, /phantasiRailScroll/)
    const wheelIdle = src.slice(
      src.indexOf('const armWheelIdle'),
      src.indexOf('const onWheel'),
    )
    assert.match(wheelIdle, /releaseGrab/)
    assert.doesNotMatch(wheelIdle, /RAIL_WHEEL_COAST_PX_S/)
    assert.doesNotMatch(wheelIdle, /finishCoast/)
    assert.match(wheelIdle, /current = target/)
    assert.match(wheelIdle, /current = clampConversationScroll\(current, max\)/)
    const seek = src.slice(src.indexOf('const seek ='), src.indexOf('if (apiRef)'))
    assert.match(seek, /writeTransform/)
    assert.match(
      seek,
      /if \(Math.abs\(scroll - current\) < 0.5 && Math.abs\(scroll - target\) < 0.5\)/,
    )
    assert.doesNotMatch(seek, /leadElAt/)
    assert.doesNotMatch(seek, /slotAt/)
    assert.match(src, /notifyLeadAt/)
    assert.doesNotMatch(seek, /willChange/)
    assert.doesNotMatch(seek, /stop\(\)/)
    assert.doesNotMatch(seek, /paintFollowLayer/)
    assert.doesNotMatch(seek, /persistScroll/)
    assert.doesNotMatch(seek, /phantasiRailScroll/)
    assert.match(src, /if \(totalCols <= 1 \|\| colW <= 1\) recache\(\)/)
    assert.match(src, /if \(Math\.abs\(viewW - prevView\) <= 8\) return/)
    const nudge = src.slice(src.indexOf('const nudge ='), src.indexOf('const seek ='))
    assert.match(nudge, /syncColumnSlots/)
    assert.match(src, /contentRect\.width/)
    assert.match(src, /if \(trackChanged && !dragging && !grabOn\) recache\(\)/)
    assert.match(src, /if \(dragging \|\| grabOn\) return/)
    assert.doesNotMatch(
      src.slice(
        src.indexOf('if (trackChanged && !dragging && !grabOn) recache()'),
        src.indexOf('if (dragging || grabOn) return'),
      ),
      /measure\(\)/,
    )
    assert.match(src, /if \(next !== lastTransform\)/)
    assert.match(src, /restoreArmed/)
    assert.match(src, /scrollState\.scroll = current/)
    assert.match(src, /scrollNotifyFrame/)
    assert.match(src, /flushScrollNotify/)
    assert.match(src, /notifyScroll\(syncScroll\)/)
    const tick = src.slice(src.indexOf('const tick ='), src.indexOf('const kick ='))
    assert.match(tick, /if \(!arrived\) \{\n {8}write\(true\)/)
    assert.match(tick, /write\(true\)\n {6}stop\(fromPointer \? 0 : RAIL_WHEEL_SETTLE_MS\)/)
    assert.match(tick, /current = clampConversationScroll\(current, max\)/)
    assert.doesNotMatch(tick, /seating = true/)
    assert.match(src, /lastLeadCol/)
    assert.match(src, /if \(col === lastLeadCol\) return/)
    assert.match(src, /if \(fresh \|\| !cards\.length\) recache/)
    assert.match(src, /card\.el\.isConnected/)
    assert.match(src, /if \(cols > 1 && cards\.length > 0\)/)
    assert.match(src, /if \(cols > 1 && colW > 1\) \{\n {8}rebuildSlots\(\)\n {8}return/)
    assert.match(src, /slotsCols === totalCols/)
    assert.match(src, /railCoastStep/)
    assert.match(src, /is-rail-panning/)
    assert.match(src, /pointerdown/)
    assert.match(src, /RAIL_DRAG_SLOP_PX/)
    assert.match(src, /RAIL_FLING_SLOT_PX_S/)
    assert.match(src, /fromPointer/)
    assert.match(src, /stop\(fromPointer \? 0 : RAIL_WHEEL_SETTLE_MS\)/)
    assert.doesNotMatch(src, /snapSlots/)
    assert.doesNotMatch(src, /settleRailSlot/)
    assert.match(src, /if \(!cardList\)/)
    assert.match(src, /data-rail-col="\$\{col\}"/)
    const align = src.slice(src.indexOf('const align ='), src.indexOf('const relayout'))
    assert.match(align, /data-rail-id/)
    assert.match(align, /if \(col <= 0 \|\| colW <= 1\)/)
  })
})

describe('scrollFromTrackTransform', () => {
  it('读回座定位移，空轨道是 0', () => {
    assert.equal(scrollFromTrackTransform(''), 0)
    assert.equal(scrollFromTrackTransform('none'), 0)
    assert.equal(scrollFromTrackTransform('translate3d(-936px, 0, 0)'), 936)
    assert.equal(scrollFromTrackTransform('translate3d(0px, 0, 0)'), 0)
  })
})

describe('railLeadColumn / railTrackScroll', () => {
  it('领头列跟滚动对齐，热路位移跟 transform，记下的只是停稳值', () => {
    assert.equal(railLeadColumn(0, 280), 1)
    assert.equal(railLeadColumn(279, 280), 1)
    assert.equal(railLeadColumn(280, 280), 2)
    assert.equal(
      railTrackScroll({
        dataset: { phantasiRailScroll: '840' },
        style: { transform: 'translate3d(-100px, 0, 0)' },
      }),
      100,
    )
    assert.equal(
      railTrackScroll({
        dataset: { phantasiRailScroll: '840' },
        style: { transform: 'translate3d(0px, 0, 0)' },
      }),
      0,
    )
    assert.equal(
      railTrackScroll({
        dataset: { phantasiRailScroll: '840' },
        style: { transform: '' },
      }),
      840,
    )
    assert.equal(
      railTrackScroll({
        dataset: {},
        style: { transform: 'translate3d(-936px, 0, 0)' },
      }),
      936,
    )
    assert.equal(railTrackScroll(null, 12), 12)
  })
})

describe('storyCardFace', () => {
  it('同一篇文章面对象沿用，字段变了才重算', async () => {
    const { storyCardFace } = await import('./storyFace.ts')
    const times = {
      justNow: 'now',
      minutesAgo: '{minutes}m',
      hoursAgo: '{hours}h',
      daysAgo: '{days}d',
    }
    const labels = { unread: 'new', starred: 'star', unstar: 'unstar' }
    const item = {
      id: 1,
      title: 'Hello',
      summary: '<p>Hi</p>',
      image: null,
      published_at: Date.now(),
      is_read: false,
      is_starred: false,
      source_name: 'A',
    }
    const a = storyCardFace(item, times, 'en', labels)
    const b = storyCardFace(item, times, 'en', labels)
    assert.equal(a, b)
    const { storyCardInnerHtml, warmStoryCovers, warmStoryFaces } = await import('./storyFace.ts')
    warmStoryFaces([item], times, 'en', labels)
    assert.equal(storyCardFace(item, times, 'en', labels), b)
    const inner = storyCardInnerHtml(b, 'new', 'star', 'unstar', true, true)
    assert.equal(storyCardInnerHtml(b, 'new', 'star', 'unstar', true, true), inner)
    assert.match(inner, /phantasi-story__title/)
    assert.match(inner, /Hello/)
    warmStoryCovers([item])
    item.is_starred = true
    const c = storyCardFace(item, times, 'en', labels)
    assert.notEqual(c, a)
    assert.equal(c.starred, true)
  })

  it('游客不显示未读，登录后按 is_read 显示', async () => {
    const { storyCardFace } = await import('./storyFace.ts')
    const { phantasiSubject } = await import('../../../utils/phantasiSubject.ts')
    const times = { justNow: 'now', minutesAgo: '{minutes}m', hoursAgo: '{hours}h', daysAgo: '{days}d' }
    const labels = { unread: 'new', starred: 'star', unstar: 'unstar' }
    const item = {
      id: 2,
      title: 'Unread',
      summary: '',
      image: null,
      published_at: Date.now(),
      is_read: false,
      is_starred: false,
      source_name: 'A',
    }
    phantasiSubject.change('guest')
    assert.equal(storyCardFace(item, times, 'en', labels).unread, false)
    phantasiSubject.change('user:1:member')
    try {
      assert.equal(storyCardFace(item, times, 'en', labels).unread, true)
    } finally {
      phantasiSubject.change('guest')
    }
  })
})
