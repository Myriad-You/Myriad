import type { AnnotationItem } from '../../services/phantasiaiApi'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { describe, it } from 'node:test'
import {
  COMPANION_HOST_ID,
  companionHost,
  connectArticle,
  findCompanionPage,
  paintAnnotations,
  termCandidates,
  unpaintAnnotations,
} from './companionBridge.ts'

const require = createRequire(import.meta.url)
const { JSDOM } = require(
  require.resolve('jsdom', {
    paths: [require.resolve('isomorphic-dompurify')],
  }),
)

function page(body: string) {
  const dom = new JSDOM(
    `<article><div class="content">${body}<iframe class="myriad-companion__frame" src="/embed/companion/7" hidden></iframe></div></article>`,
    { url: 'https://myriad.test/blog/posts/a/' },
  )
  const doc = dom.window.document as Document
  const frame = doc.querySelector('iframe')!
  return { dom, doc, frame, root: doc.querySelector('.content') as HTMLElement }
}

function note(term: string, extra: Partial<AnnotationItem> = {}): AnnotationItem {
  return {
  type: 'term',
  term,
  explanation: `${term} 的解释`,
  ...extra,
}
}

describe('companion bridge', () => {
  it('finds the article the frame was written into, and makes one host at the end of the body', () => {
    const { doc, frame, root } = page('<p>正文</p>')
    const found = findCompanionPage(frame)!
    assert.equal(found.root, root)
    assert.equal(findCompanionPage(null), null)
    assert.equal(findCompanionPage(doc.createElement('iframe')), null)
    const host = companionHost(found)
    assert.equal(host.id, COMPANION_HOST_ID)
    assert.equal(doc.body.lastElementChild, host)
    assert.equal(companionHost(found), host)
  })

  it('prefers the AI offset, then every occurrence in order', () => {
    assert.deepEqual(termCandidates('a Rust b Rust', 'Rust', 9), [9, 2])
    assert.deepEqual(termCandidates('a Rust b Rust', 'Rust', 1), [2, 9])
    assert.deepEqual(termCandidates('abc', ''), [])
  })

  it('marks plain text once per term, skipping links, code and frames', () => {
    const { root } = page(
      '<p><a href="/x">Rust</a> 与 <code>Rust</code></p><pre>Rust</pre><p>后来 Rust 稳定了，Rust 很快。</p>',
    )
    const painted = paintAnnotations(root, [note('不存在'), note('Rust'), note('伴读')])
    assert.deepEqual(painted, [1])
    const marks = [...root.querySelectorAll('mark.myriad-annot')]
    assert.equal(marks.length, 1)
    assert.equal(marks[0].textContent, 'Rust')
    assert.equal(marks[0].closest('p')?.textContent, '后来 Rust 稳定了，Rust 很快。')
    assert.equal(root.querySelector('a mark, pre mark, code mark, iframe mark'), null)
  })

  it('does not nest a short term inside a longer one, and unpaints cleanly', () => {
    const { root } = page('<p>Rust 编译器 很严格。</p>')
    const before = root.innerHTML
    paintAnnotations(root, [note('Rust'), note('Rust 编译器')])
    const marks = [...root.querySelectorAll('mark.myriad-annot')]
    assert.deepEqual(marks.map((mark) => mark.textContent), ['Rust 编译器'])
    assert.equal(root.querySelector('mark mark'), null)
    unpaintAnnotations(root)
    assert.equal(root.innerHTML, before)
  })

  it('shows AI text as text, reports picks and Esc, and leaves nothing behind', () => {
    const { dom, doc, frame, root } = page('<p>谈谈 XSS 吧。</p>')
    const found = findCompanionPage(frame)!
    const picked: number[] = []
    let escaped = 0
    const link = connectArticle(found, {
      typeLabel: (type) => `类型:${type}`,
      onPick: (index) => picked.push(index),
      onEscape: () => { escaped += 1 },
    })
    const hostile = note('XSS', { explanation: '<img src=x onerror=alert(1)>' })
    assert.deepEqual(link.paint([hostile]), [0])
    const mark = root.querySelector('mark.myriad-annot') as HTMLElement
    mark.dispatchEvent(new dom.window.MouseEvent('mouseover', { bubbles: true }))
    const tip = doc.getElementById('myriad-annot-tip')!
    assert.equal(tip.hidden, false)
    assert.equal(tip.querySelector('img'), null)
    assert.match(tip.textContent ?? '', /<img src=x/)
    assert.equal(mark.getAttribute('aria-describedby'), 'myriad-annot-tip')

    mark.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }))
    assert.deepEqual(picked, [0])
    doc.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape' }))
    assert.equal(escaped, 1)
    assert.equal(tip.hidden, true)

    link.dispose()
    assert.equal(doc.getElementById('myriad-annot-tip'), null)
    assert.equal(root.querySelector('mark'), null)
  })
})
