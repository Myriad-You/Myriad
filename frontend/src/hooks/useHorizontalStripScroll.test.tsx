import type { Root } from 'react-dom/client'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { after, afterEach, beforeEach, it } from 'node:test'
import { act, createElement } from 'react'
import { useHorizontalStripScroll } from './useHorizontalStripScroll'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const dom = new JSDOM('<div id="root"></div>')
const prior = new Map<string, PropertyDescriptor | undefined>()
for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true })) {
  prior.set(key, Object.getOwnPropertyDescriptor(globalThis, key))
  Object.defineProperty(globalThis, key, { configurable: true, value })
}
// Load React DOM after the browser globals so its passive-event detection runs.
const { createRoot } = await import('react-dom/client')
after(() => {
  dom.window.close()
  for (const [key, descriptor] of prior) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor)
    else Reflect.deleteProperty(globalThis, key)
  }
})
let root: Root
beforeEach(() => { root = createRoot(document.getElementById('root')!) })
afterEach(async () => { await act(async () => root.unmount()) })

function Harness({ visible = true }: { visible?: boolean }) {
  const bind = useHorizontalStripScroll()
  const { isDragging: _, ...props } = bind
  return visible ? createElement('div', { ...props, 'data-strip': true }) : null
}
async function mount(visible = true) {
  await act(async () => root.render(createElement(Harness, { visible })))
  const el = document.querySelector<HTMLDivElement>('[data-strip]')
  if (el) Object.defineProperties(el, { scrollWidth: { configurable: true, value: 800 }, clientWidth: { configurable: true, value: 300 } })
  return el!
}
function wheel(el: HTMLDivElement, options: WheelEventInit = {}, at?: number) {
  const event = new dom.window.WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: 60, ...options })
  if (at !== undefined) Object.defineProperty(event, 'timeStamp', { value: at })
  el.dispatchEvent(event)
  return event
}

it('cancels the native wheel event when converting vertical motion to horizontal scroll', async () => {
  const el = await mount()
  assert.equal(wheel(el).defaultPrevented, true)
  assert.equal(el.scrollLeft, 60)
})

it('lets the page scroll at strip boundaries and when there is no overflow', async () => {
  const el = await mount()
  assert.equal(wheel(el, { deltaY: -60 }).defaultPrevented, false)
  el.scrollLeft = 500
  assert.equal(wheel(el).defaultPrevented, false)
  Object.defineProperty(el, 'scrollWidth', { value: 300 })
  el.scrollLeft = 0
  assert.equal(wheel(el).defaultPrevented, false)
})

it('preserves trackpad horizontal gestures and browser zoom', async () => {
  const el = await mount()
  assert.equal(wheel(el, { deltaX: 20 }).defaultPrevented, false)
  assert.equal(wheel(el, { ctrlKey: true }).defaultPrevented, false)
  assert.equal(el.scrollLeft, 0)
})

it('normalizes line and page deltas and clamps to the strip extent', async () => {
  const el = await mount()
  wheel(el, { deltaY: 3, deltaMode: 1 })
  assert.equal(el.scrollLeft, 48)
  wheel(el, { deltaY: 1, deltaMode: 2 })
  assert.equal(el.scrollLeft, 348)
  wheel(el, { deltaY: 1, deltaMode: 2 })
  assert.equal(el.scrollLeft, 500)
})

it('attaches after conditional mounting and removes listeners from replaced strips', async () => {
  await mount(false)
  const el = await mount()
  assert.equal(wheel(el).defaultPrevented, true)
  await mount(false)
  assert.equal(wheel(el).defaultPrevented, false)
  assert.equal(el.scrollLeft, 60)
  const replacement = await mount()
  assert.equal(wheel(replacement).defaultPrevented, true)
})

/** 四张 180 宽的卡，间距 20：吸附点 0/200/400/500（最后一张受 maxScrollLeft=500 截断）。 */
function withSnapCards(el: HTMLDivElement) {
  el.replaceChildren()
  for (let i = 0; i < 4; i++) {
    const card = document.createElement('div')
    card.dataset.snap = 'start'
    card.getBoundingClientRect = () => {
      const left = i * 200 - el.scrollLeft
      return { left, right: left + 180, width: 180, top: 0, bottom: 100, height: 100, x: left, y: 0, toJSON: () => ({}) } as DOMRect
    }
    el.append(card)
  }
  const calls: ScrollToOptions[] = []
  el.scrollTo = ((options: ScrollToOptions) => { calls.push(options) }) as typeof el.scrollTo
  const getComputedStyle = dom.window.getComputedStyle
  dom.window.getComputedStyle = ((node: Element) => ({
    ...getComputedStyle(node),
    scrollSnapType: 'x mandatory',
    scrollSnapAlign: (node as HTMLElement).dataset?.snap ?? 'none',
    scrollPaddingLeft: '0px',
    scrollPaddingRight: '0px',
  })) as typeof getComputedStyle
  return { calls, restore: () => { dom.window.getComputedStyle = getComputedStyle } }
}

it('scrolls exactly to the next snap point instead of relying on scrollBy', async () => {
  // #620：Firefox 把 scrollBy 一格滚轮的终点按最近吸附点处理，弹回原卡。
  const el = await mount()
  const { calls, restore } = withSnapCards(el)
  try {
    const first = wheel(el, { deltaY: 100 }, 1000)
    assert.equal(first.defaultPrevented, true)
    assert.deepEqual(calls, [{ left: 200, behavior: 'smooth' }])
    assert.equal(el.scrollLeft, 0)
    // Trackpad inertia (decaying deltas, over a second long) is swallowed, not paged again
    // and not turned into page scroll halfway through.
    for (let i = 1; i <= 70; i++) {
      assert.equal(wheel(el, { deltaY: Math.max(1, 100 - i) }, 1000 + i * 16).defaultPrevented, true)
    }
    assert.equal(calls.length, 1)
    // A new gesture after a pause steps again; at the leading edge the page keeps the wheel.
    assert.equal(wheel(el, { deltaY: -100 }, 3000).defaultPrevented, false)
    assert.equal(wheel(el, { deltaY: 100 }, 3400).defaultPrevented, true)
    assert.deepEqual(calls.at(-1), { left: 200, behavior: 'smooth' })
    // A mouse spun continuously (equal notches) keeps stepping, one card per notch,
    // each from where the previous smooth scroll is headed, not from where it is so far.
    wheel(el, { deltaY: 100 }, 3560)
    wheel(el, { deltaY: 100 }, 3720)
    assert.deepEqual(calls.slice(-2).map((c) => c.left), [400, 500])
    // Reversing mid-flight steps back from the pending target.
    el.scrollLeft = 350
    wheel(el, { deltaY: -100 }, 3880)
    assert.equal(calls.at(-1)?.left, 400)
    // At the far edge the wheel goes to the page even mid-gesture: no scroll trap.
    el.scrollLeft = 500
    assert.equal(wheel(el, { deltaY: 100 }, 4100).defaultPrevented, false)
  } finally {
    restore()
  }
})

it('steps from the current position after the previous scroll has settled', async () => {
  const el = await mount()
  const { calls, restore } = withSnapCards(el)
  try {
    el.scrollLeft = 400
    wheel(el, { deltaY: -100 }, 1000)
    assert.equal(calls.at(-1)?.left, 200)
    // 停了很久再滚：以实际位置为准（比如中间被拖动过）。
    el.scrollLeft = 200
    wheel(el, { deltaY: 100 }, 5000)
    assert.equal(calls.at(-1)?.left, 400)
  } finally {
    restore()
  }
})

it('shows the grab cursor only when the strip overflows', async () => {
  const el = await mount()
  await act(async () => { el.dispatchEvent(new dom.window.PointerEvent('pointerover', { bubbles: true })) })
  assert.match(el.className, /cursor-grab/)
  Object.defineProperty(el, 'scrollWidth', { value: 300 })
  await act(async () => { el.dispatchEvent(new dom.window.PointerEvent('pointerover', { bubbles: true })) })
  assert.doesNotMatch(el.className, /cursor-grab/)
})
