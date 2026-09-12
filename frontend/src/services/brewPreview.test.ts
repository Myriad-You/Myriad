import assert from 'node:assert/strict'
import { it } from 'node:test'
import { loadFeedStories, loadHomeBoardNotes } from '../components/brew/pageData'
import { getItem, getItemPreviews, getItems } from './brewApi'

it('requests explicit previews while preserving full list and detail contracts', async () => {
  const originalFetch = globalThis.fetch
  const urls: URL[] = []
  globalThis.fetch = async input => {
    const url = new URL(String(input), 'https://test.invalid')
    urls.push(url)
    if (url.pathname.endsWith('/items/91234')) return Response.json({ item: { id: 91234, content: '<p>detail</p>' } })
    const item = url.searchParams.get('projection') === 'preview'
      ? { id: 91234, title: 'preview' }
      : { id: 91234, title: 'full', content: '<p>list body</p>' }
    return Response.json({ items: [item], total: 1, page: 1, per_page: 20 })
  }
  try {
    const previews = await getItemPreviews({ source_id: 8, page: 2 })
    assert.equal(urls[0].searchParams.get('projection'), 'preview')
    assert.equal(urls[0].searchParams.get('source_id'), '8')
    assert.equal(urls[0].searchParams.get('page'), '2')
    assert.equal(Object.hasOwn(previews.items[0], 'content'), false)
    const full = await getItems()
    assert.equal(urls[1].searchParams.has('projection'), false)
    assert.equal(full.items[0].content, '<p>list body</p>')
    assert.equal((await getItem(91234)).content, '<p>detail</p>')
    await loadFeedStories(91235, 1)
    assert.equal(urls.at(-1)?.searchParams.get('projection'), 'preview')
    await loadHomeBoardNotes([{ id: 91236, source_type: 'note' }])
    assert.equal(urls.at(-1)?.searchParams.get('projection'), 'preview')
  } finally {
    globalThis.fetch = originalFetch
  }
})
