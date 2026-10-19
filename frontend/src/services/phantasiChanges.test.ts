import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { phantasiWriteTopic } from './phantasiChanges'

describe('phantasiWriteTopic', () => {
  it('笔记、分类、评论、审核的写请求各归一类', () => {
    assert.equal(phantasiWriteTopic('PUT', '/notes/docs/3'), 'notes')
    assert.equal(phantasiWriteTopic('POST', '/notes/docs/3/publish'), 'notes')
    assert.equal(phantasiWriteTopic('DELETE', '/notes/7'), 'notes')
    assert.equal(phantasiWriteTopic('POST', '/notes'), 'notes')
    assert.equal(phantasiWriteTopic('PUT', '/categories/2'), 'categories')
    assert.equal(phantasiWriteTopic('POST', '/items/9/comments'), 'comments')
    assert.equal(phantasiWriteTopic('DELETE', '/comments/4'), 'comments')
    assert.equal(phantasiWriteTopic('POST', '/applications/5/approve'), 'reviews')
  })

  it('读请求、不改数据的 POST、和创作台无关的写都不算', () => {
    assert.equal(phantasiWriteTopic('GET', '/notes/docs'), null)
    assert.equal(phantasiWriteTopic('POST', '/notes/preview'), null)
    assert.equal(phantasiWriteTopic('PUT', '/notes/editor-preference'), null)
    assert.equal(phantasiWriteTopic('PUT', '/notes/rss'), null)
    assert.equal(phantasiWriteTopic('POST', '/items/9/read'), null)
    assert.equal(phantasiWriteTopic('POST', '/sources'), null)
  })
})
