/**
 * Run from frontend/:
 *   pnpm test:unit -- src/components/settings/guides/configSearch.test.ts
 */

/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  extractMatchSnippet,
  itemMatchesQuery,
  parseSearchQuery,
  rankConfigSearch,
  scoreSearchItem,
  type ConfigSearchableItem,
} from './configSearch.ts'

const sample: ConfigSearchableItem[] = [
  {
    type: 'section',
    section: 'advanced',
    title: '高级配置',
    description: '代理与备份',
    keywords: ['proxy', '代理', 'reset'],
    haystack: '高级配置\n代理与备份\nproxy 代理 reset',
  },
  {
    type: 'guide',
    section: 'advanced',
    title: '服务器出外网时是否使用代理',
    description: '间接：同步/AI 成功率',
    keywords: ['代理', 'proxy', '外网'],
    haystack:
      '服务器出外网时是否使用代理 打开后按下面的代理地址访问外网 国内平台',
    guidePath: 'advanced.proxyEnable',
  },
  {
    type: 'guide',
    section: 'basic',
    title: '工信部 ICP 备案号',
    description: '页脚备案文字',
    keywords: ['备案', 'icp'],
    haystack: '工信部 icp 备案号 有才填 页脚多出备案这一行',
    guidePath: 'ui.siteIcp',
  },
  {
    type: 'section',
    section: 'platforms',
    title: '数据及统计',
    description: '接入平台与访客统计',
    keywords: ['github', 'steam'],
  },
]

describe('parseSearchQuery', () => {
  it('splits on spaces and full-width space', () => {
    assert.deepEqual(parseSearchQuery('  代理  同步  '), ['代理', '同步'])
    assert.deepEqual(parseSearchQuery('proxy\u3000sync'), ['proxy', 'sync'])
  })
})

describe('itemMatchesQuery', () => {
  it('requires AND for multi tokens', () => {
    const item = sample[1]!
    assert.equal(itemMatchesQuery(item, ['代理']), true)
    assert.equal(itemMatchesQuery(item, ['代理', '外网']), true)
    assert.equal(itemMatchesQuery(item, ['代理', '备案']), false)
  })
})

describe('scoreSearchItem', () => {
  it('ranks title hits above body-only', () => {
    const guideTitle = scoreSearchItem(sample[1]!, ['代理'])
    const section = scoreSearchItem(sample[0]!, ['代理'])
    assert.ok(guideTitle)
    assert.ok(section)
    assert.ok(guideTitle.score > 0)
    assert.ok(section.score > 0)
  })

  it('prefers exact title match', () => {
    const item: ConfigSearchableItem = {
      type: 'section',
      section: 'ai',
      title: 'ai',
      description: 'models',
      keywords: [],
    }
    const exact = scoreSearchItem(item, ['ai'])!
    const partial = scoreSearchItem(
      { ...item, title: 'ai provider settings' },
      ['ai'],
    )!
    assert.ok(exact.score >= partial.score)
  })
})

describe('extractMatchSnippet', () => {
  it('wraps match with ellipsis context', () => {
    const hay = 'abcdefghij代理服务器地址klmnopqrstuvwxyz'
    const snip = extractMatchSnippet(hay, ['代理'], 'fallback', 4)
    assert.ok(snip.includes('代理'))
    assert.ok(snip.length < hay.length)
  })
})

describe('rankConfigSearch', () => {
  it('returns empty for blank query', () => {
    assert.deepEqual(rankConfigSearch(sample, '   '), [])
  })

  it('finds 备案 via guide haystack', () => {
    const r = rankConfigSearch(sample, '备案')
    assert.ok(r.some(x => x.guidePath === 'ui.siteIcp'))
  })

  it('caps guides per section', () => {
    const many: ConfigSearchableItem[] = Array.from({ length: 8 }, (_, i) => ({
      type: 'guide' as const,
      section: 'advanced',
      title: `代理相关说明 ${i}`,
      description: 'desc',
      keywords: ['代理'],
      haystack: `代理 说明 ${i}`,
      guidePath: `advanced.x${i}`,
    }))
    const r = rankConfigSearch(many, '代理', { maxGuidesPerSection: 3 })
    assert.equal(r.filter(x => x.type === 'guide').length, 3)
  })

  it('AND query 代理 备案 matches neither alone item', () => {
    const r = rankConfigSearch(sample, '代理 备案')
    // no single item has both
    assert.equal(r.length, 0)
  })
})
