/**
 * 控制面板小组件自动翻页闸门的单元测试。
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/ControlPanel/widgetCarousel.test.ts
 */

import type { WidgetCarouselGate } from './widgetCarousel.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isHoverCapablePointer,
  shouldAutoAdvanceWidgets,
} from './widgetCarousel.ts'

/** 唯一会自动翻页的组合。 */
const RUNNING: WidgetCarouselGate = {
  isEditMode: false,
  maxPage: 1,
  panelVisible: true,
  isHovering: false,
}

describe('shouldAutoAdvanceWidgets', () => {
  it('面板可见、有多页、非编辑、指针不在区域内时才自动翻页', () => {
    assert.equal(shouldAutoAdvanceWidgets(RUNNING), true)
  })

  it('只有一页时不轮播', () => {
    assert.equal(
      shouldAutoAdvanceWidgets({ ...RUNNING, maxPage: 0 }),
      false,
    )
  })

  it('编辑模式下由用户自己翻页', () => {
    assert.equal(
      shouldAutoAdvanceWidgets({ ...RUNNING, isEditMode: true }),
      false,
    )
  })

  it('面板收起 / 切到通知页时不在看不见的子树上翻页', () => {
    assert.equal(
      shouldAutoAdvanceWidgets({ ...RUNNING, panelVisible: false }),
      false,
    )
  })

  it('指针停在小组件上时暂停，避免打断阅读或点击', () => {
    assert.equal(
      shouldAutoAdvanceWidgets({ ...RUNNING, isHovering: true }),
      false,
    )
  })

  it('任意一项不满足就停：四个输入都是独立的一票否决', () => {
    const overrides: Array<Partial<WidgetCarouselGate>> = [
      { isEditMode: true },
      { maxPage: 0 },
      { panelVisible: false },
      { isHovering: true },
    ]
    for (const o of overrides) {
      assert.equal(
        shouldAutoAdvanceWidgets({ ...RUNNING, ...o }),
        false,
        JSON.stringify(o),
      )
    }
  })

  it('负数页索引也当作没有可轮播内容', () => {
    assert.equal(
      shouldAutoAdvanceWidgets({ ...RUNNING, maxPage: -1 }),
      false,
    )
  })
})

describe('isHoverCapablePointer', () => {
  it('只把鼠标当成会粘滞的 hover，触屏 / 笔不暂停轮播', () => {
    assert.equal(isHoverCapablePointer('mouse'), true)
    assert.equal(isHoverCapablePointer('touch'), false)
    assert.equal(isHoverCapablePointer('pen'), false)
    assert.equal(isHoverCapablePointer(''), false)
  })
})
