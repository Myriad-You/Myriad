/**
 * 控制面板小组件行数切换重排的单元测试。
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/ControlPanel/widgetReflow.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  packControlPanelWidgets,
  PAGE_COLS,
  PAGE_COUNT,
  parseWidgetSize,
  widgetsOverlap,
} from './widgetReflow.ts'

interface W { id: string, size: string, position: { x: number, y: number } }
const w = (id: string, size: string, x = 0, y = 0): W => ({ id, size, position: { x, y } })

/** 断言结果里任意两个小组件都不相交。 */
function assertNoOverlap(items: W[]) {
  for (let i = 0; i < items.length; i++) {
    for (let j = i + 1; j < items.length; j++) {
      assert.equal(
        widgetsOverlap(items[i], items[j]),
        false,
        `${items[i].id}@(${items[i].position.x},${items[i].position.y},${items[i].size}) 与 `
        + `${items[j].id}@(${items[j].position.x},${items[j].position.y},${items[j].size}) 重叠`,
      )
    }
  }
}

describe('parseWidgetSize', () => {
  it('解析 WxH', () => {
    assert.deepEqual(parseWidgetSize('4x1'), { w: 4, h: 1 })
    assert.deepEqual(parseWidgetSize('2x2'), { w: 2, h: 2 })
  })
  it('非法输入返回 null', () => {
    for (const s of ['', 'x', '4x', 'ax1', '0x1', '4x0', '4X1', '4-1']) {
      assert.equal(parseWidgetSize(s), null, s)
    }
  })
})

describe('旧实现会造成的重叠（回归用例）', () => {
  it('默认布局的两个 2x2 压成 4x1 后不再重叠', () => {
    // 旧实现：只改 size 保留 x → [0,4) 与 [2,6) 重叠两列
    const legacy = [w('a', '4x1', 0, 0), w('b', '4x1', 2, 0)]
    assert.equal(widgetsOverlap(legacy[0], legacy[1]), true, '前置条件：旧结果确实重叠')

    const packed = packControlPanelWidgets([w('a', '4x1', 0, 0), w('b', '4x1', 2, 0)], 1)
    assertNoOverlap(packed)
    assert.deepEqual(packed.map(p => p.position), [{ x: 0, y: 0 }, { x: 4, y: 0 }])
  })

  it('y 被归零的一组 2x2 切回 2 行后不再重叠', () => {
    // 旧实现切回 2 行只恢复 size、不动 position，于是全挤在 y=0
    const legacy = [w('a', '2x2', 0, 0), w('b', '2x2', 0, 0)]
    assert.equal(widgetsOverlap(legacy[0], legacy[1]), true, '前置条件：旧结果确实重叠')

    const packed = packControlPanelWidgets(legacy, 2)
    assertNoOverlap(packed)
    assert.deepEqual(packed.map(p => p.position), [{ x: 0, y: 0 }, { x: 2, y: 0 }])
  })

  it('1↔2 行来回切换任意多次都不产生重叠', () => {
    let items = [w('a', '2x2', 0, 0), w('b', '2x2', 2, 0), w('c', '2x2', 4, 0)]
    for (let i = 0; i < 6; i++) {
      const rows = i % 2 === 0 ? 1 : 2
      const size = rows === 1 ? '4x1' : '2x2'
      items = packControlPanelWidgets(items.map(x => ({ ...x, size })), rows)
      assertNoOverlap(items)
      assert.equal(items.length, 3, `第 ${i} 次切换后丢了小组件`)
    }
  })
})

describe('打包规则', () => {
  it('保持传入顺序', () => {
    const packed = packControlPanelWidgets(
      [w('a', '2x2'), w('b', '2x2'), w('c', '2x2')], 2,
    )
    assert.deepEqual(packed.map(p => p.id), ['a', 'b', 'c'])
  })

  it('不跨页：一页装不下就整块挪到下一页', () => {
    // 2x2 + 4x1：4x1 装不进同页剩下的 2 列，必须换页
    const packed = packControlPanelWidgets([w('a', '2x2'), w('b', '4x1')], 2)
    assert.equal(packed[1].position.x, PAGE_COLS, '4x1 应落在第 2 页页首')
    assertNoOverlap(packed)
  })

  it('每个小组件都落在单一页内，不跨页边界', () => {
    const packed = packControlPanelWidgets(
      Array.from({ length: 6 }, (_, i) => w(`w${i}`, '2x2')), 2,
    )
    for (const p of packed) {
      const span = parseWidgetSize(p.size)!
      const startPage = Math.floor(p.position.x / PAGE_COLS)
      const endPage = Math.floor((p.position.x + span.w - 1) / PAGE_COLS)
      assert.equal(startPage, endPage, `${p.id} 跨了页边界`)
    }
  })

  it('先填满一行再换行', () => {
    const packed = packControlPanelWidgets(
      [w('a', '2x1'), w('b', '2x1'), w('c', '2x1')], 2,
    )
    assert.deepEqual(
      packed.map(p => p.position),
      [{ x: 0, y: 0 }, { x: 2, y: 0 }, { x: 0, y: 1 }],
    )
  })

  it('高于当前行数的尺寸放不下，直接丢弃', () => {
    const packed = packControlPanelWidgets([w('a', '2x2'), w('b', '4x1')], 1)
    assert.deepEqual(packed.map(p => p.id), ['b'], '2x2 在 1 行网格里无处安放')
  })

  it('超出总容量的部分被丢弃，且不会挤占已放置的位置', () => {
    // 1 行模式每页只放得下一个 4x1，共 3 页
    const packed = packControlPanelWidgets(
      Array.from({ length: 5 }, (_, i) => w(`w${i}`, '4x1')), 1,
    )
    assert.equal(packed.length, PAGE_COUNT)
    assert.deepEqual(packed.map(p => p.position.x), [0, 4, 8])
    assertNoOverlap(packed)
  })

  it('尺寸无法解析的条目被丢弃而不是留在原位造成重叠', () => {
    const packed = packControlPanelWidgets([w('bad', 'oops'), w('ok', '2x2')], 2)
    assert.deepEqual(packed.map(p => p.id), ['ok'])
  })

  it('rows <= 0 时返回空', () => {
    assert.deepEqual(packControlPanelWidgets([w('a', '2x2')], 0), [])
  })

  it('不修改传入对象（返回新引用）', () => {
    const input = [w('a', '2x2', 9, 9)]
    const packed = packControlPanelWidgets(input, 2)
    assert.deepEqual(input[0].position, { x: 9, y: 9 }, '原对象被就地改动了')
    assert.notEqual(packed[0], input[0])
  })

  it('保留 size 之外的自定义字段', () => {
    const packed = packControlPanelWidgets(
      [{ ...w('a', '2x2'), type: 'weather', config: { k: 1 } }], 2,
    )
    assert.equal(packed[0].type, 'weather')
    assert.deepEqual(packed[0].config, { k: 1 })
  })
})

describe('输出恒不重叠（随机用例）', () => {
  it('随机尺寸组合在 1 行与 2 行下都不重叠', () => {
    const sizes = ['1x1', '2x1', '1x2', '2x2', '4x1', '4x2', '3x2', '2x3']
    let seed = 42
    const rand = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648
    for (let round = 0; round < 200; round++) {
      const n = 1 + Math.floor(rand() * 8)
      const items = Array.from({ length: n }, (_, i) =>
        w(`w${i}`, sizes[Math.floor(rand() * sizes.length)], 0, 0))
      for (const rows of [1, 2]) {
        assertNoOverlap(packControlPanelWidgets(items, rows))
      }
    }
  })
})
