import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { commentPlainText, safeCommentUrl } from './commentMarkdown.ts'

describe('safeCommentUrl', () => {
  it('只放行 http(s)', () => {
    assert.equal(safeCommentUrl('https://example.com/a'), 'https://example.com/a')
    assert.equal(safeCommentUrl(' http://example.com '), 'http://example.com/')
    assert.equal(safeCommentUrl('javascript:alert(1)'), null)
    assert.equal(safeCommentUrl('JaVaScRiPt:alert(1)'), null)
    assert.equal(safeCommentUrl('data:text/html,hi'), null)
    assert.equal(safeCommentUrl('/relative'), null)
    assert.equal(safeCommentUrl('//example.com'), null)
  })
})

describe('commentPlainText', () => {
  it('去掉 Markdown 记号', () => {
    assert.equal(commentPlainText('**很好** 的 `code`'), '很好 的 code')
    assert.equal(commentPlainText('> 引用\n\n- 一\n- 二'), '引用 一 二')
    assert.equal(commentPlainText('[链接](https://a.com) ![图](https://b.com/x.png)'), '链接 图')
  })
})
