/**
 * 装箱与分页的单元测试。
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/brew/logic/pack.test.ts
 */

import type { BrewTileSize } from './layout.ts'
import type { BrewCard, PackedCard } from './pack.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { makeSource } from './fixtures.ts'
import {
  flattenPackedCards,
  packBrewCards,
  packedCardsOverlap,
  parseTileSize,
} from './pack.ts'

/** 生产网格：16 列 × 4 行，与首页 WidgetGrid 同一套格子。 */
const COLS = 16
const ROWS = 4

function card(key: string, size: BrewTileSize): BrewCard {
  return { kind: 'source', key, src: makeSource({ id: Number(key) || 1 }), size }
}

function assertNoOverlap(page: PackedCard[]) {
  for (let i = 0; i < page.length; i++) {
    for (let j = i + 1; j < page.length; j++) {
      assert.equal(
        packedCardsOverlap(page[i], page[j]),
        false,
        `${page[i].key}@(${page[i].x},${page[i].y},${page[i].size}) 与 ` +
          `${page[j].key}@(${page[j].x},${page[j].y},${page[j].size}) 重叠`,
      )
    }
  }
}

function assertInBounds(page: PackedCard[], cols: number, rows: number) {
  for (const c of page) {
    const span = parseTileSize(c.size)!
    assert.ok(c.x >= 0 && c.x + span.w <= cols, `${c.key} 越界 x=${c.x}`)
    assert.ok(c.y >= 0 && c.y + span.h <= rows, `${c.key} 越界 y=${c.y}`)
  }
}

describe('parseTileSize', () => {
  it('解析 WxH', () => {
    assert.deepEqual(parseTileSize('4x2'), { w: 4, h: 2 })
    assert.deepEqual(parseTileSize('2x2'), { w: 2, h: 2 })
    assert.deepEqual(parseTileSize('4x4'), { w: 4, h: 4 })
  })
  it('非法输入返回 null', () => {
    for (const s of ['', 'x', '4x', 'ax1', '0x1', '4x0', '4X2', '4-2']) {
      assert.equal(parseTileSize(s), null, s)
    }
  })
})

describe('packBrewCards', () => {
  it('空输入返回空页数组', () => {
    assert.deepEqual(packBrewCards([], COLS, ROWS), [])
  })

  it('有卡时至少一页', () => {
    const pages = packBrewCards([card('1', '2x2')], COLS, ROWS)
    assert.equal(pages.length, 1)
    assert.equal(pages[0].length, 1)
  })

  it('先填行再换行', () => {
    const cards = Array.from({ length: 4 }, (_, i) => card(String(i + 1), '4x2'))
    const [page] = packBrewCards(cards, COLS, ROWS)
    assert.deepEqual(
      page.map((c) => [c.x, c.y]),
      [
        [0, 0],
        [4, 0],
        [8, 0],
        [12, 0],
      ],
    )
  })

  it('一行填满后落到下一行', () => {
    const cards = Array.from({ length: 5 }, (_, i) => card(String(i + 1), '4x2'))
    const [page] = packBrewCards(cards, COLS, ROWS)
    assert.deepEqual(page[4] && [page[4].x, page[4].y], [0, 2])
  })

  it('16x4 一页只放得下 4 张 4x4', () => {
    const cards = Array.from({ length: 8 }, (_, i) => card(String(i + 1), '4x4'))
    const pages = packBrewCards(cards, COLS, ROWS)
    assert.equal(pages.length, 2, '8 张 4x4 只能放 4 张一页')
    assert.equal(pages[0].length, 4)
    assert.equal(pages[1].length, 4)
    pages.forEach((p) => {
      assertNoOverlap(p)
      assertInBounds(p, COLS, ROWS)
    })
  })

  it('16x4 一页放得下 16 张 2x2（8 列 × 2 行）', () => {
    const cards = Array.from({ length: 17 }, (_, i) =>
      card(String(i + 1), '2x2'),
    )
    const pages = packBrewCards(cards, COLS, ROWS)
    assert.equal(pages.length, 2)
    assert.equal(pages[0].length, 16)
    assert.equal(pages[1].length, 1)
    pages.forEach((p) => {
      assertNoOverlap(p)
      assertInBounds(p, COLS, ROWS)
    })
  })

  it('不重叠、不越界、不丢卡（混合尺寸）', () => {
    const sizes: BrewTileSize[] = ['4x4', '2x2', '4x2', '2x2', '4x4', '4x2', '2x2']
    const cards = Array.from({ length: 60 }, (_, i) =>
      card(String(i + 1), sizes[i % sizes.length]),
    )
    const pages = packBrewCards(cards, COLS, ROWS)
    assert.ok(pages.length > 1)
    pages.forEach((p) => {
      assertNoOverlap(p)
      assertInBounds(p, COLS, ROWS)
    })
    assert.deepEqual(
      flattenPackedCards(pages).map((c) => c.key),
      cards.map((c) => c.key),
      '装箱顺序必须等于输入顺序（键盘 j/k 依赖这一点）',
    )
  })

  it('一张卡不跨页', () => {
    // 一页 16 列；第 5 张 4x2 只能进下一行而不是横跨页边界
    const cards = Array.from({ length: 9 }, (_, i) => card(String(i + 1), '4x2'))
    const pages = packBrewCards(cards, COLS, ROWS)
    assert.equal(pages[0].length, 8, '16x4 装 8 张 4x2 满页')
    assert.equal(pages[1].length, 1)
    assertInBounds(pages[0], COLS, ROWS)
  })

  it('breakOn 在边界强制换页', () => {
    const cards: BrewCard[] = [
      card('1', '2x2'),
      card('2', '2x2'),
      card('3', '2x2'),
    ]
    const pages = packBrewCards(cards, COLS, ROWS, {
      breakOn: (_prev, next) => next.key === '3',
    })
    assert.equal(pages.length, 2)
    assert.deepEqual(pages[0].map((c) => c.key), ['1', '2'])
    assert.deepEqual(pages[1].map((c) => c.key), ['3'])
  })

  it('breakOn 命中在空页时不再开新页（不产生空白页）', () => {
    const pages = packBrewCards([card('1', '2x2'), card('2', '2x2')], COLS, ROWS, {
      breakOn: () => true,
    })
    assert.equal(pages.length, 2)
    assert.ok(pages.every((p) => p.length > 0), '不允许出现空白页')
  })

  it('phone 4x4 网格：4x2 满宽，2x2 两列', () => {
    const cards: BrewCard[] = [card('1', '4x2'), card('2', '2x2'), card('3', '2x2')]
    const pages = packBrewCards(cards, 4, 4)
    assert.equal(pages.length, 1)
    assert.deepEqual(
      pages[0].map((c) => [c.x, c.y]),
      [
        [0, 0],
        [0, 2],
        [2, 2],
      ],
    )
  })

  it('超过一页容量的尺寸不丢卡（夹进网格）', () => {
    // 理论上 tileSize 已按 band 降过档，不该出现；夹住是保命而不是设计。
    // 卡片自身的 size 保持原样（渲染侧照声明画），这里只保证不被静默吞掉。
    const oversized = { ...card('1', '4x4'), size: '8x8' as BrewTileSize }
    const pages = packBrewCards([oversized, card('2', '2x2')], 4, 4)
    const flat = flattenPackedCards(pages)
    assert.equal(flat.length, 2)
    flat.forEach((c) => {
      assert.ok(c.x >= 0 && c.x < 4, `${c.key} 的 x 越界`)
      assert.ok(c.y >= 0 && c.y < 4, `${c.key} 的 y 越界`)
    })
  })

  it('cols / rows 非法时返回空页列表（没有可渲染的页）', () => {
    assert.deepEqual(packBrewCards([], 0, 4), [])
    assert.deepEqual(packBrewCards([card('1', '2x2')], 0, 4), [])
    assert.deepEqual(packBrewCards([card('1', '2x2')], 16, 0), [])
  })
})
