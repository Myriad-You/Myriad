import type { OpenUrlDeclaration } from './openUrlAllowlist.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isAllowedOpenUrlTarget,
  listOpenUrlDeclarations,

  OpenUrlRateLimiter,
  resolveOpenUrl,
} from './openUrlAllowlist.ts'

const docs: OpenUrlDeclaration = {
  id: 'docs',
  url: 'https://docs.example.com/guide/',
  match: 'prefix',
}

const status: OpenUrlDeclaration = {
  id: 'status',
  url: 'https://status.example.com/health',
  match: 'exact',
}

const site: OpenUrlDeclaration = {
  id: 'site',
  url: 'https://example.com/',
  match: 'origin',
}

describe('openUrlAllowlist', () => {
  it('rejects non-https public targets', () => {
    assert.equal(isAllowedOpenUrlTarget('http://example.com/x'), false)
    assert.equal(isAllowedOpenUrlTarget('https://example.com/x'), true)
    assert.equal(isAllowedOpenUrlTarget('http://localhost:5173/x'), true)
    assert.equal(isAllowedOpenUrlTarget('javascript:alert(1)'), false)
    assert.equal(isAllowedOpenUrlTarget('https://user:pass@example.com/'), false)
  })

  it('opens exact declarations only as declared', () => {
    const ok = resolveOpenUrl([status], { id: 'status' })
    assert.equal(ok.ok, true)
    if (ok.ok) assert.equal(ok.url, 'https://status.example.com/health')

    const withPath = resolveOpenUrl([status], { id: 'status', path: 'extra' })
    assert.equal(withPath.ok, false)

    const withQuery = resolveOpenUrl([status], {
      id: 'status',
      query: { a: '1' },
    })
    assert.equal(withQuery.ok, false)
  })

  it('allows prefix paths under the declared base only', () => {
    const ok = resolveOpenUrl([docs], { id: 'docs', path: 'install' })
    assert.equal(ok.ok, true)
    if (ok.ok) {
      assert.equal(ok.url, 'https://docs.example.com/guide/install')
    }

    const escape = resolveOpenUrl([docs], { id: 'docs', path: '../evil' })
    assert.equal(escape.ok, false)

    const absolute = resolveOpenUrl([docs], {
      id: 'docs',
      path: 'https://evil.example/',
    })
    assert.equal(absolute.ok, false)

    const protocolRelative = resolveOpenUrl([docs], {
      id: 'docs',
      path: '//evil.example/',
    })
    assert.equal(protocolRelative.ok, false)
  })

  it('allows origin match with query', () => {
    const ok = resolveOpenUrl([site], {
      id: 'site',
      path: '/blog/post',
      query: { utm: 'tapp' },
    })
    assert.equal(ok.ok, true)
    if (ok.ok) {
      assert.equal(ok.url, 'https://example.com/blog/post?utm=tapp')
    }

    const otherOrigin = resolveOpenUrl([site], {
      id: 'site',
      path: 'https://other.example/',
    })
    assert.equal(otherOrigin.ok, false)
  })

  it('rejects unknown ids', () => {
    const res = resolveOpenUrl([docs], { id: 'missing' })
    assert.equal(res.ok, false)
  })

  it('lists declarations for the sandbox', () => {
    assert.deepEqual(listOpenUrlDeclarations([docs, status]), [
      { id: 'docs', url: 'https://docs.example.com/guide/', match: 'prefix' },
      {
        id: 'status',
        url: 'https://status.example.com/health',
        match: 'exact',
      },
    ])
  })

  it('rate-limits bursts', () => {
    const limiter = new OpenUrlRateLimiter(3, 10_000)
    assert.equal(limiter.allow('a', 1000), true)
    assert.equal(limiter.allow('a', 1001), true)
    assert.equal(limiter.allow('a', 1002), true)
    assert.equal(limiter.allow('a', 1003), false)
    assert.equal(limiter.allow('b', 1003), true)
  })
})
