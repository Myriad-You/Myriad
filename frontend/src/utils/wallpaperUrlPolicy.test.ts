import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isBlockedWallpaperHost,
  sanitizeWallpaperUrl,
} from './wallpaperUrlPolicy'

describe('isBlockedWallpaperHost', () => {
  it('blocks loopback and localhost variants', () => {
    assert.equal(isBlockedWallpaperHost('localhost'), true)
    assert.equal(isBlockedWallpaperHost('127.0.0.1'), true)
    assert.equal(isBlockedWallpaperHost('0.0.0.0'), true)
    assert.equal(isBlockedWallpaperHost('::1'), true)
    assert.equal(isBlockedWallpaperHost('foo.localhost'), true)
  })

  it('blocks private and link-local IPv4', () => {
    assert.equal(isBlockedWallpaperHost('10.0.0.1'), true)
    assert.equal(isBlockedWallpaperHost('172.16.5.1'), true)
    assert.equal(isBlockedWallpaperHost('192.168.1.1'), true)
    assert.equal(isBlockedWallpaperHost('169.254.169.254'), true)
    assert.equal(isBlockedWallpaperHost('100.64.0.1'), true)
  })

  it('blocks special TLDs', () => {
    assert.equal(isBlockedWallpaperHost('nas.local'), true)
    assert.equal(isBlockedWallpaperHost('svc.internal'), true)
  })

  it('allows public hosts', () => {
    assert.equal(isBlockedWallpaperHost('images.unsplash.com'), false)
    assert.equal(isBlockedWallpaperHost('cdn.example.com'), false)
    assert.equal(isBlockedWallpaperHost('8.8.8.8'), false)
  })
})

describe('sanitizeWallpaperUrl', () => {
  it('allows https and http public URLs', () => {
    assert.equal(
      sanitizeWallpaperUrl('https://images.unsplash.com/photo-1'),
      'https://images.unsplash.com/photo-1',
    )
    assert.equal(
      sanitizeWallpaperUrl('http://cdn.example.com/a.jpg'),
      'http://cdn.example.com/a.jpg',
    )
  })

  it('allows same-origin paths', () => {
    assert.equal(sanitizeWallpaperUrl('/uploads/wall.jpg'), '/uploads/wall.jpg')
    assert.equal(
      sanitizeWallpaperUrl('/api/proxy/image?url=https%3A%2F%2Fx.com%2Fa.jpg'),
      '/api/proxy/image?url=https%3A%2F%2Fx.com%2Fa.jpg',
    )
  })

  it('upgrades protocol-relative to https', () => {
    assert.equal(
      sanitizeWallpaperUrl('//cdn.example.com/a.jpg'),
      'https://cdn.example.com/a.jpg',
    )
  })

  it('rejects dangerous schemes and data/svg', () => {
    assert.equal(sanitizeWallpaperUrl('javascript:alert(1)'), null)
    assert.equal(sanitizeWallpaperUrl('data:image/png;base64,aaa'), null)
    assert.equal(
      sanitizeWallpaperUrl('data:image/svg+xml,<svg onload=alert(1)>'),
      null,
    )
    assert.equal(sanitizeWallpaperUrl('blob:https://x/1'), null)
    assert.equal(sanitizeWallpaperUrl('file:///etc/passwd'), null)
  })

  it('rejects private hosts and credentials', () => {
    assert.equal(sanitizeWallpaperUrl('http://127.0.0.1/a.jpg'), null)
    assert.equal(sanitizeWallpaperUrl('http://192.168.0.1/a.jpg'), null)
    assert.equal(sanitizeWallpaperUrl('http://localhost:8080/a.jpg'), null)
    assert.equal(
      sanitizeWallpaperUrl('https://user:pass@cdn.example.com/a.jpg'),
      null,
    )
  })
})
