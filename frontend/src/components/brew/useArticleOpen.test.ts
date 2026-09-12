import type { BrewItem, BrewSource } from '../../types/brew'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { it } from 'node:test'
import { act, createElement, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { brewItemState } from '../../utils/brewItemState'
import { peekFeedStoriesLoose, putFeedStories } from './pageData'
import { useArticleTaskScope } from './reader/hooks/useArticleTaskScope'
import { useContentEvents } from './reader/hooks/useContentEvents'
import { useArticleOpen } from './useArticleOpen'
import { useFeedStories } from './useBoardPage'
import { useBrewItems } from './useBrewItems'
import { useBrewSources } from './useBrewSources'
import { useBrewStarred } from './useBrewStarred'

const require = createRequire(import.meta.url)
const { JSDOM } = require(
  require.resolve('jsdom', {
    paths: [require.resolve('isomorphic-dompurify')],
  }),
)

it('Brew hooks reject stale opens, retry failed pages, and settle partial unstars', async () => {
  const dom = new JSDOM('<div id="root"></div>', { url: 'https://test.invalid' })
  const previousSessionStorage = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage')
  Object.defineProperty(globalThis, 'sessionStorage', { configurable: true, value: dom.window.sessionStorage })
  const previousStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: dom.window.localStorage })
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
  const previousDocument = Object.getOwnPropertyDescriptor(
    globalThis,
    'document',
  )
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: dom.window,
  })
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: dom.window.document,
  })
  const globals = globalThis as typeof globalThis & {
    IS_REACT_ACT_ENVIRONMENT?: boolean
  }
  globals.IS_REACT_ACT_ENVIRONMENT = true
  let session!: ReturnType<typeof useArticleOpen>
  const errors: string[] = []
  const report = (message: string) => {
    errors.push(message)
  }
  function Harness() {
    session = useArticleOpen(report, 'load failed')
    return createElement('p', null, session.selectedItem?.title ?? 'closed')
  }
  const root = createRoot(dom.window.document.getElementById('root'))
  const previousFetch = globalThis.fetch
  try {
    await act(async () => {
      root.render(createElement(Harness))
    })
    let finishOld!: (item: BrewItem) => void
    let old!: Promise<BrewItem | undefined>
    await act(async () => {
      old = session.openArticle(() => {
        const deferred = Promise.withResolvers<BrewItem>()
        finishOld = deferred.resolve
        return deferred.promise
      })
      await session.openArticle({ id: 2, title: 'B' } as BrewItem)
    })
    await act(async () => {
      finishOld({ id: 1, title: 'A' } as BrewItem)
      await old
    })
    assert.equal(dom.window.document.querySelector('p').textContent, 'B')
    await act(async () => {
      old = session.openArticle(() => {
        const deferred = Promise.withResolvers<BrewItem>()
        finishOld = deferred.resolve
        return deferred.promise
      })
      session.closeArticle()
    })
    await act(async () => {
      finishOld({ id: 3, title: 'C' } as BrewItem)
      await old
    })
    assert.equal(dom.window.document.querySelector('p').textContent, 'closed')
    assert.equal(session.opening, false)
    assert.deepEqual(errors, [])

    const requested: string[] = []
    const projections: (string | null)[] = []
    let failNextPage = true
    globalThis.fetch = async (input) => {
      const url = new URL(String(input), 'https://test.invalid')
      const cursor = url.searchParams.get('cursor') ?? ''
      requested.push(cursor)
      projections.push(url.searchParams.get('projection'))
      if (cursor && failNextPage) {
        failNextPage = false
        return Response.json({ error: 'page unavailable' }, { status: 503 })
      }
      const id = cursor ? 2 : 1
      return Response.json({
        items: [{ id, title: `page ${id}` }],
        total: 60,
        per_page: 20,
        next_cursor: `1:${id}`,
      })
    }
    let list!: ReturnType<typeof useBrewItems>
    function ListHarness() {
      list = useBrewItems('load failed', report, 'starred')
      return createElement('p', null, list.items.map(item => item.id).join(','))
    }
    await act(async () => { root.render(createElement(ListHarness)) })
    await act(async () => { list.loadMore(); list.loadMore() })
    await act(async () => { list.loadMore() })
    assert.deepEqual(requested, ['', '1:1', '1:1'])
    assert.deepEqual(projections, ['preview', 'preview', 'preview'])
    assert.deepEqual(list.items.map(item => item.id), [1, 2])
    assert.equal(list.items[0]?.content, null)

    const unstarred: number[] = []
    let failSecond = true
    globalThis.fetch = async (input) => {
      const url = String(input)
      if (url.includes('/csrf-token')) return Response.json({ csrf_token: null })
      const id = Number(url.match(/items\/(\d+)\/unstar/)?.[1])
      unstarred.push(id)
      if (id === 2 && failSecond) {
        failSecond = false
        return Response.json({ error: 'unstar unavailable' }, { status: 503 })
      }
      return Response.json({ success: true })
    }
    let starred!: ReturnType<typeof useBrewStarred>
    let remaining: BrewItem[] = []
    let count = 0
    function StarredHarness() {
      const [items, setItems] = useState([{ id: 1 }, { id: 2 }] as BrewItem[])
      const [total, setTotal] = useState(2)
      starred = useBrewStarred(items, setItems, setTotal, 'star failed', report)
      remaining = items
      count = total
      return null
    }
    await act(async () => { root.render(createElement(StarredHarness)) })
    await act(async () => { starred.enterEdit(); starred.selectAll() })
    await act(async () => { await Promise.all([starred.batchUnstar(), starred.batchUnstar()]) })
    assert.deepEqual(unstarred, [1, 2])
    assert.deepEqual(remaining.map(item => item.id), [2])
    assert.deepEqual(Iterator.from(starred.selectedIds).toArray(), [2])
    assert.equal(count, 1)
    assert.equal(starred.editMode, true)
    assert.equal(starred.processing, false)
    await act(async () => { await starred.batchUnstar() })
    assert.deepEqual(unstarred, [1, 2, 2])
    assert.deepEqual(remaining, [])
    assert.equal(count, 0)
    assert.equal(starred.editMode, false)

    let chosenIds: number[] = []
    let panelOpened = false
    function CommentEventsHarness() {
      const contentRef = useRef<HTMLDivElement>(null)
      const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
      const noop = () => {}
      useContentEvents({
        contentRef, hoverTimeoutRef, comments: [], isAuthenticated: false,
        showCommentPopup: false, showAnnotations: false,
        setFocusedCommentIds: ids => { chosenIds = ids },
        setShowCommentsPanel: show => { panelOpened = show },
        setLightboxImage: noop, setCommentTooltip: noop, setHoveredAnnotation: noop,
        setTooltipPosition: noop, setSelectedText: noop, setCommentPopupPosition: noop,
        setSelectionRange: noop, setShowCommentPopup: noop, setCommentInput: noop,
        showToastMessage: noop, t: { brew: {} },
      })
      return createElement('div', { ref: contentRef, dangerouslySetInnerHTML: { __html: '<mark class="user-comment-highlight" data-comment-id="1"><mark tabindex="0" class="user-comment-highlight" data-comment-id="2">overlap</mark></mark>' } })
    }
    await act(async () => { root.render(createElement(CommentEventsHarness)) })
    const overlap = dom.window.document.querySelector('[data-comment-id="2"]')
    await act(async () => { overlap.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Enter', bubbles: true })) })
    assert.deepEqual(chosenIds, [2, 1])
    assert.equal(panelOpened, true)
    chosenIds = []
    await act(async () => { overlap.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: ' ', bubbles: true })) })
    assert.deepEqual(chosenIds, [2, 1])
    await act(async () => { root.render(null) })

    let capture!: ReturnType<typeof useArticleTaskScope>
    let scopeItemId = 1
    function ScopeHarness() {
      capture = useArticleTaskScope(scopeItemId)
      return null
    }
    await act(async () => { root.render(createElement(ScopeHarness)) })
    const oldTask = capture()
    assert.equal(oldTask(), true)
    scopeItemId = 2
    await act(async () => { root.render(createElement(ScopeHarness)) })
    assert.equal(oldTask(), false)
    const newTask = capture()
    assert.equal(newTask(), true)
    await act(async () => { root.render(null) })
    assert.equal(newTask(), false)

    const feedSource = { id: 91, last_success_at: 0 } as BrewSource
    const story = { id: 92, is_starred: false, title: 'story', is_read: false }
    putFeedStories(91, 0, [story])
    let feed!: ReturnType<typeof useFeedStories>
    let acceptStar = false
    const changeStar = async () => {
      if (!acceptStar) return false as const
      brewItemState.commit(story.id, { is_starred: true })
    }
    function FeedHarness() {
      feed = useFeedStories('feeds', feedSource, changeStar)
      session = useArticleOpen(report, 'failed')
      return null
    }
    await act(async () => { root.render(createElement(FeedHarness)) })
    await act(async () => { await feed.onStar(story) })
    assert.equal(feed.stories[0].is_starred, false)
    assert.equal(peekFeedStoriesLoose(91)![0].is_starred, false)
    await act(async () => { await session.openArticle(story as BrewItem) })
    acceptStar = true
    await act(async () => { await feed.onStar(story) })
    assert.equal(feed.stories[0].is_starred, true)
    assert.equal(session.selectedItem?.is_starred, true)
    assert.equal(peekFeedStoriesLoose(91)![0].is_starred, false)

    let serverStarred = 5
    let statsFetches = 0
    globalThis.fetch = async (input) => {
      if (String(input).endsWith('/stats')) {
        statsFetches++
        return Response.json({ stats: { total_starred: serverStarred, total_unread: 7, total_items: 12, total_sources: 1 } })
      }
      return Response.json({ sources: [] })
    }
    let sourcesState!: ReturnType<typeof useBrewSources>
    function SourcesHarness() {
      sourcesState = useBrewSources(false, { loadFailed: 'failed', refreshFailed: 'failed' }, report)
      return null
    }
    await act(async () => { root.render(createElement(SourcesHarness)) })
    assert.equal(sourcesState.stats?.total_starred, 5)
    serverStarred = 3
    await act(async () => {
      const { invalidateSourcesCache } = await import('../../services/brewApi')
      invalidateSourcesCache()
      brewItemState.commit(1, { is_starred: false })
      brewItemState.commit(2, { is_starred: false })
      await new Promise(resolve => setTimeout(resolve, 150))
    })
    assert.equal(sourcesState.stats?.total_starred, 3)
    assert.equal(statsFetches, 2)

    await act(async () => { root.render(null) })
    brewItemState.clear()
    let serverItems = Array.from({ length: 60 }, (_, index) => ({ id: index + 1, is_starred: true }))
    const cursors: string[] = []
    let failRepair = false
    globalThis.fetch = async input => {
      const url = new URL(String(input), 'https://test.invalid')
      const cursor = url.searchParams.get('cursor') ?? ''
      cursors.push(cursor)
      if (failRepair) {
        failRepair = false
        return Response.json({ error: 'repair unavailable' }, { status: 503 })
      }
      const afterId = cursor ? Number(cursor.split(':')[1]) : 0
      const start = serverItems.findIndex(item => item.id > afterId)
      const index = start < 0 ? serverItems.length : start
      const page = serverItems.slice(index, index + 20)
      const last = page.at(-1)
      const more = index + 20 < serverItems.length
      return Response.json({
        items: page,
        total: serverItems.length,
        per_page: 20,
        next_cursor: more && last ? `1:${last.id}` : null,
      })
    }
    await act(async () => { root.render(createElement(ListHarness)) })
    await act(async () => { list.loadMore() })
    assert.equal(list.items.length, 40)
    serverItems = serverItems.filter(item => item.id !== 1)
    await act(async () => {
      brewItemState.commit(1, { is_starred: false })
      await new Promise(resolve => setTimeout(resolve, 150))
    })
    assert.equal(list.items.length, 20)
    assert.equal(list.items.at(-1)?.id, 21)
    assert.equal(list.total, 59)
    await act(async () => { list.loadMore() })
    assert.deepEqual(list.items.map(item => item.id), Array.from({ length: 40 }, (_, index) => index + 2))
    assert.deepEqual(cursors, ['', '1:20', '', '1:21'])
    failRepair = true
    serverItems = serverItems.filter(item => item.id !== 2)
    await act(async () => {
      brewItemState.commit(2, { is_starred: false })
      await new Promise(resolve => setTimeout(resolve, 150))
    })
    assert.equal(list.total, 59)
    await act(async () => { list.loadMore() })
    assert.equal(list.total, 58)
    assert.deepEqual(list.items.map(item => item.id), Array.from({ length: 20 }, (_, index) => index + 3))
    assert.deepEqual(cursors, ['', '1:20', '', '1:21', '', ''])
  } finally {
    globalThis.fetch = previousFetch
    if (previousSessionStorage) Object.defineProperty(globalThis, 'sessionStorage', previousSessionStorage)
    else Reflect.deleteProperty(globalThis, 'sessionStorage')
    if (previousStorage) Object.defineProperty(globalThis, 'localStorage', previousStorage)
    else Reflect.deleteProperty(globalThis, 'localStorage')
    await act(async () => root.unmount())
    dom.window.close()
    if (previousWindow)
      Object.defineProperty(globalThis, 'window', previousWindow)
    else Reflect.deleteProperty(globalThis, 'window')
    if (previousDocument)
      Object.defineProperty(globalThis, 'document', previousDocument)
    else Reflect.deleteProperty(globalThis, 'document')
    delete globals.IS_REACT_ACT_ENVIRONMENT
  }
})
