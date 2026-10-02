import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { afterEach, it, mock } from 'node:test'
import { fileURLToPath } from 'node:url'
import { displayImageUrl } from '../components/phantasi/notes/noteImageUrl'
import { apiService } from './api'
import { listMedia, mediaAssetSrc, saveMediaEdit, uploadMedia } from './mediaApi'

const asset = {
  id: 7, kind: 'upload' as const, url: '/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/photo.png',
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
  assert.equal(asset.url, '/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/photo.png')
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
    return Response.json({ success: true, item: asset })
  })
  assert.deepEqual(await uploadMedia(file), asset)
})

it('an asset is shown at its one permanent address whatever its exposure', () => {
  const url = '/media/assets/11111111-1111-1111-1111-111111111111/photo.png'
  for (const exposure of ['private', 'public']) {
    assert.equal(mediaAssetSrc({ ...asset, url, exposure, content_path: '/api/media/7/content' }), url)
  }
})

it('journal editor uploads through mediaApi, not federationApi', () => {
  const src = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), '../components/phantasi/notes/useNoteEditorFormat.ts'),
    'utf8',
  )
  assert.match(src, /uploadMedia\(file\)/)
  assert.doesNotMatch(src, /federationApi/)
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
