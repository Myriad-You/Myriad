import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import React, { act } from 'react'
import { createRoot } from 'react-dom/client'
import ts from 'typescript'
import { beginTappSubjectChange, finishTappSubjectChange, getTappSubjectSnapshot, useTappSubject } from '../utils/tappSubject'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))

test('runtime import recovery binds events; identity replacement drops old results and rebinds subscriptions', async () => {
  const dom = new JSDOM('<div id="root"></div>')
  const globals = { window: dom.window, document: dom.window.document, CustomEvent: dom.window.CustomEvent, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { value, configurable: true })
  const source = ts.createSourceFile('hook.ts', readFileSync(new URL('./useTappWidgets.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true)
  const declaration = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === 'useTappWidgets')!
  const makeRuntime = (name: string) => {
    const listeners = new Map<string, () => void>()
    return { name, listeners, waitForSync: async () => {}, syncFromBackend: async () => {}, getRegisteredWidgets: () => [{ id: name }], on: (event: string, listener: () => void) => { listeners.set(event, listener); return () => listeners.delete(event) } }
  }
  const old = makeRuntime('old-owner')
  const fresh = makeRuntime('new-owner')
  let current = old
  let failImport = true
  let retry!: () => void
  const dependencies = { ...React, useTappSubject, getTappSubjectSnapshot, loadTappRuntimeModule: async () => {
    if (failImport) throw new Error('runtime import failed')
    return { getTappRuntime: () => current }
  }, setTimeout: (callback: () => void) => { retry = callback }, createTappWidgetType: (widget: unknown) => widget, currentCopy: () => ({ errors: {} }), formatUserFacingError: async () => '' }
  const hook = compileFunction(`${ts.transpile(declaration.getText(source).replace(/^export /, ''), { target: ts.ScriptTarget.ESNext })}; return useTappWidgets`, Object.keys(dependencies))(...Object.values(dependencies))
  let value: { tappWidgets: Array<{ id: string }>; refreshWidgets: () => Promise<void> }
  function Probe() { value = hook(); return null }
  const root = createRoot(document.getElementById('root')!)
  try {
    await act(async () => root.render(React.createElement(Probe)))
    assert.equal(old.listeners.size, 0)
    assert.deepEqual(value!.tappWidgets, [])
    await act(async () => { failImport = false; retry() })
    assert.equal(old.listeners.size, 3)
    assert.deepEqual(value!.tappWidgets.map(widget => widget.id), ['old-owner'])
    const held = Promise.withResolvers<void>()
    old.waitForSync = () => held.promise
    let stale!: Promise<void>
    await act(async () => { stale = value!.refreshWidgets() })
    let epoch = 0
    await act(async () => { epoch = beginTappSubjectChange() })
    assert.deepEqual(value!.tappWidgets, [])
    assert.equal(old.listeners.size, 0)
    await act(async () => { held.resolve(); await stale })
    assert.deepEqual(value!.tappWidgets, [], 'late old results cannot refill the new subject')
    current = fresh
    await act(async () => finishTappSubjectChange(epoch, true))
    assert.equal(fresh.listeners.size, 3)
    assert.deepEqual(value!.tappWidgets.map(widget => widget.id), ['new-owner'])
    await act(async () => { fresh.getRegisteredWidgets = () => [{ id: 'new-widget' }]; fresh.listeners.get('widget:registered')!() })
    assert.deepEqual(value!.tappWidgets.map(widget => widget.id), ['new-widget'])
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
