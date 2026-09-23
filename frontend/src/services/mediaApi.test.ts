import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { afterEach, it, mock } from 'node:test'
import { fileURLToPath } from 'node:url'
import { displayImageUrl } from '../components/phantasi/notes/noteImageUrl'
import { apiService } from './api'
import { draftMediaSrc, fetchMediaObjectUrl, isPrivateMediaPath, listMedia, saveMediaEdit, uploadMedia } from './mediaApi'

const asset = {
  id: 7, kind: 'upload' as const, url: '/media/federation/1/photo.png',
  mime: 'image/png', name: 'photo.png', size: 12, created_at: 1, references: [],
}

afterEach(() => mock.restoreAll())

it('list and saved edits retain API references and paths; display resolves per origin', async () => {
  const items = [asset]
  mock.method(apiService, 'get', async () => ({ success: true, items }))
  mock.method(apiService, 'post', async () => ({ item: asset }))
  assert.equal((await listMedia()).items, items)
  assert.equal(await saveMediaEdit(7, 'data:image/png;base64,AA==', false), asset)
  for (const origin of ['', 'https://api.example']) {
    assert.equal(displayImageUrl(asset.url, origin), `${origin}${asset.url}`)
  }
  assert.equal(asset.url, '/media/federation/1/photo.png')
})

it('invalid catalog rows fail instead of disappearing or receiving empty URLs', async () => {
  mock.method(apiService, 'get', async () => ({ items: [asset, { ...asset, url: '' }] }))
  mock.method(apiService, 'post', async () => ({ item: { ...asset, id: '7' } }))
  await assert.rejects(listMedia(), /Invalid media response/)
  await assert.rejects(saveMediaEdit(7, 'data:image/png;base64,AA==', false), /Invalid media response/)
})

it('upload retains the response asset and sends file contents in multipart', async (t) => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage')
  Object.defineProperty(globalThis, 'sessionStorage', { configurable: true, value: {
    getItem: () => null, setItem: () => {}, removeItem: () => {},
  } })
  t.after(() => {
    if (previous) Object.defineProperty(globalThis, 'sessionStorage', previous)
    else Reflect.deleteProperty(globalThis, 'sessionStorage')
  })
  const file = new File(['image bytes'], 'photo.png', { type: 'image/png' })
  mock.method(globalThis, 'fetch', async (url: string, options?: RequestInit) => {
    if (String(url).includes('csrf')) return Response.json({ csrf_token: null })
    assert.equal(url, '/api/media')
    assert.equal(options?.method, 'POST')
    const uploaded = (options?.body as FormData).get('file') as File
    assert.equal(await uploaded.text(), await file.text())
    return { ok: true, json: async () => ({ success: true, item: asset }) } as Response
  })
  assert.equal(await uploadMedia(file), asset)
})

it('drafts prefer the authenticated content path until publication', () => {
  assert.equal(
    draftMediaSrc({
      ...asset,
      exposure: 'private',
      content_path: '/api/media/7/content',
      public_path: null,
    }),
    '/api/media/7/content',
  )
  assert.equal(
    draftMediaSrc({
      ...asset,
      exposure: 'public',
      content_path: '/api/media/7/content',
      public_path: '/media/assets/11111111-1111-1111-1111-111111111111/photo.png',
    }),
    '/media/assets/11111111-1111-1111-1111-111111111111/photo.png',
  )
  assert.equal(isPrivateMediaPath('/api/media/7/content'), true)
  assert.equal(isPrivateMediaPath('/media/assets/11111111-1111-1111-1111-111111111111/photo.png'), false)
})

it('journal editor uploads through mediaApi, not federationApi', () => {
  const src = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), '../components/phantasi/notes/useNoteEditorFormat.ts'),
    'utf8',
  )
  assert.match(src, /uploadMedia\(file\)/)
  assert.doesNotMatch(src, /federationApi/)
})

it('private previews retain authentication with query strings, fragments, and absolute URLs', () => {
  for (const src of ['/api/media/7/content?v=2', '/api/media/7/content#preview', 'https://api.example/api/media/7/content?v=2', '//api.example/api/media/7/content']) {
    assert.equal(isPrivateMediaPath(src), true)
  }
  for (const src of ['/api/media/7/content/other', '/media/assets/photo.png', 'data:image/png;base64,AA==']) {
    assert.equal(isPrivateMediaPath(src), false)
  }
})

it('a body that completes after cancellation cannot allocate an orphaned preview URL', async () => {
  const controller = new AbortController()
  let finish!: (value: Blob) => void
  const body = new Promise<Blob>((resolve) => { finish = resolve })
  mock.method(globalThis, 'fetch', async () => ({ ok: true, blob: () => body }))
  const create = mock.method(URL, 'createObjectURL', () => 'blob:unexpected')
  const result = fetchMediaObjectUrl('/api/media/7/content', controller.signal)
  controller.abort()
  finish(new Blob(['image']))
  await assert.rejects(result, { name: 'AbortError' })
  assert.equal(create.mock.callCount(), 0)
})

it('serializes filters and the full timestamp cursor without changing its precision', async () => {
  const cursor = { created_at: '2026-09-22T01:02:03.123456+00:00', id: 42 }
  const page = { items: [asset], next_cursor: null }
  const get = mock.method(apiService, 'get', async () => page)
  assert.equal(await listMedia({ filter: { kind: 'generated', format: 'png', query: '  100% sky  ' }, cursor, limit: 10 }), page)
  const url = new URL(get.mock.calls[0].arguments[0], 'https://test.invalid')
  assert.equal(url.pathname, '/media')
  assert.deepEqual(Object.fromEntries(url.searchParams), {
    kind: 'generated', format: 'png', query: '100% sky',
    before_created_at: cursor.created_at, before_id: '42', limit: '10',
  })
})

it('private preview fetch stays on the current site when a stored URL has an old origin', async () => {
  const fetch = mock.method(globalThis, 'fetch', async () => new Response(new Blob(['image'])))
  const url = await fetchMediaObjectUrl('https://old.example/api/media/7/content?v=2')
  assert.equal(fetch.mock.calls[0].arguments[0], '/api/media/7/content?v=2')
  assert.equal(fetch.mock.calls[0].arguments[1]?.credentials, 'include')
  URL.revokeObjectURL(url)
})
