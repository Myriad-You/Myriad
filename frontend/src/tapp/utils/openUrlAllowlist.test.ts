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

  it('resolves same-origin declarations against the host origin', () => {
    const self: OpenUrlDeclaration = {
      id: 'self',
      url: '/',
      match: 'same-origin',
    }

    const ok = resolveOpenUrl(
      [self],
      { id: 'self', path: '/journal/notes/1' },
      'https://sua17.cn',
    )
    assert.equal(ok.ok, true)
    if (ok.ok) {
      assert.equal(ok.url, 'https://sua17.cn/journal/notes/1')
      assert.equal(ok.match, 'same-origin')
    }

    const withQuery = resolveOpenUrl(
      [self],
      { id: 'self', path: '/journal', query: { from: 'tapp' } },
      'https://sua17.cn',
    )
    assert.equal(withQuery.ok, true)
    if (withQuery.ok) {
      assert.equal(withQuery.url, 'https://sua17.cn/journal?from=tapp')
    }

    // The same declaration follows whatever host it runs on, so it works on any
    // self-hosted domain instead of the package author's.
    const other = resolveOpenUrl(
      [self],
      { id: 'self', path: '/journal' },
      'https://other.example',
    )
    assert.equal(other.ok, true)
    if (other.ok) assert.equal(other.url, 'https://other.example/journal')

    // No host origin available → reject.
    assert.equal(
      resolveOpenUrl([self], { id: 'self', path: '/journal' }).ok,
      false,
    )

    // Cannot escape the host origin via an absolute or protocol-relative path.
    assert.equal(
      resolveOpenUrl(
        [self],
        { id: 'self', path: 'https://evil.example/' },
        'https://sua17.cn',
      ).ok,
      false,
    )
    assert.equal(
      resolveOpenUrl(
        [self],
        { id: 'self', path: '//evil.example/' },
        'https://sua17.cn',
      ).ok,
      false,
    )

    // A same-origin declaration must be a rooted relative path, not an absolute URL.
    const absoluteDecl = resolveOpenUrl(
      [{ id: 'bad', url: 'https://example.com/', match: 'same-origin' }],
      { id: 'bad' },
      'https://sua17.cn',
    )
    assert.equal(absoluteDecl.ok, false)
  })

  it('rejects same-origin traversal before URL normalization', () => {
    const self: OpenUrlDeclaration = { id: 'self', url: '/', match: 'same-origin' }
    for (const path of [
      '/journal/../config',
      '/journal/%2e%2e/config',
      '/journal/.%2e/config',
      '/journal/%2E%2E/config',
      '/journal/%2e%2e%2fconfig',
      '/journal%2f..%2fconfig',
      '../config',
      '/journal/.. ',
      '/journal/%5cconfig',
      '/journal/%00',
      '/journal/%0a',
      '/journal/%7f',
      '/journal/%',
      '/journal/%zz',
      '/journal/%ff',
    ]) {
      assert.equal(
        resolveOpenUrl([self], { id: 'self', path }, 'https://host.example').ok,
        false,
        path,
      )
    }
  })

  it('rejects invalid same-origin declarations even with a safe caller path', () => {
    for (const url of [
      '/journal/../',
      '/journal/%2e%2e/',
      '/journal/.%2e/',
      '/journal/%2e%2e%2fconfig',
      '/journal/%5cconfig',
      '/journal/%00',
      '/journal/%zz',
      '/journal/%ff',
      '/journal#fragment',
      '/journal notes',
      '/journal/\nnotes',
      `/${'a'.repeat(2048)}`,
    ]) {
      const entry: OpenUrlDeclaration = { id: 'self', url, match: 'same-origin' }
      for (const request of [{ id: 'self' }, { id: 'self', path: '/' }]) {
        assert.equal(resolveOpenUrl([entry], request, 'https://host.example').ok, false, url)
      }
    }
  })

  it('preserves same-origin prefix boundaries, safe encodings, and queries', () => {
    const self: OpenUrlDeclaration = { id: 'self', url: '/journal', match: 'same-origin' }
    const paths = [
      '/journal',
      '/journal/notes/1',
      '/journal/%E7%AC%94%E8%AE%B0',
      '/journal/100%25',
      '/journal/v1..2',
      '/journal/a%2Fb',
      '/journal/notes?next=/a/../b&invalid=%zz',
    ]
    for (const path of paths) {
      const result = resolveOpenUrl([self], { id: 'self', path }, 'https://host.example')
      assert.equal(result.ok, true, path)
      if (result.ok) assert.equal(result.url, `https://host.example${path}`)
    }
    for (const path of ['/config', '/journal-other', '/journalistic/notes']) {
      assert.equal(resolveOpenUrl([self], { id: 'self', path }, 'https://host.example').ok, false, path)
    }
    const result = resolveOpenUrl(
      [{ ...self, url: '/journal?from=manifest' }],
      { id: 'self', query: { next: '/a/../b' } },
      'http://localhost:5173',
    )
    assert.equal(result.ok, true)
    if (result.ok) {
      const url = new URL(result.url)
      assert.equal(url.origin, 'http://localhost:5173')
      assert.equal(url.pathname, '/journal')
      assert.equal(url.searchParams.get('from'), 'manifest')
      assert.equal(url.searchParams.get('next'), '/a/../b')
    }
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
