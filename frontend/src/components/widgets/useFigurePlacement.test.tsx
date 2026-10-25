import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { test } from 'node:test'
import { act, createElement, Fragment, useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))

test('the one figure moves between card and stage without remounting, leaving a still frame behind', async () => {
  const dom = new JSDOM('<div id="root"></div><div id="stage"></div>')
  const globals = { window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value })
  const { createRoot } = await import('react-dom/client')
  const { useFigurePlacement } = await import('./useFigurePlacement')
  const stageModule = await import('../../features/merope/presence/figureStage')
  stageModule.resetFigureStageForTests()

  let mounts = 0
  let placement: { container: HTMLElement; onStage: boolean } | null = null
  function Figure() {
    useEffect(() => {
      mounts++
    }, [])
    return createElement('canvas', { id: 'live' })
  }
  function Card() {
    const home = useRef<HTMLDivElement>(null)
    placement = useFigurePlacement('card-a', home)
    return createElement(
      Fragment,
      null,
      createElement('div', { ref: home, id: 'home' }),
      createPortal(createElement(Figure), placement.container),
    )
  }

  const root = createRoot(dom.window.document.getElementById('root'))
  const stage = dom.window.document.getElementById('stage') as HTMLElement
  try {
    await act(async () => root.render(createElement(Card)))
    const home = dom.window.document.getElementById('home')
    const container = placement!.container
    assert.equal(container.parentElement, home)

    // Another card's stage is not ours.
    stageModule.showFigureOnStage('card-b', stage)
    assert.equal(container.parentElement, home)

    // Lending the stage moves the same node synchronously, before React renders.
    stageModule.showFigureOnStage('card-a', stage)
    assert.equal(container.parentElement, stage)
    await act(async () => {})
    assert.equal(placement!.onStage, true)

    // Giving it back moves it home at once and leaves a still copy on the stage.
    stageModule.releaseFigureStage('card-a', stage)
    assert.equal(container.parentElement, home)
    const hold = stage.querySelector('[data-figure-hold]')
    assert.ok(hold, 'the stage keeps a frame while the popup lands')
    assert.notEqual(hold, container)
    assert.equal(hold?.getAttribute('aria-hidden'), 'true')

    // Coming back clears that frame.
    stageModule.showFigureOnStage('card-a', stage)
    assert.equal(container.parentElement, stage)
    assert.equal(stage.querySelector('[data-figure-hold]'), null)
    assert.equal(placement!.container, container)
    assert.equal(container.querySelector('#live'), dom.window.document.querySelector('#stage > .merope-widget__figure #live'))
    assert.equal(mounts, 1, 'the figure is never remounted')
  } finally {
    await act(async () => root.unmount())
    stageModule.resetFigureStageForTests()
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else delete (globalThis as Record<string, unknown>)[key]
    }
  }
})
