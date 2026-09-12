import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { brewItemFromWebSearch, webSearchInList } from './webSearchItem.ts'

describe('brewItemFromWebSearch', () => {
  it('标成网络搜索，正文可空', () => {
    const item = brewItemFromWebSearch(
      { id: 8, title: 'hit', sourceName: '外站', summary: 's' },
      '网络搜索',
    )
    assert.equal(item.fromWebSearch, true)
    assert.equal(item.source_id, 0)
    assert.equal(item.guid, 'web_search_8')
    assert.equal(item.source_name, '外站')
    assert.equal(item.content, null)
  })
})

describe('webSearchInList', () => {
  it('只认网络搜索条目', () => {
    const items = [
      { id: 1, fromWebSearch: false, title: '库内' },
      { id: 2, fromWebSearch: true, title: '外站' },
    ]
    assert.equal(webSearchInList(items, 1), null)
    assert.equal(webSearchInList(items, 2)?.index, 1)
    assert.equal(webSearchInList(undefined, 2), null)
  })
})
