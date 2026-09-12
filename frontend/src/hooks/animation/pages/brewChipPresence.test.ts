import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  awaitLaneSwap,
  BREW_CARD_EXIT_TRANSFORM,
  BREW_SURFACE_CARD_CAP,
  BREW_TAG_EXIT_TRANSFORM,
  BREW_TAG_SWAP_PAD_MS,
  brewSurfaceSwapWait,
  brewTagSwapWait,
  chipEnterFrames,
  chipExitFrames,
  diffChipKeys,
  planChipLaneSwap,
  shouldPlayChipEnter,
} from './brewChipPresence.ts'
import { BREW_TAG_EXIT_MS, brewTagDelay } from './brewTag.ts'

describe('diffChipKeys', () => {
  it('整栏替换先退后进', () => {
    const diff = diffChipKeys(['d:sort', 'd:search'], ['s:field', 's:close'])
    assert.equal(diff.fullSwap, true)
    assert.deepEqual(diff.leave, ['d:sort', 'd:search'])
    assert.deepEqual(diff.enter, ['s:field', 's:close'])
    assert.deepEqual(diff.stay, [])
  })

  it('只加一枚不是整栏替换', () => {
    const diff = diffChipKeys(['d:sort', 'd:search'], [
      'd:sort',
      'd:edit',
      'd:search',
    ])
    assert.equal(diff.fullSwap, false)
    assert.deepEqual(diff.enter, ['d:edit'])
    assert.deepEqual(diff.leave, [])
  })

  it('同一组只刷新不算进出', () => {
    const diff = diffChipKeys(['s:field', 's:close'], ['s:field', 's:close'])
    assert.equal(diff.fullSwap, false)
    assert.deepEqual(diff.enter, [])
    assert.deepEqual(diff.leave, [])
  })
})

describe('planChipLaneSwap', () => {
  it('退场中只改目的地', () => {
    assert.equal(planChipLaneSwap('default', 'search', true), 'retarget')
    assert.equal(planChipLaneSwap('default', 'default', true), 'retarget')
  })

  it('同波次不重开', () => {
    assert.equal(planChipLaneSwap('search', 'search', false), 'hold')
  })

  it('换波次才开退场', () => {
    assert.equal(planChipLaneSwap('default', 'search', false), 'start-exit')
  })
})

describe('chipExitFrames', () => {
  it('从当前透明度退，不从 1 起笔', () => {
    const frames = chipExitFrames('0.4', 'none')
    assert.equal(frames[0]?.opacity, '0.4')
    assert.equal(frames[0]?.transform, 'none')
    assert.equal(frames[1]?.opacity, 0)
    assert.equal(frames[1]?.transform, BREW_TAG_EXIT_TRANSFORM)
  })

  it('卡片可换成不带缩放的落点', () => {
    const frames = chipExitFrames('1', 'none', BREW_CARD_EXIT_TRANSFORM)
    assert.equal(frames[1]?.transform, BREW_CARD_EXIT_TRANSFORM)
  })
})

describe('brewTagSwapWait', () => {
  it('等最后一枚退完', () => {
    assert.equal(BREW_TAG_EXIT_MS, 220)
    assert.equal(
      brewTagSwapWait(3),
      brewTagDelay(2) + BREW_TAG_EXIT_MS + BREW_TAG_SWAP_PAD_MS,
    )
  })

  it('降级立刻切', () => {
    assert.equal(brewTagSwapWait(4, true), 0)
  })
})

describe('chipEnterFrames', () => {
  it('从退场姿态进到当前值', () => {
    const frames = chipEnterFrames('1', 'none')
    assert.equal(frames[0]?.opacity, 0)
    assert.equal(frames[0]?.transform, BREW_TAG_EXIT_TRANSFORM)
    assert.equal(frames[1]?.opacity, '1')
    assert.equal(frames[1]?.transform, 'none')
  })
})

describe('shouldPlayChipEnter', () => {
  it('退场中不播入场', () => {
    assert.equal(shouldPlayChipEnter(true), false)
    assert.equal(shouldPlayChipEnter(false), true)
  })
})

describe('awaitLaneSwap', () => {
  it('finished 立刻回来也要满拍', async () => {
    const started = Date.now()
    await awaitLaneSwap(Promise.resolve(), 40)
    assert.ok(Date.now() - started >= 35)
  })
})

describe('brewSurfaceSwapWait', () => {
  it('卡片多时以封顶错开为准', () => {
    assert.equal(
      brewSurfaceSwapWait(2, 20),
      brewTagSwapWait(BREW_SURFACE_CARD_CAP),
    )
  })

  it('栏更长时听栏的', () => {
    assert.equal(brewSurfaceSwapWait(6, 2), brewTagSwapWait(6))
  })
})
