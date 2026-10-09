import type { Root } from 'react-dom/client'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { after, afterEach, beforeEach, it } from 'node:test'
import React, { act, StrictMode } from 'react'
import { BrowserRouter } from 'react-router-dom'
import { NavigationProvider, useNavigation, useSecondaryNav } from '../contexts/NavigationContext'
import { LIBRARY_FILTERS, useLibraryNavigation } from './useLibraryNavigation'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const dom = new JSDOM('<div id="root"></div>', { url: 'https://myriad.test/library' })
const prior = new Map<string, PropertyDescriptor | undefined>()
for (const [key, value] of Object.entries({
  React,
  window: dom.window,
  document: dom.window.document,
  navigator: dom.window.navigator,
  IS_REACT_ACT_ENVIRONMENT: true,
})) {
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

const items = LIBRARY_FILTERS.map(id => ({ id, label: id, icon: null }))
function LibraryProbe() {
  const { filter } = useLibraryNavigation(items, 'Expand')
  return <output data-filter>{filter}</output>
}
function NavProbe() {
  const { secondaryNav } = useNavigation()
  return (
    <nav data-active={secondaryNav?.activeId}>
      {secondaryNav?.items.map(item => (
        <button key={item.id} data-id={item.id} onClick={() => secondaryNav.onChange(item.id)}>{item.label}</button>
      ))}
    </nav>
  )
}
function UncontrolledProbe() {
  const { activeId } = useSecondaryNav({ routePath: '/journal', items, defaultActiveId: 'all' })
  return <output data-filter>{activeId}</output>
}

let root: Root
beforeEach(() => { root = createRoot(document.getElementById('root')!) })
afterEach(async () => { await act(async () => root.unmount()) })

async function mount(url: string, pageKey = 'initial', uncontrolled = false) {
  await act(async () => {
    window.history.replaceState(null, '', url)
    window.dispatchEvent(new dom.window.PopStateEvent('popstate'))
    root.render(
      <StrictMode>
        <BrowserRouter>
          <NavigationProvider>
            {uncontrolled ? <UncontrolledProbe /> : <LibraryProbe key={pageKey} />}
            <NavProbe />
          </NavigationProvider>
        </BrowserRouter>
      </StrictMode>,
    )
  })
}
function expectFilter(filter: string) {
  assert.equal(document.querySelector('[data-filter]')?.textContent, filter)
  assert.equal(document.querySelector('nav')?.getAttribute('data-active'), filter)
}
async function select(id: string) {
  await act(async () => { document.querySelector<HTMLButtonElement>(`[data-id="${id}"]`)!.click() })
}
async function traverse(direction: 'back' | 'forward') {
  await act(async () => {
    await new Promise<void>(resolve => {
      window.addEventListener('popstate', () => resolve(), { once: true })
      window.history[direction]()
    })
  })
}

for (const filter of LIBRARY_FILTERS) {
  it(`opens the ${filter} deep link with matching content and navigation`, async () => {
    await mount(`/library?type=${filter}`)
    expectFilter(filter)
  })
}
for (const search of ['', '?type=', '?type=unknown', '?type=GAME']) {
  it(`defaults ${search || 'a bare URL'} to all`, async () => {
    await mount(`/library${search}`)
    expectFilter('all')
  })
}
it('updates the URL from the actual nav callback and preserves other query parameters and hash', async () => {
  await mount('/library?ref=shared#collection')
  await select('game')
  expectFilter('game')
  assert.equal(window.location.search, '?ref=shared&type=game')
  assert.equal(window.location.hash, '#collection')
  await select('book')
  expectFilter('book')
  assert.equal(window.location.search, '?ref=shared&type=book')
  await select('all')
  expectFilter('all')
  assert.equal(window.location.search, '?ref=shared')
  assert.equal(window.location.hash, '#collection')
})
it('restores both the filter and highlighted navigation on back and forward, without extra history entries', async () => {
  await mount('/library')
  const initialLength = window.history.length
  await select('game')
  await select('music')
  await select('music')
  assert.equal(window.history.length, initialLength + 2)
  await traverse('back')
  expectFilter('game')
  await traverse('back')
  expectFilter('all')
  assert.equal(window.location.search, '')
  await traverse('forward')
  expectFilter('game')
  await traverse('forward')
  expectFilter('music')
})
it('restores the chosen category after a reload', async () => {
  await mount('/library')
  await select('tv_series')
  const url = window.location.href
  await act(async () => root.unmount())
  root = createRoot(document.getElementById('root')!)
  await mount(url)
  expectFilter('tv_series')
})
it('uses the URL instead of a retained secondary nav selection when the page remounts', async () => {
  await mount('/library?type=game')
  await mount('/library', 'remounted')
  expectFilter('all')
  assert.equal(window.location.search, '')
})
it('keeps existing secondary nav users without URL control working', async () => {
  await mount('/journal', 'initial', true)
  expectFilter('all')
  await select('video')
  expectFilter('video')
  assert.equal(window.location.pathname, '/journal')
  assert.equal(window.location.search, '')
})
