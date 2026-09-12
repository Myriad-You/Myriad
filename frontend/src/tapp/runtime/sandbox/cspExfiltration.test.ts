/** connect-src 仅 blob:/data:。img/media 的裸 http(s) 挂 network:fetch。 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

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
  it('does not allow arbitrary remote images without network:fetch', () => {
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

  it('allows remote img/media once network:fetch is granted', () => {
    const csp = generateCSP(
      'n0nce',
      cspOptionsFromPermissions(['network:fetch']),
    )
    assert.ok(/\bhttps:/.test(directive(csp, 'img-src')))
    assert.ok(/\bhttp:/.test(directive(csp, 'img-src')))
    assert.ok(/\bhttps:/.test(directive(csp, 'media-src')))
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
    ]) {
      const csp = generateCSP('n0nce', cspOptionsFromPermissions(perms))
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
