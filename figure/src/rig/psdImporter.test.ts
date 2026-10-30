import assert from 'node:assert/strict'
import { test } from 'node:test'
import { sourceMasterFetchUrl } from './psdImporter'

test('PSD source reads use the API origin without changing the stored source identity', () => {
  const source = '/media/assets/id/portrait.png'
  const page = 'https://frontend.example/agent/settings'
  assert.equal(sourceMasterFetchUrl(source, page, ''), `https://frontend.example${source}`)
  assert.equal(sourceMasterFetchUrl(source, page, 'https://api.example'), `https://api.example${source}`)
  assert.equal(sourceMasterFetchUrl('assets/master.png', 'https://frontend.example/rigImport.html', ''), 'https://frontend.example/assets/master.png')
  assert.equal(sourceMasterFetchUrl('blob:https://frontend.example/id', page, 'https://api.example'), 'blob:https://frontend.example/id')
  assert.equal(source, '/media/assets/id/portrait.png')
})
