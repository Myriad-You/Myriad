import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  addFieldKind,
  addHintKey,
  addSubmitLabelKey,
  addUrlPlaceholder,
  canAutoDiscover,
  canSubmitAdd,
  faviconForUrl,
  isOpmlFilename,
  pickAddKind,
  resolveAddSourceType,
  toAddSourceInput,
} from './addSource.ts'

describe('addFieldKind / canAutoDiscover', () => {
  it('只有普通 RSS 才自动探测', () => {
    assert.equal(addFieldKind('rss', 'rss'), 'rss')
    assert.equal(addFieldKind('brewlia', 'rss'), 'rss')
    assert.equal(addFieldKind('rss', 'notion'), 'notion')
    assert.equal(addFieldKind('rsshub', 'rsshub'), 'rsshub')
    assert.equal(addFieldKind('link', 'rss'), 'link')
    assert.equal(canAutoDiscover('rss', 'rss'), true)
    assert.equal(canAutoDiscover('brewlia', 'rss'), true)
    assert.equal(canAutoDiscover('link', 'rss'), false)
    assert.equal(canAutoDiscover('rsshub', 'rsshub'), false)
    assert.equal(canAutoDiscover('rss', 'notion'), false)
  })
})

describe('resolveAddSourceType / canSubmitAdd', () => {
  it('RSSHub 开 AI 才落成 brewlia；入口型必须有名字', () => {
    assert.equal(resolveAddSourceType('rsshub', true), 'brewlia')
    assert.equal(resolveAddSourceType('rsshub', false), 'rsshub')
    assert.equal(resolveAddSourceType('rss', true), 'rss')
    assert.equal(
      canSubmitAdd({
        sourceType: 'rsshub',
        url: '',
        name: '',
        rsshubFullUrl: 'https://hub.test/x',
      }),
      true,
    )
    assert.equal(
      canSubmitAdd({
        sourceType: 'link',
        url: 'https://a.test',
        name: '',
        rsshubFullUrl: '',
      }),
      false,
    )
    assert.equal(
      canSubmitAdd({
        sourceType: 'link',
        url: 'https://a.test',
        name: '站',
        rsshubFullUrl: '',
      }),
      true,
    )
  })
})

describe('pickAddKind / labels / opml', () => {
  it('选类型清 RSSHub 地址；文案键和扩展名对得上', () => {
    assert.deepEqual(pickAddKind('rsshub'), {
      sourceType: 'rsshub',
      feedType: 'rsshub',
      clearUrl: true,
    })
    assert.equal(pickAddKind('notion').feedType, 'notion')
    assert.equal(addHintKey('link'), 'linkDesc')
    assert.equal(addSubmitLabelKey('brewlia'), 'addBrewlia')
    assert.equal(addUrlPlaceholder('notion'), 'notion://database/xxx')
    assert.equal(isOpmlFilename('a.opml'), true)
    assert.equal(isOpmlFilename('a.xml'), true)
    assert.equal(isOpmlFilename('a.txt'), false)
  })
})

describe('toAddSourceInput / faviconForUrl', () => {
  it('空名字和分类不传；坏 URL 不抛', () => {
    const input = toAddSourceInput({
      url: 'https://a.test/feed',
      name: '  ',
      category: '  科技  ',
      customIcon: null,
      sourceType: 'rss',
      feedType: 'rss',
      notionToken: '  tok  ',
    })
    assert.equal(input.name, undefined)
    assert.equal(input.category, '科技')
    assert.equal(input.icon, undefined)
    assert.equal(input.notionToken, 'tok')
    assert.equal(faviconForUrl('not a url'), null)
    assert.match(faviconForUrl('https://a.test/x') ?? '', /a\.test/)
  })
})
