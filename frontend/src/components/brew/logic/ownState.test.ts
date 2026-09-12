import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { ownItemState } from './ownState.ts'

describe('ownItemState', () => {
  it('没打开文章是 unknown；网络搜索和空源是 not-own', () => {
    assert.equal(ownItemState(null, null, false), 'unknown')
    assert.equal(
      ownItemState({ fromWebSearch: true, source_id: 3 }, null, true),
      'not-own',
    )
    assert.equal(ownItemState({ source_id: 0 }, null, true), 'not-own')
  })

  it('源未加载保持 unknown；加载后找不到源是 not-own', () => {
    assert.equal(ownItemState({ source_id: 4 }, null, false), 'unknown')
    assert.equal(ownItemState({ source_id: 4 }, null, true), 'not-own')
  })

  it('分类含「我」且非 admin_only 才是 own', () => {
    assert.equal(
      ownItemState({ source_id: 4 }, { category: '我' }, true),
      'own',
    )
    assert.equal(
      ownItemState(
        { source_id: 4 },
        { category: '我', admin_only: true },
        true,
      ),
      'not-own',
    )
    assert.equal(
      ownItemState({ source_id: 4 }, { category: '友情链接' }, true),
      'not-own',
    )
  })
})
