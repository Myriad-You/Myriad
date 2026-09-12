import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { cssBackgroundImage, cssUrl } from './cssUrl'

describe('cssUrl', () => {
  it('quotes and wraps a plain https URL', () => {
    assert.equal(
      cssUrl('https://cdn.example.com/a.jpg'),
      'url("https://cdn.example.com/a.jpg")',
    )
  })

  it('escapes quotes and backslashes', () => {
    assert.equal(cssUrl('https://x.com/a"b\\c'), 'url("https://x.com/a\\"b\\\\c")')
  })

  it('escapes closing paren inside the string so declaration stays one token', () => {
    const out = cssUrl('https://x.com/a)b')
    assert.equal(out, 'url("https://x.com/a)b")')
    assert.ok(out.startsWith('url("') && out.endsWith('")'))
  })

  it('returns none for empty', () => {
    assert.equal(cssUrl(''), 'none')
    assert.equal(cssUrl(null), 'none')
    assert.equal(cssBackgroundImage(undefined), 'none')
  })
})
