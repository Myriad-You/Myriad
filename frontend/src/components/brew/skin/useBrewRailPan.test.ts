import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  isDiscreteWheel,
  nearestRailSlot,
  neighborRailSlot,
  RAIL_COMMIT_RATIO,
  RAIL_FLING_SLOT_PX_S,
  railLeadIndex,
  railMaxScroll,
  railOverflowLeft,
  railSeatScroll,
  railSeatSlots,
  railSettleTau,
  railSlotOffsets,
  scrollFromTrackTransform,
  settleRailSlot,
} from './railPan.ts'

const cards = [0, 320, 640, 960].map((left) => ({ left, width: 304 }))
const slots = railSlotOffsets(cards)
const max = 960

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
  })
})

describe('railSeatSlots', () => {
  it('吸入槽位跟溢出座定同一落点', () => {
    assert.deepEqual(railSeatSlots(slots, 0), [0, 320, 640, 960])
    assert.deepEqual(railSeatSlots(slots, 44), [0, 276, 596, 916])
    assert.equal(railSeatSlots(slots, 44)[2], railSeatScroll(cards, 2, 44))
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

describe('settleRailSlot', () => {
  it('没推过阈值就弹回当前槽', () => {
    const stay = 320 * (RAIL_COMMIT_RATIO - 0.04)
    assert.equal(settleRailSlot(stay, 0, slots, max), 0)
  })

  it('推过阈值就坐进下一槽', () => {
    const push = 320 * (RAIL_COMMIT_RATIO + 0.04)
    assert.equal(settleRailSlot(push, 0, slots, max), 320)
  })

  it('快甩至少进一格，远则跳多格', () => {
    assert.equal(
      settleRailSlot(20, RAIL_FLING_SLOT_PX_S + 10, slots, max),
      320,
    )
    assert.equal(settleRailSlot(20, 2400, slots, max), 640)
  })

  it('回甩至少退一格，远则跳多格', () => {
    assert.equal(
      settleRailSlot(320, -(RAIL_FLING_SLOT_PX_S + 10), slots, max, 320),
      0,
    )
    assert.equal(settleRailSlot(500, -2400, slots, max, 640), 0)
  })

  it('相对起手槽往回推过阈值就坐进上一张', () => {
    const stay = 320 - 320 * (RAIL_COMMIT_RATIO - 0.04)
    const push = 320 - 320 * (RAIL_COMMIT_RATIO + 0.04)
    assert.equal(settleRailSlot(stay, 0, slots, max, 320), 320)
    assert.equal(settleRailSlot(push, 0, slots, max, 320), 0)
  })

  it('一次推过好几格才吸入更远，轻推只进一格', () => {
    assert.equal(settleRailSlot(400, 0, slots, max, 0), 320)
    assert.equal(settleRailSlot(800, 0, slots, max, 0), 640)
    assert.equal(settleRailSlot(500, 0, slots, max, 640), 320)
  })

  it('轻甩即使速度快也只进一格', () => {
    assert.equal(settleRailSlot(80, 1200, slots, max, 0), 320)
  })
})

describe('neighborRailSlot', () => {
  it('滚轮一格推入下一张', () => {
    assert.equal(neighborRailSlot(0, 1, slots, max), 320)
    assert.equal(neighborRailSlot(320, 1, slots, max), 640)
  })

  it('往回一格到上一张', () => {
    assert.equal(neighborRailSlot(80, -1, slots, max), 0)
    assert.equal(neighborRailSlot(320, -1, slots, max), 0)
    assert.equal(neighborRailSlot(640, -1, slots, max), 320)
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

describe('nearestRailSlot', () => {
  it('贴最近的左缘', () => {
    assert.equal(nearestRailSlot(10, slots, max), 0)
    assert.equal(nearestRailSlot(300, slots, max), 320)
  })
})

describe('railSettleTau / isDiscreteWheel', () => {
  it('坐槽时近处更慢', () => {
    assert.ok(railSettleTau(24, true) > railSettleTau(200, true))
    assert.ok(railSettleTau(80, false) < railSettleTau(24, true))
  })

  it('只有刻度滚轮当一格，触控板像素量走吸入', () => {
    assert.equal(isDiscreteWheel(wheel(120)), false)
    assert.equal(isDiscreteWheel(wheel(3, 1)), true)
    assert.equal(isDiscreteWheel(wheel(12.4)), false)
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
