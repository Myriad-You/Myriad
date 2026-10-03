import assert from 'node:assert/strict'
import { test } from 'node:test'
import { displayMediaUrl } from './displayMediaUrl'

const api = 'https://api.example'

test('platform media follows the API origin, also under an older site origin', () => {
  for (const path of [
    '/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.png',
    '/api/media/7/content',
    `/api/phantasi/image-cache/ab/ab${'0'.repeat(62)}.png`,
  ]) {
    assert.equal(displayMediaUrl(path, api), `${api}${path}`)
    assert.equal(displayMediaUrl(`https://old.example${path}`, api), `${api}${path}`)
  }
})

test("another site's /api/ image stays on that site", () => {
  // The reader once rewrote this to the local origin and showed a 404.
  assert.equal(displayMediaUrl('https://other.example/api/x.jpg', api), 'https://other.example/api/x.jpg')
})

test('inline data and blob URLs are left alone', () => {
  assert.equal(displayMediaUrl('data:image/png;base64,AA==', api), 'data:image/png;base64,AA==')
  assert.equal(displayMediaUrl('blob:https://ui.example/id', api), 'blob:https://ui.example/id')
})
