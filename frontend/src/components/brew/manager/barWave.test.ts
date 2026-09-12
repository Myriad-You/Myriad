import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  waveAfterBoardEdit,
  waveFromLane,
  waveIfEditTargetLost,
  waveOnChange,
  waveOnClose,
} from './barWave.ts'

describe('waveFromLane', () => {
  it('收藏编辑压过收藏，收藏压过主题流', () => {
    assert.equal(
      waveFromLane({ starredEdit: true, hasStarred: true, hasTopic: true }),
      'starred-edit',
    )
    assert.equal(
      waveFromLane({ starredEdit: false, hasStarred: true, hasTopic: true }),
      'starred',
    )
    assert.equal(
      waveFromLane({ starredEdit: false, hasStarred: false, hasTopic: true }),
      'topic-feed',
    )
    assert.equal(
      waveFromLane({ starredEdit: false, hasStarred: false, hasTopic: false }),
      'default',
    )
  })
})

describe('waveAfterBoardEdit / waveIfEditTargetLost', () => {
  it('进编辑落到 edit，退出回到 default；全屏编辑丢源要退', () => {
    assert.equal(waveAfterBoardEdit(true, 'default'), 'edit')
    assert.equal(waveAfterBoardEdit(true, 'edit'), null)
    assert.equal(waveAfterBoardEdit(false, 'source-edit'), 'default')
    assert.equal(waveIfEditTargetLost('source-edit', false, true), 'edit')
    assert.equal(waveIfEditTargetLost('source-edit', true, true), null)
  })
})

describe('waveOnClose / waveOnChange', () => {
  it('关全屏编辑回到多选；离开编辑才清编辑态', () => {
    assert.deepEqual(waveOnClose('source-edit'), {
      mode: 'edit',
      exitEdit: false,
    })
    assert.deepEqual(waveOnClose('edit'), { mode: 'default', exitEdit: true })
    assert.deepEqual(waveOnChange('source-edit', 'edit'), {
      enterEdit: true,
      exitEdit: false,
      clearSearch: false,
    })
    assert.deepEqual(waveOnChange('default', 'edit'), {
      enterEdit: false,
      exitEdit: true,
      clearSearch: false,
    })
    assert.equal(waveOnChange('search', 'default').clearSearch, true)
  })
})
