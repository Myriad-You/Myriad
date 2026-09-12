import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  BREW_TAG_ENTER_MS,
  BREW_TAG_EXIT_MS,
  BREW_TAG_STAGGER_MS,
  brewTagDelay,
} from './brewTag.ts'

describe('brewTagDelay', () => {
  it('第一枚立刻走', () => {
    assert.equal(brewTagDelay(0), 0)
  })

  it('按序号错开，对齐 --sm-stagger', () => {
    assert.equal(BREW_TAG_STAGGER_MS, 26)
    assert.equal(brewTagDelay(1), 26)
    assert.equal(brewTagDelay(3), 78)
  })

  it('降级时全部立刻走', () => {
    assert.equal(brewTagDelay(4, true), 0)
  })

  it('入场时长对齐 --sm-dur-slow', () => {
    assert.equal(BREW_TAG_ENTER_MS, 320)
  })

  it('退场时长对齐 --sm-dur-base', () => {
    assert.equal(BREW_TAG_EXIT_MS, 220)
  })
})
