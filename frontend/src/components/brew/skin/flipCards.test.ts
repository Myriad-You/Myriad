import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  FLIP_INTRO_STORY_MS,
  FLIP_SITE_MS,
  FLIP_STAGGER_CAP_MS,
  FLIP_STAGGER_MS,
  FLIP_STORY_FOLLOW_MS,
  FLIP_STORY_LEAD_MS,
  FLIP_STORY_MS,
  flipDelay,
  flipDelayFromLead,
  flipWaitMs,
  revealFeedsTree,
  SITE_ENTER,
  SITE_ENTER_LEFT,
  SITE_GRID_OP,
  SITE_RAIL_OP,
  siteOpenDelay,
  siteRestOpacity,
  storyColumn,
  storyDelay,
  storyPhaseDelay,
} from './flipCards.ts'

describe('flipDelay', () => {
  it('焦点卡立刻动', () => {
    assert.equal(flipDelay(9, true), 0)
  })

  it('按序号错开，封顶', () => {
    assert.equal(flipDelay(0), 0)
    assert.equal(flipDelay(2), FLIP_STAGGER_MS * 2)
    assert.ok(flipDelay(80) <= FLIP_STAGGER_CAP_MS)
  })

  it('从焦点向外错开', () => {
    assert.equal(flipDelayFromLead(5, 5), 0)
    assert.equal(flipDelayFromLead(3, 5), FLIP_STAGGER_MS * 2)
    assert.equal(flipDelayFromLead(8, 5), FLIP_STAGGER_MS * 3)
    assert.equal(flipDelayFromLead(2, null), FLIP_STAGGER_MS * 2)
    assert.ok(flipDelayFromLead(80, 0) <= FLIP_STAGGER_CAP_MS)
  })

  it('网站卡沿轨道入场，前一张从左边来', () => {
    assert.match(SITE_ENTER, /24px, 0, 0/)
    assert.match(SITE_ENTER_LEFT, /-24px, 0, 0/)
  })

  it('文章卡按列错开，同一列一起走', () => {
    assert.equal(storyColumn(0), 0)
    assert.equal(storyColumn(1), 0)
    assert.equal(storyColumn(2), 1)
    assert.equal(storyDelay(0), 0)
    assert.equal(storyDelay(1), 0)
    assert.equal(storyDelay(2), FLIP_STAGGER_MS)
    assert.equal(storyDelay(0, FLIP_STORY_FOLLOW_MS), FLIP_STORY_FOLLOW_MS)
  })

  it('展开先退文章再飞网站，收回先落网站再进文章，中间不空拍', () => {
    assert.equal(siteOpenDelay('exit'), FLIP_STORY_LEAD_MS)
    assert.equal(siteOpenDelay('enter'), 0)
    assert.equal(storyPhaseDelay('enter'), FLIP_STORY_FOLLOW_MS)
    assert.equal(storyPhaseDelay('exit'), 0)
    assert.ok(FLIP_STORY_LEAD_MS < FLIP_STORY_MS)
    assert.ok(FLIP_STORY_FOLLOW_MS < FLIP_SITE_MS)
    assert.ok(FLIP_INTRO_STORY_MS < FLIP_STORY_FOLLOW_MS)
    assert.ok(
      flipWaitMs() >=
        FLIP_STORY_FOLLOW_MS + FLIP_STORY_MS + FLIP_STAGGER_CAP_MS,
    )
    assert.ok(
      flipWaitMs() >= FLIP_STORY_LEAD_MS + FLIP_SITE_MS + FLIP_STAGGER_CAP_MS,
    )
  })

  it('换树揭开合树时空根是空操作', () => {
    revealFeedsTree(null)
    revealFeedsTree(undefined)
  })

  it('开合落点透明度跟皮肤走，不读隐藏真卡', () => {
    assert.equal(siteRestOpacity(false, false), SITE_RAIL_OP)
    assert.equal(siteRestOpacity(true, false), SITE_GRID_OP)
    assert.equal(siteRestOpacity(false, true), 1)
    assert.equal(siteRestOpacity(true, true), 1)
  })
})
