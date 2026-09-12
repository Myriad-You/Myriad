import type { CommentItem } from '../../../services/brewApi'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { it } from 'node:test'
import {
  commentAnchorStale,
  cssCustomHighlightAvailable,
  highlightAnchoredAnnotations,
  highlightAnchoredComments,
  resolveCommentAnchor,
} from './commentAnchors'

const require = createRequire(import.meta.url)
const { JSDOM } = require(
  require.resolve('jsdom', {
    paths: [require.resolve('isomorphic-dompurify')],
  }),
)

it('treats a comment as stale only when both sides have a version and they differ', () => {
  assert.equal(commentAnchorStale({}, 3), false)
  assert.equal(commentAnchorStale({ content_revision: 3 }, 3), false)
  assert.equal(commentAnchorStale({ content_revision: 2 }, 3), true)
  assert.equal(commentAnchorStale({ content_revision: 2 }, undefined), false)
})

it('uses offsets and context to distinguish repeated text without guessing ambiguous legacy anchors', () => {
  const text = 'first word; second word'
  assert.equal(
    resolveCommentAnchor(text, {
      selected_text: 'word',
      start_offset: 19,
      end_offset: 23,
    }),
    19,
  )
  assert.equal(
    resolveCommentAnchor(text, {
      selected_text: 'word',
      start_offset: 6,
      context_before: 'second ',
    }),
    19,
  )
  assert.equal(resolveCommentAnchor(text, { selected_text: 'word' }), null)
  assert.equal(resolveCommentAnchor(text, { selected_text: 'missing' }), null)
})

it('highlights a cross-tag quote once, preserves markup and excludes media text', () => {
  const dom = new JSDOM('')
  const original = Object.getOwnPropertyDescriptor(globalThis, 'DOMParser')
  Object.defineProperty(globalThis, 'DOMParser', {
    configurable: true,
    value: dom.window.DOMParser,
  })
  try {
    const html =
      '<p>one <em>two</em> then one two</p><div class="brew-embed-card">media text</div>'
    const comments = [
      { id: 1, selected_text: 'one two', start_offset: 0, end_offset: 7 },
      { id: 2, selected_text: 'two', start_offset: 4, end_offset: 7 },
      { id: 3, selected_text: 'media text' },
    ] as CommentItem[]
    const result = new dom.window.DOMParser().parseFromString(
      highlightAnchoredComments(html, comments, 'light'),
      'text/html',
    )
    assert.equal(result.body.textContent, 'one two then one twomedia text')
    assert.equal(
      Iterator.from(result.querySelectorAll('[data-comment-id="1"]'))
        .map((mark: Element) => mark.textContent)
        .toArray()
        .join(''),
      'one two',
    )
    assert.equal(
      result.querySelector('em [data-comment-id="2"]').textContent,
      'two',
    )
    assert.equal(result.querySelector('[data-comment-id="3"]'), null)
    assert.equal(
      result.querySelector('p').lastChild.textContent,
      ' then one two',
    )
  } finally {
    if (original) Object.defineProperty(globalThis, 'DOMParser', original)
    else Reflect.deleteProperty(globalThis, 'DOMParser')
    dom.window.close()
  }
})

it('annotates a unique term with the comment index and skips repeats and media', () => {
  const dom = new JSDOM('')
  const original = Object.getOwnPropertyDescriptor(globalThis, 'DOMParser')
  Object.defineProperty(globalThis, 'DOMParser', {
    configurable: true,
    value: dom.window.DOMParser,
  })
  try {
    const html =
      '<p>alpha <em>term</em> later term</p><div class="brew-embed-card">solo</div>'
    const result = new dom.window.DOMParser().parseFromString(
      highlightAnchoredAnnotations(html, [
        { type: 'term', term: 'term', explanation: 'n', position: 6 },
        { type: 'term', term: 'solo', explanation: 'media' },
        { type: 'term', term: 'missing', explanation: 'no' },
      ]),
      'text/html',
    )
    assert.equal(
      result.querySelector('em .brewlia-annotation')?.textContent,
      'term',
    )
    assert.equal(result.querySelectorAll('.brewlia-annotation').length, 1)
    assert.equal(result.querySelector('.brew-embed-card .brewlia-annotation'), null)
    assert.equal(typeof cssCustomHighlightAvailable(), 'boolean')
    const ambiguous = highlightAnchoredAnnotations(
      '<p>term then term</p>',
      [{ type: 'term', term: 'term', explanation: 'n' }],
    )
    assert.equal(ambiguous.includes('brewlia-annotation'), false)
  } finally {
    if (original) Object.defineProperty(globalThis, 'DOMParser', original)
    else Reflect.deleteProperty(globalThis, 'DOMParser')
    dom.window.close()
  }
})
