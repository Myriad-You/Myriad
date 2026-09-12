/** XSS: DOMPurify allowlist, not regex denylist + innerHTML. */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isTrustedIframeHost,
  processRssContent,
  sanitizeRssHtml,
  selfCheckSanitize,
  stripUntrustedIframes,
} from './rssContentProcessor'

describe('sanitizeRssHtml (DOMPurify allowlist)', () => {
  it('strips script tags and event handlers', () => {
    const out = sanitizeRssHtml(
      '<p>safe</p><script>alert(1)</script><img src="x" onerror="alert(1)">',
    )
    assert.equal(/<script/i.test(out), false)
    assert.equal(/onerror/i.test(out), false)
    assert.match(out, /safe/)
  })

  it('strips javascript: hrefs', () => {
    const out = sanitizeRssHtml('<a href="javascript:alert(1)">click</a>')
    assert.equal(/javascript:/i.test(out), false)
  })

  it('drops untrusted iframes but keeps trusted hosts', () => {
    const evil = sanitizeRssHtml(
      '<iframe src="https://evil.example/phish"></iframe>',
    )
    assert.equal(/evil\.example/i.test(evil), false)

    const yt = sanitizeRssHtml(
      '<iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>',
    )
    assert.match(yt, /youtube\.com/i)
  })

  it('drops srcdoc iframes', () => {
    const out = sanitizeRssHtml(
      '<iframe srcdoc="<script>alert(1)</script>"></iframe>',
    )
    assert.equal(/srcdoc/i.test(out), false)
    assert.equal(/<script/i.test(out), false)
  })

  it('preserves basic formatting, links, and images', () => {
    const out = sanitizeRssHtml(
      '<p>Hello <strong>world</strong> and <em>more</em></p>'
      + '<a href="https://example.com/path">link</a>'
      + '<img src="https://cdn.example.com/a.jpg" alt="cover">',
    )
    assert.match(out, /<strong/i)
    assert.match(out, /<em/i)
    assert.match(out, /href="https:\/\/example\.com\/path"/)
    assert.match(out, /<img\b/i)
    assert.match(out, /alt="cover"/)
  })

  it('handles nested script mutation payloads', () => {
    const out = sanitizeRssHtml(
      '<div><scr<script>ipt>alert(1)</script></div>',
    )
    // DOMPurify must not leave an executable <script>.
    assert.equal(/<script/i.test(out), false)
    assert.equal(/on\w+\s*=/i.test(out), false)
  })
})

describe('processRssContent', () => {
  it('keeps presentation classes theme-agnostic', () => {
    const out = processRssContent(
      '<blockquote>q</blockquote><pre>code</pre><table><tr><td>1</td></tr></table><mark>m</mark><hr>',
    )
    assert.match(out, /rss-content-blockquote/)
    assert.match(out, /rss-content-pre/)
    assert.match(out, /rss-content-table/)
    assert.match(out, /rss-content-mark/)
    assert.equal(/bg-white\/|bg-black\/|bg-yellow-/.test(out), false)
  })

  it('still sanitizes end-to-end after presentation rewrites', () => {
    const out = processRssContent(
      '<p onclick="evil()">hi</p><script>x</script>'
      + '<a href="javascript:alert(1)">bad</a>'
      + '<a href="https://example.com">good</a>',
    )
    assert.equal(/onclick/i.test(out), false)
    assert.equal(/<script/i.test(out), false)
    assert.equal(/javascript:/i.test(out), false)
    assert.match(out, /example\.com/)
    assert.match(out, /hi/)
  })

  it('selfCheckSanitize passes', () => {
    const failures = selfCheckSanitize()
    assert.deepEqual(failures, [])
  })
})

describe('iframe host policy', () => {
  it('trusts known players only', () => {
    assert.equal(isTrustedIframeHost('player.bilibili.com'), true)
    assert.equal(isTrustedIframeHost('www.youtube.com'), true)
    assert.equal(isTrustedIframeHost('evil.example'), false)
  })

  it('stripUntrustedIframes removes unknown hosts', () => {
    const out = stripUntrustedIframes(
      '<iframe src="https://evil.example/x"></iframe>'
      + '<iframe src="https://player.bilibili.com/player.html?bvid=1"></iframe>',
    )
    assert.equal(/evil\.example/i.test(out), false)
    assert.match(out, /player\.bilibili\.com/)
  })
})
