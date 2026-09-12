/**
 * @vitest-environment node
 */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  emptyFooterCustomItem,
  FOOTER_CUSTOM_MAX,
  isFooterCustomHref,
  parseFooterCustom,
  parseFooterCustomSlots,
  serializeFooterCustom,
  stripHtmlTags,
} from './footerCustomLogic'

describe('footerCustomLogic', () => {
  it('parses valid array and caps at max', () => {
    const raw = JSON.stringify([
      { text: 'A', icon: '/a.png', url: 'https://a.example' },
      { text: 'B', icon: '', url: '' },
      { text: 'C', icon: '', url: '' },
    ])
    const items = parseFooterCustom(raw)
    assert.equal(items.length, FOOTER_CUSTOM_MAX)
    assert.equal(items[0]?.text, 'A')
    assert.equal(items[1]?.text, 'B')
  })

  it('drops empty text for display parse', () => {
    assert.deepEqual(parseFooterCustom('[{}]'), [])
    assert.deepEqual(parseFooterCustom('[{"text":""}]'), [])
    assert.deepEqual(parseFooterCustom('not-json'), [])
    assert.deepEqual(parseFooterCustom(''), [])
  })

  it('keeps empty slots for editor parse/serialize', () => {
    const draft = serializeFooterCustom([emptyFooterCustomItem()])
    assert.equal(draft, JSON.stringify([{ text: '', icon: '', url: '' }]))
    const slots = parseFooterCustomSlots(draft)
    assert.equal(slots.length, 1)
    assert.equal(slots[0]?.text, '')
    assert.deepEqual(parseFooterCustom(draft), [])
  })

  it('serializes and round-trips filled items', () => {
    const s = serializeFooterCustom([
      { text: ' Hi ', icon: ' /x.webp ', url: ' https://x ' },
      { text: '', icon: 'ignored', url: '' },
    ])
    assert.equal(
      s,
      JSON.stringify([
        { text: 'Hi', icon: '/x.webp', url: 'https://x' },
        { text: '', icon: 'ignored', url: '' },
      ]),
    )
    assert.equal(serializeFooterCustom([]), '')
  })

  it('validates href schemes', () => {
    assert.equal(isFooterCustomHref('https://ok.example'), true)
    assert.equal(isFooterCustomHref('/path'), true)
    assert.equal(isFooterCustomHref('javascript:alert(1)'), false)
    assert.equal(isFooterCustomHref(''), false)
  })

  it('strips html tags and entities from text', () => {
    assert.equal(stripHtmlTags('<b>Hello</b>'), 'Hello')
    assert.equal(
      stripHtmlTags('<script>alert(1)</script>hi'),
      'hi',
    )
    assert.equal(stripHtmlTags('a &amp; b &lt;c&gt;'), 'a & b <c>')
    assert.equal(
      parseFooterCustom(
        JSON.stringify([{ text: '<img src=x onerror=alert(1)>友链', icon: '', url: '' }]),
      )[0]?.text,
      '友链',
    )
    assert.equal(
      serializeFooterCustom([
        { text: '<b>X</b>', icon: '', url: '' },
      ]),
      JSON.stringify([{ text: 'X', icon: '', url: '' }]),
    )
  })
})
