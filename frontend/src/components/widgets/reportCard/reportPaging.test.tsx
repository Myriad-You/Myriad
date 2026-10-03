import type { Root } from 'react-dom/client'
import type { ReportDetailPaging } from './reportPaging'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { after, afterEach, beforeEach, it } from 'node:test'
import { act, createElement } from 'react'
import { pairPageCount, ReportDetailPagingContext, useReportDetailPage } from './reportPaging'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const dom = new JSDOM('<div id="root"></div>')
const prior = new Map<string, PropertyDescriptor | undefined>()
for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true })) {
  prior.set(key, Object.getOwnPropertyDescriptor(globalThis, key))
  Object.defineProperty(globalThis, key, { configurable: true, value })
}
const { createRoot } = await import('react-dom/client')
after(() => {
  dom.window.close()
  for (const [key, descriptor] of prior) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor)
    else Reflect.deleteProperty(globalThis, key)
  }
})

let root: Root
let seen: Array<number | null>
beforeEach(() => {
  seen = []
  root = createRoot(document.getElementById('root')!)
})
afterEach(async () => {
  await act(async () => root.unmount())
})

function Face({ pages }: { pages: number }) {
  seen.push(useReportDetailPage(pages, 5000))
  return null
}

it('returns null without a provider so embedded cards keep their own rotation', async () => {
  await act(async () => root.render(createElement(Face, { pages: 4 })))
  assert.equal(seen.at(-1), null)
})

it('reports the detail page count and follows the card page, withdrawing on unmount', async () => {
  const registered: Array<[number, number]> = []
  const paging = (detailIndex: number): ReportDetailPaging => ({
    detailIndex,
    register: (pages, dwell) => registered.push([pages, dwell]),
  })
  const render = (detailIndex: number, face: boolean) =>
    root.render(createElement(ReportDetailPagingContext, { value: paging(detailIndex) }, face ? createElement(Face, { pages: 3 }) : null))
  await act(async () => render(1, true))
  assert.equal(seen.at(-1), 1)
  assert.deepEqual(registered.at(-1), [3, 5000])
  await act(async () => render(5, true))
  assert.equal(seen.at(-1), 2, 'wraps past the last page')
  await act(async () => render(0, false))
  assert.deepEqual(registered.at(-1), [0, 5000], 'a face that goes away withdraws its pages')
})

it('counts two items per page for paired details', () => {
  assert.equal(pairPageCount(0), 0)
  assert.equal(pairPageCount(1), 1)
  assert.equal(pairPageCount(5), 3)
})

it('keeps the page registration while only the current detail page changes', async () => {
  const registered: Array<[number, number]> = []
  const register = (pages: number, dwell: number) => registered.push([pages, dwell])
  const render = (detailIndex: number, pages = 3) => root.render(
    createElement(ReportDetailPagingContext, { value: { detailIndex, register } }, createElement(Face, { pages })),
  )
  await act(async () => render(0))
  await act(async () => render(1))
  assert.equal(seen.at(-1), 1)
  assert.deepEqual(registered, [[3, 5000]], 'paging must not withdraw and register the same count again')
  await act(async () => render(2, 4))
  assert.deepEqual(registered, [[3, 5000], [0, 5000], [4, 5000]], 'a changed page count still updates the registration')
})
