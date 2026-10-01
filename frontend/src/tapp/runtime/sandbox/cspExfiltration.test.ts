/** connect-src 仅 blob:/data:。远端 img/media 只放 media:remote 授予的域名，从不放整个 https:。 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import vm from 'node:vm'

import { REMOTE_MEDIA_MATCH_SOURCE, sanitizeRemoteMediaHosts } from './remoteMedia.ts'
import {
  cspOptionsFromPermissions,
  generateCSP,
  generateSecurityWrapper,
} from './security.ts'

function directive(csp: string, name: string): string {
  const found = csp
    .split(';')
    .map((d) => d.trim())
    .find((d) => d === name || d.startsWith(`${name} `))
  return found ?? ''
}

describe('generateCSP media directives', () => {
  it('does not allow arbitrary remote images by default', () => {
    const csp = generateCSP('n0nce', cspOptionsFromPermissions([]))

    const img = directive(csp, 'img-src')
    assert.ok(img.length > 0, 'img-src must be present')
    assert.ok(
      !/\bhttps:/.test(img),
      `img-src must not allow bare https: by default; got "${img}"`,
    )
    assert.ok(
      !/\bhttp:/.test(img),
      `img-src must not allow bare http: by default; got "${img}"`,
    )
    assert.ok(img.includes('data:'), 'packaged/data URIs still allowed')
    assert.ok(img.includes('blob:'), 'blob URIs still allowed')

    const media = directive(csp, 'media-src')
    assert.ok(
      !/\bhttps:/.test(media),
      `media-src must not allow bare https: by default; got "${media}"`,
    )
  })

  it('network:fetch no longer opens remote images or media', () => {
    const csp = generateCSP(
      'n0nce',
      cspOptionsFromPermissions(['network:fetch'], ['cdn.example.com']),
    )
    for (const name of ['img-src', 'media-src']) {
      const value = directive(csp, name)
      assert.ok(!/https?:/.test(value), `${name} must stay closed: "${value}"`)
    }
  })

  it('media:remote adds exactly the granted hosts, never bare https:', () => {
    const csp = generateCSP(
      'n0nce',
      cspOptionsFromPermissions(
        ['media:remote'],
        ['act-webstatic.mihoyo.com', '*.miyoushe.com'],
      ),
    )
    for (const name of ['img-src', 'media-src']) {
      const value = directive(csp, name)
      assert.ok(value.includes(' https://act-webstatic.mihoyo.com'), value)
      assert.ok(value.includes(' https://*.miyoushe.com'), value)
      assert.ok(!/(^|\s)https?:(\s|$)/.test(value), `no bare scheme: "${value}"`)
      assert.ok(!value.includes('http://'), `no http hosts: "${value}"`)
    }
    // Granted hosts without the permission do nothing.
    const withoutPermission = generateCSP(
      'n0nce',
      cspOptionsFromPermissions([], ['cdn.example.com']),
    )
    assert.ok(!directive(withoutPermission, 'img-src').includes('cdn.example.com'))
  })

  it('drops host entries that could rewrite the policy', () => {
    const hostile = [
      "cdn.example.com; script-src 'unsafe-inline'",
      'cdn.example.com https:',
      '*',
      'https://cdn.example.com',
      'CDN.EXAMPLE.COM',
      "evil.example.com'",
      '*.com',
      'ok.example.com',
    ]
    assert.deepEqual(sanitizeRemoteMediaHosts(hostile), ['ok.example.com'])
    const csp = generateCSP('n0nce', cspOptionsFromPermissions(['media:remote'], hostile))
    assert.equal(
      directive(csp, 'script-src'),
      "script-src 'nonce-n0nce' 'wasm-unsafe-eval'",
    )
    assert.ok(!/(^|\s)https:(\s|$)/.test(directive(csp, 'img-src')))
  })

  it('keeps media:audio and network:fetch independent', () => {
    const audioOnly = generateCSP(
      'n',
      cspOptionsFromPermissions(['media:audio']),
    )
    const media = directive(audioOnly, 'media-src')
    assert.ok(media.includes('blob:'), 'media:audio grants blob:')
    assert.ok(
      !/\bhttps:/.test(media),
      `media:audio must not imply remote media; got "${media}"`,
    )
    const img = directive(audioOnly, 'img-src')
    assert.ok(
      !/\bhttps:/.test(img),
      `media:audio must not open remote images; got "${img}"`,
    )
  })

  it('never relaxes the directives that make the sandbox a sandbox', () => {
    for (const perms of [
      [],
      ['network:fetch'],
      ['media:audio', 'network:fetch'],
      ['media:remote', 'media:audio'],
    ]) {
      const csp = generateCSP(
        'n0nce',
        cspOptionsFromPermissions(perms, ['cdn.example.com']),
      )
      assert.equal(directive(csp, 'connect-src'), 'connect-src blob: data:')
      assert.ok(
        !/\bhttps:/.test(directive(csp, 'connect-src')),
        'connect-src must not allow https',
      )
      assert.equal(directive(csp, 'worker-src'), "worker-src 'none'")
      assert.equal(directive(csp, 'frame-src'), "frame-src 'none'")
      assert.equal(directive(csp, 'object-src'), "object-src 'none'")
      assert.equal(directive(csp, 'form-action'), "form-action 'none'")
      assert.equal(directive(csp, 'base-uri'), "base-uri 'none'")
      const script = directive(csp, 'script-src')
      assert.ok(script.includes("'nonce-n0nce'"))
      assert.ok(
        !script.includes('https:'),
        `script-src must stay nonce-only: ${script}`,
      )
    }
  })

  it('keeps fetch blocked for network URLs and only mentions local schemes', () => {
    const wrapper = generateSecurityWrapper('tok')
    assert.match(wrapper, /isSandboxedFetchUrl/)
    assert.match(wrapper, /blob:/)
    assert.match(wrapper, /fetch disabled/)
    assert.doesNotMatch(wrapper, /connect-src 'none'/)
  })
})

describe('sandbox remote image matcher', () => {
  const context = vm.createContext({ URL })
  vm.runInContext(REMOTE_MEDIA_MATCH_SOURCE, context)
  const allowed = (url: string, hosts: string[]) =>
    vm.runInContext('isRemoteMediaUrlAllowed', context)(url, hosts) as boolean

  it('follows CSP host-source semantics', () => {
    const hosts = ['act-webstatic.mihoyo.com', '*.miyoushe.com']
    assert.equal(allowed('https://act-webstatic.mihoyo.com/a.png', hosts), true)
    assert.equal(allowed('//act-webstatic.mihoyo.com/a.png', hosts), true)
    assert.equal(allowed('https://bbs-static.miyoushe.com/a.png', hosts), true)
    // `*.x` does not include the apex, and exact hosts do not include subdomains.
    assert.equal(allowed('https://miyoushe.com/a.png', hosts), false)
    assert.equal(allowed('https://x.act-webstatic.mihoyo.com/a.png', hosts), false)
    // Scheme, port, credentials and lookalikes stay blocked.
    assert.equal(allowed('http://act-webstatic.mihoyo.com/a.png', hosts), false)
    assert.equal(allowed('https://act-webstatic.mihoyo.com:8443/a.png', hosts), false)
    assert.equal(allowed('https://u:p@act-webstatic.mihoyo.com/a.png', hosts), false)
    assert.equal(allowed('https://evilmiyoushe.com/a.png', hosts), false)
    assert.equal(allowed('https://miyoushe.com.evil.example/a.png', hosts), false)
    assert.equal(allowed('https://evil.example/?u=act-webstatic.mihoyo.com', hosts), false)
    assert.equal(allowed('https://act-webstatic.mihoyo.com/a.png', []), false)
  })

  it('is what the security wrapper installs, with only sanitized hosts', () => {
    const wrapper = generateSecurityWrapper('tok', ['cdn.example.com', "x';alert(1)//"])
    assert.match(wrapper, /function isRemoteMediaUrlAllowed/)
    assert.match(wrapper, /const _REMOTE_MEDIA_HOSTS = \["cdn\.example\.com"\];/)
    assert.doesNotMatch(wrapper, /alert\(1\)/)
    assert.doesNotMatch(wrapper, /_ALLOW_REMOTE_MEDIA/)
  })
})
