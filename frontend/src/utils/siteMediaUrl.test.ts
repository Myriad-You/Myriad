import assert from 'node:assert/strict'
import { test } from 'node:test'
import { siteMediaUrl } from './siteMediaUrl'

test('backend media reads resolve at the API origin; stored identity and external URLs stay intact', () => {
  for (const path of ['/api/media/7/content', '/api/merope/rig/assets/hash']) {
    assert.equal(siteMediaUrl(path, ''), path)
    assert.equal(siteMediaUrl(path, 'https://api.example/'), `https://api.example${path}`)
  }
  for (const url of ['https://other.example/image.png', 'blob:https://ui.example/id', 'data:image/png;base64,AA==', '/assets/style.png', '//cdn.example/img.png']) {
    assert.equal(siteMediaUrl(url, 'https://api.example'), url)
  }
})

test('site media displays through the /api alias that every proxy version forwards', () => {
  for (const path of ['/media/assets/id/portrait.png', '/media/federation/1/a.jpg']) {
    assert.equal(siteMediaUrl(path, ''), `/api${path}`)
    assert.equal(siteMediaUrl(path, 'https://api.example/'), `https://api.example/api${path}`)
  }
})

test('persisted media follows the current API origin after a site-domain change', () => {
  for (const [path, shown] of [
    ['/media/assets/id/portrait.jpg', '/api/media/assets/id/portrait.jpg'],
    ['/media/federation/1/photo.jpg', '/api/media/federation/1/photo.jpg'],
    ['/api/media/7/content', '/api/media/7/content'],
    // A display alias copied with an old origin is not prefixed twice.
    ['/api/media/assets/id/portrait.jpg', '/api/media/assets/id/portrait.jpg'],
    ['/api/media/federation/1/photo.jpg', '/api/media/federation/1/photo.jpg'],
  ]) {
    assert.equal(siteMediaUrl(`https://old.example${path}?v=2`, 'https://api.example'), `https://api.example${shown}?v=2`)
    assert.equal(siteMediaUrl(`//old.example${path}`, ''), shown)
  }
  assert.equal(siteMediaUrl('https://other.example/api/private', ''), 'https://other.example/api/private')
})
