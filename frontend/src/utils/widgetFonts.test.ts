import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  sanitizeWidgetFontUrl,
  widgetFontFamilyName,
} from './widgetFonts'

const sample =
  '/api/home/widget-fonts/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.woff2'

describe('sanitizeWidgetFontUrl', () => {
  it('accepts content-addressed host font URLs', () => {
    assert.equal(sanitizeWidgetFontUrl(sample), sample)
    assert.equal(
      sanitizeWidgetFontUrl(
        '/api/home/widget-fonts/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.otf',
      ).endsWith('.otf'),
      true,
    )
  })

  it('rejects anything else', () => {
    assert.equal(sanitizeWidgetFontUrl(''), '')
    assert.equal(sanitizeWidgetFontUrl('/fonts/hoyo/GenshinUI-subset.woff2'), '')
    assert.equal(sanitizeWidgetFontUrl('https://example.test/x.woff2'), '')
    assert.equal(sanitizeWidgetFontUrl('/api/home/widget-fonts/../etc/passwd'), '')
    assert.equal(sanitizeWidgetFontUrl('/api/home/widget-fonts/abc.woff2'), '')
  })
})

describe('widgetFontFamilyName', () => {
  it('is a CSS-safe prefix of the hash', () => {
    assert.equal(widgetFontFamilyName(sample), 'gpaaaaaaaaaaaaaaaa')
  })
})
