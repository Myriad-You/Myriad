import type { WorkbenchPane } from './logic/board'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { it } from 'node:test'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { requestCache } from '../../utils/requestCache'
import { usePhantasiWorkbench } from './usePhantasiWorkbench'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', {
  paths: [require.resolve('isomorphic-dompurify')],
}))
const labels = {
  loadFailed: 'load', noteDeleteFailed: 'delete', unscheduleFailed: 'unschedule',
  mediaLoadFailed: 'media load', mediaUploadFailed: 'upload', mediaDeleteFailed: 'media delete',
  commentDeleteFailed: 'comment delete', reviewApproveFailed: 'approve',
  reviewRejectFailed: 'reject', reviewDeleteFailed: 'review delete',
}
const notesPath = '/api/phantasi/notes/docs'
const commentsPath = '/api/phantasi/comments'
const reviewsPath = '/api/phantasi/applications'
const mediaPath = '/api/media'
const doc = {
  id: 1, item_id: null, title: 'Draft', content_md: '', topic: 'science', image: null,
  status: 'draft', scheduled_at: null, published_at: null, revision: 1, updated_at: 1,
}
const replies: Record<string, unknown> = {
  [notesPath]: { success: true, docs: [doc] },
  [commentsPath]: { success: true, comments: [{
    id: 2, item_id: 3, user_id: 1, selected_text: '', comment: 'Hello',
    is_public: true, created_at: 1, updated_at: 1,
  }] },
  [reviewsPath]: { success: true, applications: [{
    id: 4, kind: 'feed', status: 'pending', site_name: 'Site', site_url: 'https://test.invalid',
    created_at: 1, updated_at: 1, has_feed: true,
  }] },
  [mediaPath]: { total: 1, next_cursor: null, items: [{
    id: 5, kind: 'upload', url: '/asset.png', mime: 'image/png', name: 'asset.png',
    size: 12, created_at: 1, references: [],
  }] },
}

async function withWorkbench(run: (harness: {
  render: (pane: WorkbenchPane, epoch?: number) => Promise<void>
  current: () => ReturnType<typeof usePhantasiWorkbench>
  requested: string[]
  errors: string[]
}) => Promise<void>) {
  const dom = new JSDOM('<div id="root"></div>', { url: 'https://test.invalid' })
  const values = {
    window: dom.window, document: dom.window.document,
    localStorage: dom.window.localStorage, sessionStorage: dom.window.sessionStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
  }
  const previous = new Map(Object.keys(values).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  const fetch = globalThis.fetch
  for (const [key, value] of Object.entries(values)) {
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  const root = createRoot(dom.window.document.getElementById('root'))
  const requested: string[] = []
  const errors: string[] = []
  const report = (message: string) => { errors.push(message) }
  let current!: ReturnType<typeof usePhantasiWorkbench>
  function Harness({ pane, epoch }: { pane: WorkbenchPane; epoch: number }) {
    current = usePhantasiWorkbench(pane, epoch, labels, report)
    return null
  }
  globalThis.fetch = async (input) => {
    const path = new URL(String(input), 'https://test.invalid').pathname
    requested.push(path)
    assert.ok(path in replies, `Unexpected request: ${path}`)
    return Response.json(replies[path])
  }
  requestCache.deleteByPrefix('phantasi:')
  try {
    await run({
      render: async (pane, epoch = 0) => {
        await act(async () => root.render(createElement(Harness, { pane, epoch })))
      },
      current: () => current,
      requested,
      errors,
    })
  } finally {
    await act(async () => root.unmount())
    requestCache.deleteByPrefix('phantasi:')
    globalThis.fetch = fetch
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
    dom.window.close()
  }
}

const demands: Array<[WorkbenchPane, string[], boolean]> = [
  ['home', [notesPath, mediaPath, commentsPath, reviewsPath], false],
  ['notes', [notesPath], true],
  ['notesIo', [notesPath], false],
  ['noteCategories', [notesPath], true],
  ['sourceCategories', [notesPath], true],
  ['sources', [notesPath], true],
  ['add', [notesPath], true],
  ['media', [mediaPath], false],
  ['comments', [commentsPath], false],
  ['reviews', [reviewsPath], false],
  ['topics', [], false],
  ['rsshub', [], false],
  ['feedsIo', [], false],
]
for (const [pane, paths, categories] of demands) {
  it(`${pane} requests only resources used by its visible content and category actions`, async () => {
    await withWorkbench(async ({ render, current, requested, errors }) => {
      await render(pane)
      assert.deepEqual(requested.toSorted(), paths.toSorted())
      assert.equal(current().needsCategories, categories)
      if (paths.includes(notesPath)) assert.deepEqual(current().docs, [doc])
      if (paths.includes(commentsPath)) assert.equal(current().comments.length, 1)
      if (paths.includes(mediaPath)) assert.equal(current().media.length, 1)
      if (paths.includes(reviewsPath)) assert.equal(current().applications.length, 1)
      assert.deepEqual(errors, [])
    })
  })
}

it('note epoch refreshes only notes while overview still has every resource', async () => {
  await withWorkbench(async ({ render, current, requested }) => {
    await render('home')
    requestCache.deleteByPrefix('phantasi:')
    requested.length = 0
    await render('home', 1)
    assert.deepEqual(requested, [notesPath])
    assert.equal(current().comments.length, 1)
    assert.equal(current().media.length, 1)
    assert.equal(current().applications.length, 1)
  })
})

for (const pane of ['comments', 'media', 'reviews'] as const) {
  it(`leaving ${pane} aborts its request and ignores its late result`, async () => {
    await withWorkbench(async ({ render, current, requested, errors }) => {
      const response = Promise.withResolvers<Response>()
      let signal: AbortSignal | null | undefined
      const fetch = globalThis.fetch
      globalThis.fetch = (input, init) => {
        signal = init?.signal
        return response.promise
      }
      try {
        await render(pane)
        assert.ok(signal)
        await render('topics')
        assert.equal(signal.aborted, true)
        globalThis.fetch = fetch
        await render(pane)
        await act(async () => { response.resolve(Response.json({ success: true, items: [], comments: [], applications: [] })) })
        assert.equal(current().commentsLoading, false)
        assert.equal(current().mediaLoading, false)
        assert.equal(current().applicationsLoading, false)
        const rows = pane === 'comments' ? current().comments : pane === 'media' ? current().media : current().applications
        assert.equal(rows.length, 1)
        assert.equal(requested.length, 1)
        assert.deepEqual(errors, [])
      } finally {
        await act(async () => { response.resolve(Response.json({ success: true, docs: [], items: [], comments: [], applications: [] })) })
      }
    })
  })
}

it('explicit media reload works but a note epoch does not restart it', async () => {
  await withWorkbench(async ({ render, current, requested }) => {
    await render('media')
    requested.length = 0
    await render('media', 1)
    assert.deepEqual(requested, [])
    await act(async () => { await current().reloadMedia() })
    assert.deepEqual(requested, [mediaPath])
  })
})

for (const [pane, reload] of [
  ['notes', 'reloadNotes'], ['media', 'reloadMedia'],
  ['comments', 'reloadComments'], ['reviews', 'reloadApplications'],
] as const) {
  it(`a delayed ${pane} mutation callback cannot restart a deactivated resource`, async () => {
    await withWorkbench(async ({ render, current, requested }) => {
      await render(pane)
      const reloadAfterMutation = current()[reload]
      await render('topics')
      requestCache.deleteByPrefix('phantasi:')
      requested.length = 0
      await act(async () => { await reloadAfterMutation() })
      assert.deepEqual(requested, [])
    })
  })
}

it('leaving notes discards a late shared-cache result without clearing previously loaded docs', async () => {
  await withWorkbench(async ({ render, current, errors }) => {
    await render('notes')
    const response = Promise.withResolvers<Response>()
    globalThis.fetch = () => response.promise
    requestCache.deleteByPrefix('phantasi:')
    let pending!: Promise<void>
    await act(async () => { pending = current().reloadNotes() })
    await render('topics')
    await act(async () => {
      response.resolve(Response.json({ success: true, docs: [{ ...doc, title: 'Too late' }] }))
      await pending
    })
    assert.deepEqual(current().docs, [doc])
    assert.equal(current().notesLoading, false)
    assert.deepEqual(errors, [])
  })
})

it('appends cursor pages, preserves total, and cancels the old page on filter changes', async () => {
  await withWorkbench(async ({ render, current, errors }) => {
    const asset = { id: 50, kind: 'upload', url: '/50.png', mime: 'image/png', name: '50.png', size: 1, created_at: 1, references: [] }
    const cursor = { created_at: '2026-09-22T01:02:03.123456+00:00', id: 50 }
    const urls: URL[] = []
    globalThis.fetch = async (input) => {
      const url = new URL(String(input), 'https://test.invalid')
      urls.push(url)
      return Response.json(url.searchParams.has('before_id')
        ? { items: [{ ...asset, id: 49 }], next_cursor: cursor }
        : { items: [asset], total: 100, next_cursor: cursor })
    }
    await render('media')
    assert.equal(current().mediaTotal, 100)
    assert.equal(current().media.length, 1)
    await act(async () => { await current().loadMoreMedia() })
    assert.deepEqual(current().media.map(row => row.id), [50, 49])
    assert.equal(current().mediaTotal, 100)
    assert.equal(urls[1].searchParams.get('before_created_at'), cursor.created_at)
    const late = Promise.withResolvers<Response>()
    let oldSignal: AbortSignal | null | undefined
    globalThis.fetch = async (input, init) => {
      const url = new URL(String(input), 'https://test.invalid')
      if (url.searchParams.has('before_id')) {
        oldSignal = init?.signal
        return late.promise
      }
      assert.equal(url.searchParams.get('format'), 'webp')
      return Response.json({ items: [{ ...asset, id: 10 }], total: 1, next_cursor: null })
    }
    let pending!: Promise<void>
    await act(async () => { pending = current().loadMoreMedia() })
    await act(async () => { current().setMediaFilter({ kind: 'all', format: 'webp', query: '' }) })
    assert.equal(oldSignal?.aborted, true)
    await act(async () => {
      late.resolve(Response.json({ items: [{ ...asset, id: 48 }], next_cursor: cursor }))
      await pending
    })
    assert.deepEqual(current().media.map(row => row.id), [10])
    assert.equal(current().mediaTotal, 1)
    assert.equal(current().mediaHasMore, false)
    assert.deepEqual(errors, [])
  })
})
