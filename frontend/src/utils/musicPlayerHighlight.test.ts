/** 远端曲名进 innerHTML：无搜索词也必须转义。 */

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import { escapeHtmlText, highlightText } from './musicPlayer.ts'

const PAYLOAD = '<img src=x onerror=alert(1)>'

describe('highlightText', () => {
  it('escapes HTML when there is no search query', () => {
    // 空 query 也必须转义
    for (const query of ['', '   ']) {
      const out = highlightText(PAYLOAD, query)
      assert.ok(
        !out.includes('<img'),
        `raw tag leaked with query=${JSON.stringify(query)}`,
      )
      assert.ok(out.includes('&lt;img'))
      assert.ok(!out.includes('onerror=alert(1)>'))
    }
  })

  it('escapes HTML while highlighting a match', () => {
    const out = highlightText(PAYLOAD, 'img')
    assert.ok(!out.includes('<img'))
    // 高亮标记本身仍然是真标签
    assert.ok(out.includes('<mark>'))
    assert.ok(out.includes('&lt;<mark>img</mark>'))
  })

  it('escapes quotes and ampersands', () => {
    assert.equal(
      escapeHtmlText(`a & b < c > d " e ' f`),
      'a &amp; b &lt; c &gt; d &quot; e &#39; f',
    )
  })

  it('treats regex metacharacters in the query as literals', () => {
    // 曲名里的 `.` `*` 不应该被当成正则
    const out = highlightText('a.b*c', '.')
    assert.ok(out.includes('<mark>.</mark>'))
    assert.ok(!out.includes('<mark>a</mark>'))
  })

  it('leaves ordinary titles intact', () => {
    assert.equal(highlightText('Bohemian Rhapsody', ''), 'Bohemian Rhapsody')
  })

  it('matches punctuation, escapes and Unicode literally in every occurrence', () => {
    for (const query of ['[x]', '(a)+', 'a-b', 'a/b', '\\d', '音楽', '\uD800']) {
      assert.equal(
        highlightText(`${query} ... ${query}`, query),
        `<mark>${query}</mark> ... <mark>${query}</mark>`,
      )
    }
  })
})

/**
 * 上面那些用例其实**盖不住**真正的缺陷 —— highlightText 一直是转义的，出问题的是
 * 调用方绕过了它。这个不变量（"每个 __html 都必须是 highlightText 的返回值"）
 * 类型系统表达不了：dangerouslySetInnerHTML 接受任意字符串。
 *
 * 项目没有 DOM/React 测试环境（test:unit 是 `tsx --test`，无 jsdom），所以这里
 * 直接对源码断言。粗糙，但它精确地锁住了会复发的那一行。
 */
describe('MusicPlayer innerHTML call sites', () => {
  it('only ever feeds highlightText output into dangerouslySetInnerHTML', () => {
    const componentPath = fileURLToPath(
      new URL('../components/ControlPanel/MusicPlayer.tsx', import.meta.url),
    )
    const source = readFileSync(componentPath, 'utf8')

    // 取每个 `__html:` 到其后第一个行尾逗号之间的表达式。
    // 用字符串切分而不是正则，省得为了跨行匹配写出会回溯爆炸的模式。
    const htmlExpressions = source
      .split('__html:')
      .slice(1)
      .map((chunk) => {
        const end = chunk.indexOf(',\n')
        return (end === -1 ? chunk : chunk.slice(0, end)).trim()
      })

    assert.ok(
      htmlExpressions.length > 0,
      'expected to find __html call sites; did the component move?',
    )
    for (const expr of htmlExpressions) {
      assert.ok(
        expr.startsWith('highlightText('),
        `dangerouslySetInnerHTML must use highlightText(); found: ${expr}`,
      )
    }
  })
})
