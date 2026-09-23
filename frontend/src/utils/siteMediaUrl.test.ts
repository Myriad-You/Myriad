import assert from 'node:assert/strict'
import { test } from 'node:test'
import { siteMediaUrl } from './siteMediaUrl'

test('backend media reads resolve at the API origin; stored identity and external URLs stay intact', () => {
  for (const path of ['/media/assets/id/portrait.png', '/api/media/7/content', '/media/federation/1/a.jpg', '/api/merope/rig/assets/hash']) {
    assert.equal(siteMediaUrl(path, ''), path)
    assert.equal(siteMediaUrl(path, 'https://api.example/'), `https://api.example${path}`)
  }
  for (const url of ['https://other.example/image.png', 'blob:https://ui.example/id', 'data:image/png;base64,AA==', '/assets/style.png', '//cdn.example/img.png']) {
    assert.equal(siteMediaUrl(url, 'https://api.example'), url)
  }
})

test('persisted media follows the current API origin after a site-domain change', () => {
  for (const path of ['/media/assets/id/portrait.jpg', '/media/federation/1/photo.jpg', '/api/media/7/content']) {
    assert.equal(siteMediaUrl(`https://old.example${path}?v=2`, 'https://api.example'), `https://api.example${path}?v=2`)
    assert.equal(siteMediaUrl(`//old.example${path}`, ''), path)
  }
  assert.equal(siteMediaUrl('https://other.example/api/private', ''), 'https://other.example/api/private')
})
