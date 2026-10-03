import type { Root } from 'react-dom/client'
import type { WidgetRotationOptions } from './useWidgetRotation'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { after, afterEach, beforeEach, describe, it, mock } from 'node:test'
import { act, createElement } from 'react'
import {
  horizontalWheelDelta,
  ROTATION_HOLD_MS,
  swipeIntent,
  useWidgetRotation,
} from './useWidgetRotation'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const dom = new JSDOM('<div id="root"></div>', { pretendToBeVisual: true })
const prior = new Map<string, PropertyDescriptor | undefined>()
for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true })) {
  prior.set(key, Object.getOwnPropertyDescriptor(globalThis, key))
  Object.defineProperty(globalThis, key, { configurable: true, value })
}
// Load React DOM after the browser globals so its event system binds to jsdom.
const { createRoot } = await import('react-dom/client')
after(() => {
  dom.window.close()
  for (const [key, descriptor] of prior) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor)
    else Reflect.deleteProperty(globalThis, key)
  }
})

let root: Root
let steps: number[]
let latest: ReturnType<typeof useWidgetRotation>
beforeEach(() => {
  steps = []
  root = createRoot(document.getElementById('root')!)
})
afterEach(async () => {
  await act(async () => root.unmount())
  mock.timers.reset()
})

function Harness(props: Partial<WidgetRotationOptions>) {
  latest = useWidgetRotation({
    count: 3,
    interactive: true,
    delay: null,
    autoplay: false,
    onStep: (delta) => steps.push(delta),
    ...props,
  })
  return createElement(
    'div',
    { 'ref': latest.rootRef, ...latest.rootProps, 'data-root': true },
    createElement('button', { type: 'button', onClick: () => steps.push(0) }, 'open'),
    createElement('input', { 'data-input': true }),
  )
}
async function mount(props: Partial<WidgetRotationOptions> = {}) {
  await act(async () => root.render(createElement(Harness, props)))
  return document.querySelector<HTMLElement>('[data-root]')!
}
function pointer(el: Element, type: string, init: PointerEventInit) {
  el.dispatchEvent(new dom.window.PointerEvent(type, { bubbles: true, cancelable: true, isPrimary: true, pointerId: 1, button: 0, ...init }))
}
async function swipe(el: Element, dx: number, dy = 0, pointerType = 'touch') {
  await act(async () => {
    pointer(el, 'pointerdown', { clientX: 100, clientY: 100, pointerType })
    pointer(el, 'pointermove', { clientX: 100 + dx / 2, clientY: 100 + dy / 2, pointerType })
    pointer(el, 'pointermove', { clientX: 100 + dx, clientY: 100 + dy, pointerType })
    pointer(el, 'pointerup', { clientX: 100 + dx, clientY: 100 + dy, pointerType })
  })
}

describe('swipeIntent', () => {
  it('waits, then calls a horizontal swipe; left is next', () => {
    assert.equal(swipeIntent(-10, 2), 'pending')
    assert.equal(swipeIntent(-40, 5), 'next')
    assert.equal(swipeIntent(40, -5), 'prev')
  })

  it('hands a mostly vertical move to page scrolling', () => {
    assert.equal(swipeIntent(5, 14), 'scroll')
    assert.equal(swipeIntent(40, 34), 'pending')
  })
})

describe('horizontalWheelDelta', () => {
  const wheel = { deltaX: 0, deltaY: 0, deltaMode: 0, shiftKey: false }

  it('takes horizontal trackpad motion and ignores vertical scrolling', () => {
    assert.equal(horizontalWheelDelta({ ...wheel, deltaX: 30, deltaY: 4 }), 30)
    assert.equal(horizontalWheelDelta({ ...wheel, deltaX: 4, deltaY: 30 }), 0)
  })

  it('treats Shift + mouse wheel as horizontal and normalizes line deltas', () => {
    assert.equal(horizontalWheelDelta({ ...wheel, deltaY: 3, deltaMode: 1, shiftKey: true }), 48)
  })
})

describe('useWidgetRotation', () => {
  it('advances on its own and pauses while a mouse hovers', async () => {
    mock.timers.enable({ apis: ['setTimeout'] })
    const el = await mount({ delay: 1000, autoplay: true })
    await act(async () => mock.timers.tick(1000))
    assert.deepEqual(steps, [1])
    await act(async () => pointer(el, 'pointerover', { pointerType: 'mouse' }))
    assert.equal(latest.paused, true)
    await act(async () => mock.timers.tick(5000))
    assert.deepEqual(steps, [1])
    await act(async () => pointer(el, 'pointerout', { pointerType: 'mouse', relatedTarget: document.body }))
    await act(async () => mock.timers.tick(1000))
    assert.deepEqual(steps, [1, 1])
  })

  it('does not let a touch-synthesized enter pause it forever', async () => {
    const el = await mount()
    await act(async () => pointer(el, 'pointerover', { pointerType: 'touch' }))
    assert.equal(latest.paused, false)
  })

  it('steps on a horizontal swipe and swallows the click that follows', async () => {
    // 松手后的 click 与 pointerup 属于同一轮事件；act 的异步刷新不能先跑清理计时器。
    mock.timers.enable({ apis: ['setTimeout'] })
    const el = await mount()
    await swipe(el, -60)
    assert.deepEqual(steps, [1])
    assert.equal(latest.paused, true, 'a swipe restarts the autoplay countdown after the hold')
    await act(async () => el.querySelector('button')!.click())
    assert.deepEqual(steps, [1], 'the swipe release must not open the card')
    await act(async () => el.querySelector('button')!.click())
    assert.deepEqual(steps, [1, 0], 'a later plain click still works')
    await swipe(el, 60, 0, 'mouse')
    assert.deepEqual(steps, [1, 0, -1])
  })

  it('leaves vertical drags, form controls, and disabled widgets alone', async () => {
    let el = await mount()
    await swipe(el, -20, 60)
    await swipe(el.querySelector('[data-input]')!, -60)
    assert.deepEqual(steps, [])
    el = await mount({ interactive: false })
    await swipe(el, -60)
    assert.deepEqual(steps, [])
    el = await mount({ count: 1 })
    await swipe(el, -60)
    assert.deepEqual(steps, [])
  })

  function wheelAt(el: Element, at: number, init: WheelEventInit) {
    const event = new dom.window.WheelEvent('wheel', { bubbles: true, cancelable: true, ...init })
    Object.defineProperty(event, 'timeStamp', { value: at })
    el.dispatchEvent(event)
    return event
  }

  it('lets vertical wheel scroll the page and pages on a horizontal sweep', async () => {
    const el = await mount()
    await act(async () => {
      assert.equal(wheelAt(el, 1000, { deltaY: 80 }).defaultPrevented, false)
      assert.equal(wheelAt(el, 1016, { deltaX: 30 }).defaultPrevented, true)
      wheelAt(el, 1032, { deltaX: 30 })
    })
    assert.deepEqual(steps, [1])
  })

  it('turns one trackpad sweep, inertia included, into exactly one page', async () => {
    const el = await mount()
    await act(async () => {
      // 一秒多的惯性尾巴，每帧都还在攒位移。
      for (let i = 0; i < 80; i++) wheelAt(el, 2000 + i * 16, { deltaX: Math.max(1, 60 - i) })
    })
    assert.deepEqual(steps, [1])
    await act(async () => {
      wheelAt(el, 2000 + 79 * 16 + 300, { deltaX: -60 })
    })
    assert.deepEqual(steps, [1, -1], 'a new sweep after a pause pages again')
  })

  it('does not autoplay while a finger is down, so a slow swipe cannot land two pages', async () => {
    mock.timers.enable({ apis: ['setTimeout'] })
    const el = await mount({ delay: 1000, autoplay: true })
    await act(async () => pointer(el, 'pointerdown', { clientX: 100, clientY: 100, pointerType: 'touch' }))
    await act(async () => mock.timers.tick(5000))
    assert.deepEqual(steps, [])
    await act(async () => pointer(el, 'pointermove', { clientX: 40, clientY: 100, pointerType: 'touch' }))
    assert.deepEqual(steps, [1])
    await act(async () => {
      window.dispatchEvent(new dom.window.PointerEvent('pointerup', { pointerId: 1, pointerType: 'touch' }))
    })
    assert.equal(latest.paused, true, 'still held right after the swipe')
    await act(async () => mock.timers.tick(ROTATION_HOLD_MS))
    assert.equal(latest.paused, false, 'released finger and expired hold both clear the pause')
    await act(async () => mock.timers.tick(1000))
    assert.deepEqual(steps, [1, 1], 'after the hold it resumes')
  })

  it('lets only the innermost rotating widget take a gesture', async () => {
    const outer: number[] = []
    function Inner() {
      const rotation = useWidgetRotation({ count: 3, interactive: true, delay: null, autoplay: false, onStep: (d) => steps.push(d) })
      return createElement('div', { 'ref': rotation.rootRef, ...rotation.rootProps, 'data-inner': true })
    }
    function Outer() {
      const rotation = useWidgetRotation({ count: 3, interactive: true, delay: null, autoplay: false, onStep: (d) => outer.push(d) })
      return createElement('div', { ref: rotation.rootRef, ...rotation.rootProps }, createElement(Inner))
    }
    await act(async () => root.render(createElement(Outer)))
    const inner = document.querySelector('[data-inner]')!
    await swipe(inner, -60)
    await act(async () => {
      wheelAt(inner, 5000, { deltaX: 80 })
    })
    assert.deepEqual(steps, [1, 1])
    assert.deepEqual(outer, [], 'the panel around it must not page too')
  })

  it('pages with arrow keys and holds the autoplay after any manual step', async () => {
    mock.timers.enable({ apis: ['setTimeout'] })
    const el = await mount({ delay: 1000, autoplay: true })
    await act(async () => {
      el.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true }))
    })
    assert.deepEqual(steps, [-1])
    assert.equal(latest.paused, true)
    assert.equal(latest.showPager, true)
    await act(async () => mock.timers.tick(ROTATION_HOLD_MS - 1))
    assert.deepEqual(steps, [-1])
    await act(async () => mock.timers.tick(1))
    assert.equal(latest.paused, false)
    await act(async () => mock.timers.tick(1000))
    assert.deepEqual(steps, [-1, 1])
  })

  it('pages with arrow keys even when focus sits on the page dots', async () => {
    function WithDots() {
      latest = useWidgetRotation({ count: 3, interactive: true, delay: null, autoplay: false, onStep: (d) => steps.push(d) })
      return createElement('div', { ref: latest.rootRef, ...latest.rootProps }, createElement('div', { 'data-rotation-ignore': '' }, createElement('button', { 'type': 'button', 'data-dot': true })))
    }
    await act(async () => root.render(createElement(WithDots)))
    const dot = document.querySelector('[data-dot]')!
    await act(async () => {
      dot.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }))
    })
    assert.deepEqual(steps, [1])
  })

  for (const compact of [false, true]) {
    it(`restores lost focus to ${compact ? 'a compact pager button' : 'the current dot'} after replacing content`, async () => {
      function WithPager({ replaced = false }: { replaced?: boolean }) {
        latest = useWidgetRotation({ count: 3, interactive: true, delay: null, autoplay: false, onStep: (d) => steps.push(d) })
        return createElement('div', { ref: latest.rootRef, ...latest.rootProps },
          replaced ? null : createElement('button', { 'data-content': '' }, 'content'),
          createElement('div', { role: 'group' },
            createElement('button', { 'data-first': '' }, 'previous'),
            compact
              ? createElement('span', { 'aria-current': 'page' }, '2 / 3')
              : createElement('button', { 'data-pager-dot': '', 'aria-current': 'true' }, 'current'),
          ),
        )
      }
      const raf = window.requestAnimationFrame
      const frames: FrameRequestCallback[] = []
      window.requestAnimationFrame = (callback) => { frames.push(callback); return frames.length }
      mock.timers.enable({ apis: ['setTimeout'] })
      try {
        await act(async () => root.render(createElement(WithPager)))
        await act(async () => document.querySelector<HTMLElement>('[data-content]')!.focus())
        await act(async () => latest.step(1))
        await act(async () => root.render(createElement(WithPager, { replaced: true })))
        assert.equal(document.activeElement, document.body)
        await act(async () => frames.forEach(callback => callback(0)))
        assert.equal(document.activeElement, document.querySelector(compact ? '[data-first]' : '[data-pager-dot]'))
      } finally {
        window.requestAnimationFrame = raf
      }
    })
  }

  it('leaves wheel in an ignored strip to native scrolling, and keeps the outer panel still', async () => {
    const outer: number[] = []
    function Inner() {
      const rotation = useWidgetRotation({ count: 3, interactive: true, delay: null, autoplay: false, onStep: (d) => steps.push(d) })
      return createElement('div', { ref: rotation.rootRef, ...rotation.rootProps }, createElement('div', { 'data-rotation-ignore': '', 'data-strip': true }))
    }
    function Outer() {
      const rotation = useWidgetRotation({ count: 3, interactive: true, delay: null, autoplay: false, onStep: (d) => outer.push(d) })
      return createElement('div', { ref: rotation.rootRef, ...rotation.rootProps }, createElement(Inner))
    }
    await act(async () => root.render(createElement(Outer)))
    const strip = document.querySelector<HTMLElement>('[data-strip]')!
    // jsdom 不排版：手动给出「还能往右横滚」的几何。
    Object.defineProperties(strip, { scrollWidth: { configurable: true, value: 400 }, clientWidth: { configurable: true, value: 200 } })
    let prevented = true
    await act(async () => {
      prevented = wheelAt(strip, 9000, { deltaX: 90 }).defaultPrevented
    })
    assert.equal(prevented, false, 'the strip scrolls natively')
    await swipe(strip, -60)
    assert.deepEqual(steps, [])
    assert.deepEqual(outer, [], 'a drag inside the inner widget never pages the panel around it')
  })

  it('counts a mouse that was already over the card as hovering once it becomes pageable', async () => {
    const el = await mount({ count: 1 })
    await act(async () => pointer(el, 'pointerover', { pointerType: 'mouse' }))
    await mount({ count: 3 })
    assert.equal(latest.paused, true)
  })

  it('keeps autoplay off once the user stops it, until they turn it back on', async () => {
    mock.timers.enable({ apis: ['setTimeout'] })
    await mount({ delay: 1000, autoplay: true })
    await act(async () => latest.toggleStopped())
    await act(async () => mock.timers.tick(60_000))
    assert.deepEqual(steps, [])
    await act(async () => latest.toggleStopped())
    await act(async () => mock.timers.tick(1000))
    assert.deepEqual(steps, [1])
  })

  it('drops a stale hover when the root element goes away', async () => {
    const el = await mount()
    await act(async () => pointer(el, 'pointerover', { pointerType: 'mouse' }))
    assert.equal(latest.paused, true)
    await act(async () => {
      latest.rootRef(null)
      await Promise.resolve()
    })
    assert.equal(latest.paused, false)
  })

  it('pages on a horizontal sweep over the page dots (they do not scroll)', async () => {
    function WithDots() {
      latest = useWidgetRotation({ count: 3, interactive: true, delay: null, autoplay: false, onStep: (d) => steps.push(d) })
      return createElement('div', { ref: latest.rootRef, ...latest.rootProps }, createElement('div', { 'data-rotation-ignore': '', 'data-dots': true }))
    }
    await act(async () => root.render(createElement(WithDots)))
    let prevented = false
    await act(async () => {
      prevented = wheelAt(document.querySelector('[data-dots]')!, 12_000, { deltaX: 80 }).defaultPrevented
    })
    assert.equal(prevented, true)
    assert.deepEqual(steps, [1])
  })

  it('blocks native image drag so a mouse swipe over a cover still pages', async () => {
    const el = await mount()
    const event = new dom.window.MouseEvent('dragstart', { bubbles: true, cancelable: true })
    el.dispatchEvent(event)
    assert.equal(event.defaultPrevented, true)
  })

  it('forgets the swallowed click when no click follows a swipe', async () => {
    mock.timers.enable({ apis: ['setTimeout'] })
    const el = await mount()
    await act(async () => {
      pointer(el, 'pointerdown', { clientX: 100, clientY: 100, pointerType: 'touch' })
      pointer(el, 'pointermove', { clientX: 40, clientY: 100, pointerType: 'touch' })
      window.dispatchEvent(new dom.window.PointerEvent('pointerup', { pointerId: 1, pointerType: 'touch' }))
      mock.timers.tick(1)
    })
    assert.deepEqual(steps, [1])
    await act(async () => el.querySelector('button')!.click())
    assert.deepEqual(steps, [1, 0], 'a later keyboard/plain activation is not eaten')
  })
})

describe('pagerDotCapacity', () => {
  it('fits dots to the width and falls back to a counter when they do not fit', async () => {
    const { pagerDotCapacity } = await import('./pagerCapacity')
    assert.equal(pagerDotCapacity(308) >= 11, true, 'a 4x2 card shows 11 dots')
    assert.equal(pagerDotCapacity(150) < 24, true, 'a 2x2 card cannot show 24 dots')
    assert.equal(pagerDotCapacity(10), 0)
  })
})
