import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { it } from 'node:test'
import { applyTextDecorations } from './textDecorations'

const require = createRequire(import.meta.url)
const { JSDOM } = require(
  require.resolve('jsdom', {
    paths: [require.resolve('isomorphic-dompurify')],
  }),
)
it('decorates text without removing media ancestors, preserving selection and loaded embed text', () => {
  const dom = new JSDOM(
    '<main><p>one <em>two</em></p><section><iframe></iframe></section><div class="brew-embed-card">loaded</div></main>',
  )
  const root = dom.window.document.querySelector('main')
  const frame = root.querySelector('iframe')
  const parent = frame.parentElement
  const realm = frame.contentWindow
  const range = dom.window.document.createRange()
  range.selectNodeContents(root.querySelector('em'))
  dom.window.getSelection().addRange(range)
  const observer = new dom.window.MutationObserver(() => {})
  observer.observe(root, { childList: true, subtree: true })
  applyTextDecorations(
    root,
    '<p>one <em><mark class="user-comment-highlight" data-comment-id="1">two</mark></em></p><section><iframe></iframe></section><div class="brew-embed-card">placeholder</div>',
  )
  assert.equal(root.querySelector('mark').textContent, 'two')
  assert.equal(dom.window.getSelection().toString(), 'two')
  assert.equal(frame.parentElement, parent)
  assert.equal(frame.contentWindow, realm)
  assert.equal(root.querySelector('.brew-embed-card').textContent, 'loaded')
  for (const record of observer.takeRecords()) {
    for (const removed of record.removedNodes)
      assert.equal(removed === frame || removed.contains(frame), false)
  }
  applyTextDecorations(
    root,
    '<p>one <em>two</em></p><section><iframe></iframe></section><div class="brew-embed-card">placeholder</div>',
  )
  assert.equal(root.querySelector('mark'), null)
  assert.equal(frame.contentWindow, realm)
  assert.equal(dom.window.getSelection().toString(), 'two')
  observer.disconnect()
  dom.window.close()
})
