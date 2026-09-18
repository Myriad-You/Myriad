/** XSS: DOMPurify allowlist, not regex denylist + innerHTML. */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isTrustedIframeHost,
  processRssContent,
  processRssContentAsync,
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

  it('keeps math spans and data-tex', () => {
    const out = sanitizeRssHtml(
      '<p>见 <span class="math math-inline" data-tex="E=mc^2">E=mc^2</span></p>'
      + '<div class="notion-equation">$$x^2$$</div>',
    )
    assert.match(out, /class="[^"]*math[^"]*"/)
    assert.match(out, /data-tex="E=mc\^2"/)
    assert.match(out, /notion-equation/)
    assert.doesNotMatch(out, /<math[\s>]/)
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

  it('keeps task-list checkboxes read-only and drops every other input', () => {
    const out = sanitizeRssHtml(
      '<ul><li><input type="checkbox" checked> done</li>'
      + '<li><input type="checkbox"> todo</li></ul>'
      + '<input type="text" name="card"><input type="checkbox" onclick="x()">',
    )
    assert.equal((out.match(/<input\b/g) ?? []).length, 3)
    assert.equal(/type="text"/i.test(out), false)
    assert.equal(/onclick/i.test(out), false)
    assert.equal((out.match(/disabled/g) ?? []).length, 3)
    assert.match(out, /checked/)
  })

  it('keeps table column alignment', () => {
    const out = sanitizeRssHtml('<table><tr><th align="center">a</th></tr></table>')
    assert.match(out, /align="center"/)
  })

  it('keeps footnote anchors so the reader can jump', () => {
    const out = sanitizeRssHtml(
      '<p>a<sup class="footnote-reference"><a href="#note-fn-1">1</a></sup></p>'
      + '<div class="footnote-definition" id="note-fn-1"><sup class="footnote-definition-label">1</sup><p>b</p></div>',
    )
    assert.match(out, /id="note-fn-1"/)
    assert.match(out, /href="#note-fn-1"/)
    assert.match(out, /footnote-definition/)
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

  it('async pipeline matches the sync result', async () => {
    const html =
      '<blockquote>q</blockquote><script>x</script><p onclick="e">hi</p>'
      + '<a href="https://example.com">good</a>'
    assert.equal(await processRssContentAsync(html), processRssContent(html))
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
