import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  canAddCategory,
  joinCategories,
  resolveEditSourcePayload,
  subscriptionModeOf,
} from './editSource.ts'

describe('subscriptionModeOf', () => {
  it('停用走 disabled，Brewlia 走 brewlia，其余走 normal', () => {
    assert.equal(
      subscriptionModeOf({ enabled: false, source_type: 'rss' }),
      'disabled',
    )
    assert.equal(
      subscriptionModeOf({ enabled: true, source_type: 'brewlia' }),
      'brewlia',
    )
    assert.equal(
      subscriptionModeOf({ enabled: true, source_type: 'rsshub' }),
      'normal',
    )
  })
})

describe('canAddCategory', () => {
  it('只有预置分类能挂第二个', () => {
    assert.equal(canAddCategory([]), true)
    assert.equal(canAddCategory(['友情链接']), true)
    assert.equal(canAddCategory(['我']), true)
    assert.equal(canAddCategory(['工程']), false)
    assert.equal(canAddCategory(['友情链接', '工程']), false)
  })
})

describe('joinCategories', () => {
  it('空选不写字段，草稿在限额内并进去', () => {
    assert.equal(joinCategories([]), undefined)
    assert.equal(joinCategories(['友情链接'], '工程'), '友情链接, 工程')
    assert.equal(joinCategories(['友情链接', '工程'], '多余'), '友情链接, 工程')
  })
})

describe('resolveEditSourcePayload', () => {
  it('普通 RSS 切 Brewlia 并带上间隔', () => {
    const payload = resolveEditSourcePayload({
      source: { source_type: 'rss', feed_type: 'rss' },
      name: ' 示例 ',
      selectedCategories: ['工程'],
      updateInterval: 120,
      subscriptionMode: 'brewlia',
      customIcon: null,
      themeColor: '#f97316',
      styleTags: ['冷静'],
      adminOnly: false,
    })
    assert.equal(payload.name, '示例')
    assert.equal(payload.category, '工程')
    assert.equal(payload.update_interval, 120)
    assert.equal(payload.enabled, true)
    assert.equal(payload.source_type, 'brewlia')
    assert.equal(payload.theme_color, '#f97316')
    assert.deepEqual(payload.ai_style_tags, ['冷静'])
    assert.equal(payload.icon, undefined)
  })

  it('RSSHub 回到普通模式时 source_type 仍是 rsshub', () => {
    const payload = resolveEditSourcePayload({
      source: { source_type: 'brewlia', feed_type: 'rsshub' },
      name: 'hub',
      selectedCategories: [],
      updateInterval: 60,
      subscriptionMode: 'normal',
      customIcon: null,
      themeColor: '',
      styleTags: [],
      adminOnly: true,
    })
    assert.equal(payload.source_type, 'rsshub')
    assert.equal(payload.enabled, true)
    assert.equal(payload.theme_color, '')
    assert.equal(payload.admin_only, true)
  })

  it('停止抓取只关 enabled，纯链接不写间隔', () => {
    const paused = resolveEditSourcePayload({
      source: { source_type: 'rss', feed_type: 'atom' },
      name: 'a',
      selectedCategories: [],
      updateInterval: 30,
      subscriptionMode: 'disabled',
      customIcon: '',
      themeColor: '',
      styleTags: [],
      adminOnly: false,
    })
    assert.equal(paused.enabled, false)
    assert.equal(paused.update_interval, 30)
    assert.equal(paused.icon, '')

    const link = resolveEditSourcePayload({
      source: { source_type: 'link', feed_type: 'rss' },
      name: '入口',
      selectedCategories: ['友情链接'],
      updateInterval: 60,
      subscriptionMode: 'normal',
      customIcon: null,
      themeColor: '#111111',
      styleTags: ['朋友'],
      adminOnly: false,
    })
    assert.equal(link.update_interval, undefined)
    assert.equal(link.enabled, undefined)
    assert.equal(link.source_type, undefined)
    assert.equal(link.category, '友情链接')
  })
})
