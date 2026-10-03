import type { Root } from 'react-dom/client'
import type { WidgetPagerProps } from './WidgetPager'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { after, afterEach, beforeEach, it } from 'node:test'
import { compileFunction } from 'node:vm'
import { act, createElement } from 'react'
import ts from 'typescript'
import { pagerDotCapacity, TOGGLE_PX } from './pagerCapacity'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const dom = new JSDOM('<div id="root"></div>')
const prior = new Map<string, PropertyDescriptor | undefined>()
for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true })) {
  prior.set(key, Object.getOwnPropertyDescriptor(globalThis, key))
  Object.defineProperty(globalThis, key, { configurable: true, value })
}
const { createRoot } = await import('react-dom/client')
// 只替换图标和文案边界；运行真正的页码组件、React effect 和容量计算。
const code = ts.transpileModule(readFileSync(new URL('./WidgetPager.tsx', import.meta.url), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.ReactJSX },
}).outputText
const module = { exports: {} as typeof import('./WidgetPager') }
compileFunction(code, ['require', 'module', 'exports'])(
  (id: string) => {
    if (id === '@lib/icons') {
      return { LuChevronLeft: () => null, LuChevronRight: () => null, LuPause: () => null, LuPlay: () => null }
    }
    if (id.endsWith('/I18nContext')) {
      return {
        useI18n: () => ({
          t: { widgetGrid: { pagerLabel: 'Pages', prevPage: 'Previous', nextPage: 'Next', goToPage: 'Page {page} of {total}', pauseRotation: 'Pause', resumeRotation: 'Resume' } },
          format: (template: string, params: Record<string, number>) => template.replace(/\{(\w+)\}/g, (_, key) => String(params[key])),
        }),
      }
    }
    if (id === './pagerCapacity') return { pagerDotCapacity, TOGGLE_PX }
    return require(id)
  }, module, module.exports,
)
const { WidgetPager } = module.exports
let root: Root
let host: HTMLElement
beforeEach(() => {
  host = document.getElementById('root')!
  Object.defineProperty(host, 'clientWidth', { configurable: true, value: 300 })
  root = createRoot(host)
})
afterEach(async () => { await act(async () => root.unmount()) })
after(() => {
  dom.window.close()
  for (const [key, descriptor] of prior) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor)
    else Reflect.deleteProperty(globalThis, key)
  }
})

async function render(props: Partial<WidgetPagerProps> = {}) {
  await act(async () => root.render(createElement(WidgetPager, { count: 3, index: 0, visible: true, onSelect: () => {}, ...props })))
}

it('moves keyboard focus to the new current dot and keeps only that dot in the Tab order', async () => {
  await render()
  const dots = Array.from(host.querySelectorAll<HTMLButtonElement>('[data-pager-dot]'))
  dots[0].focus()
  await render({ index: 2 })
  assert.equal(document.activeElement, dots[2])
  assert.deepEqual(dots.map(dot => dot.tabIndex), [-1, -1, 0])
})

it('leaves focus on the pause button while the page changes and exposes resume when stopped', async () => {
  let toggles = 0
  const onToggleStopped = () => { toggles++ }
  await render({ onToggleStopped })
  const pause = host.querySelector<HTMLButtonElement>('[aria-label="Pause"]')!
  pause.focus()
  await act(async () => pause.click())
  assert.equal(toggles, 1)
  await render({ onToggleStopped, stopped: true, index: 1 })
  assert.equal(document.activeElement, pause)
  assert.equal(pause.getAttribute('aria-label'), 'Resume')
  assert.equal(pause.getAttribute('aria-pressed'), 'true')
})

it('reserves pause button space before choosing dots or the compact counter', async () => {
  Object.defineProperty(host, 'clientWidth', { configurable: true, value: 65 })
  await render()
  assert.equal(host.querySelectorAll('[data-pager-dot]').length, 3)
  await render({ onToggleStopped: () => {} })
  assert.equal(host.querySelectorAll('[data-pager-dot]').length, 0)
  assert.equal(host.querySelector('[aria-current="page"]')?.textContent, '1 / 3')
})
