import assert from 'node:assert/strict'
import test from 'node:test'
import { parseInlineTokens, parseMarkdownBlocks } from './markdown'

test('行内认得出代码、图片、链接、粗体、斜体，其余原样留着', () => {
  assert.deepEqual(parseInlineTokens('看 `code` 和 **粗** 还有 *斜*'), [
    { kind: 'text', text: '看 ' },
    { kind: 'code', text: 'code' },
    { kind: 'text', text: ' 和 ' },
    { kind: 'bold', text: '粗' },
    { kind: 'text', text: ' 还有 ' },
    { kind: 'italic', text: '斜' },
  ])

  assert.deepEqual(parseInlineTokens('![图](/a.png) 与 [链接](/b)'), [
    { kind: 'image', url: '/a.png', alt: '图' },
    { kind: 'text', text: ' 与 ' },
    { kind: 'link', url: '/b', text: '链接' },
  ])
})

test('连续两次解析互不干扰 —— 带 g 的正则不能共用', () => {
  const first = parseInlineTokens('**甲**')
  const second = parseInlineTokens('**乙**')
  assert.deepEqual(first, [{ kind: 'bold', text: '甲' }])
  assert.deepEqual(second, [{ kind: 'bold', text: '乙' }])
})

test('代码块里的内容不再当 Markdown 认', () => {
  const blocks = parseMarkdownBlocks('```ts\nconst a = **1**\n# 不是标题\n```')
  assert.deepEqual(blocks, [
    { kind: 'code', lang: 'ts', text: 'const a = **1**\n# 不是标题' },
  ])
})

test('没闭合的代码块照样收下 —— 流式回复经常只到开头那三个反引号', () => {
  const blocks = parseMarkdownBlocks('```\nhalf written')
  assert.deepEqual(blocks, [{ kind: 'code', lang: null, text: 'half written' }])
})

test('标题三级封顶，更深的井号当普通段落', () => {
  const blocks = parseMarkdownBlocks('# 一\n## 二\n### 三\n#### 四')
  assert.deepEqual(
    blocks.map((b) => b.kind),
    ['heading', 'heading', 'heading', 'paragraph'],
  )
  assert.equal(blocks[2].kind === 'heading' && blocks[2].level, 3)
})

test('有序和无序列表各自成块，不会串在一起', () => {
  const blocks = parseMarkdownBlocks('- 甲\n- 乙\n1. 一\n2. 二')
  assert.equal(blocks.length, 2)
  assert.equal(blocks[0].kind === 'list' && blocks[0].ordered, false)
  assert.equal(blocks[0].kind === 'list' && blocks[0].items.length, 2)
  assert.equal(blocks[1].kind === 'list' && blocks[1].ordered, true)
  assert.equal(blocks[1].kind === 'list' && blocks[1].items.length, 2)
})

test('引用连着的几行合成一块', () => {
  const blocks = parseMarkdownBlocks('> 第一句\n> 第二句\n后面')
  assert.equal(blocks[0].kind, 'quote')
  assert.equal(blocks[0].kind === 'quote' && blocks[0].lines.length, 2)
  assert.equal(blocks[1].kind, 'paragraph')
})

test('表格要有分隔行才算表格', () => {
  const table = parseMarkdownBlocks('| a | b |\n| --- | --- |\n| 1 | 2 |')
  assert.equal(table[0].kind, 'table')
  assert.equal(table[0].kind === 'table' && table[0].headers.length, 2)
  assert.equal(table[0].kind === 'table' && table[0].rows.length, 1)

  // 一句带竖线的话不该被排成表
  const notTable = parseMarkdownBlocks('| 这只是一句话 |\n下一行')
  assert.deepEqual(
    notTable.map((b) => b.kind),
    ['paragraph', 'paragraph'],
  )
})

test('水平线的三种写法都认', () => {
  for (const rule of ['---', '___', '***']) {
    assert.equal(parseMarkdownBlocks(rule)[0].kind, 'rule')
  }
})

test('空行只作分隔，不产出空段落', () => {
  const blocks = parseMarkdownBlocks('甲\n\n\n乙')
  assert.deepEqual(
    blocks.map((b) => b.kind),
    ['paragraph', 'paragraph'],
  )
})

test('空输入产出空列表，不产出一个空段落', () => {
  assert.deepEqual(parseMarkdownBlocks(''), [])
  assert.deepEqual(parseMarkdownBlocks('\n\n'), [])
})

test('流式往后追加时前面认完的块沿用原对象', () => {
  const first = parseMarkdownBlocks('# 题\n\n第一段')
  const second = parseMarkdownBlocks('# 题\n\n第一段\n\n第二段还在写')
  assert.equal(second[0], first[0])
  assert.equal(second[1], first[1])
  assert.equal(second[2]?.kind, 'paragraph')
  assert.notEqual(second[2], first[1])
})

test('同一段原文再解析一次拿到同一份树', () => {
  const source = '- 甲\n- 乙'
  const first = parseMarkdownBlocks(source)
  const second = parseMarkdownBlocks(source)
  assert.equal(second, first)
})
