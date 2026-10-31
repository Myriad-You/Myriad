import type { TappCodeStructure, TappInstance } from '@myriad/tapp-runtime'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import { buildLayerScript, generateThemeCSS, generateWidgetSDK, WIDGET_STATIC_CSS } from '@myriad/tapp-runtime'
import * as security from '@myriad/tapp-runtime'
import { createTestTappKernel } from '@myriad/tapp-runtime/testing'
import ts from 'typescript'

const require = createRequire(import.meta.url)
const { JSDOM, VirtualConsole } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
// Execute the private HTML generator without mounting the host iframe and its services.
const source = ts.createSourceFile('TappWidgetSandbox.tsx', readFileSync(new URL('./TappWidgetSandbox.tsx', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
const generator = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === 'generateWidgetHTML')!
const script = ts.transpile(generator.getText(source), { target: ts.ScriptTarget.ES2022 })
const dependencies = { ...security, buildLayerScript, generateWidgetSDK, generateThemeCSS, WIDGET_STATIC_CSS }
const generateHTML = compileFunction(`${script}; return generateWidgetHTML;`, Object.keys(dependencies))(...Object.values(dependencies))
const instance = {
  id: 'test.widget', manifest: { id: 'test.widget', name: 'Test', version: '1.0.0', category: 'utility', permissions: [] },
  status: 'running', grantedPermissions: [], userRole: 'guest', installedAt: '',
} as unknown as TappInstance
const documentSecurity = createTestTappKernel().kernel.documentSecurity(instance, 'session')

for (const outcome of ['resolve', 'reject', 'throw'] as const) {
  test(`widget readiness waits for first render (${outcome})`, async () => {
    const code: TappCodeStructure = {
      modules: { 'widget.js': `Tapp.widgets.card = { render: function(root) {
        ${outcome === 'throw' ? "throw new Error('render failed');" : `return new Promise(function(resolve, reject) {
          window.finish = function() { root.textContent = 'Rendered'; ${outcome === 'resolve' ? 'resolve()' : "reject(new Error('render failed'))"}; };
        });`}
      } };` },
      widgetEntries: { card: 'widget.js' },
    }
    const html = generateHTML(instance, code, 'card', { theme: 'light', locale: 'en-US' }, 'session', documentSecurity, { missing: 'Missing', renderFailed: 'Render failed' })
    const events: Array<{ action: string; content: string }> = []
    const dom = new JSDOM(html, {
      url: 'https://myriad.test/widget', runScripts: 'dangerously', virtualConsole: new VirtualConsole(),
      beforeParse(window) {
        Object.defineProperty(window, 'parent', { value: { postMessage(message) {
          events.push({ action: message.action, content: window.document.getElementById('widget-root')?.textContent ?? '' })
        } } })
      },
    })
    try {
      await new Promise(resolve => setImmediate(resolve))
      if (outcome !== 'throw') {
        assert.equal(events.filter(event => event.action === 'tapp.ready').length, 0)
        assert.equal(typeof dom.window.finish, 'function', 'the actual SDK and widget code must have booted')
        dom.window.finish()
        await new Promise(resolve => setImmediate(resolve))
      }
      assert.deepEqual(events.filter(event => event.action === 'tapp.ready'), [
        { action: 'tapp.ready', content: outcome === 'resolve' ? 'Rendered' : 'Render failed' },
      ])
    } finally { dom.window.close() }
  })
}

test('popup surface renders the popup layer with its params, not the card', async () => {
  const code: TappCodeStructure = {
    modules: {
      'widget.js': `Tapp.widgets.card = { render: function(root) { root.textContent = 'Card'; } };`,
      'popup.js': `Tapp.popups.card = { render: function(root, props) { root.textContent = 'Popup:' + props.params.view; } };`,
    },
    widgetEntries: { card: 'widget.js' },
    popupEntries: { card: 'popup.js' },
    widgetHtml: '<p>card template</p>',
  }
  const html = generateHTML(instance, code, 'card', { theme: 'light', locale: 'en-US' }, 'session', documentSecurity, { missing: 'Missing', renderFailed: 'Render failed' }, 'popup', { view: 'stats' })
  assert.doesNotMatch(html, /card template/, 'size templates belong to the card only')
  const events: Array<{ action: string; content: string }> = []
  const dom = new JSDOM(html, {
    url: 'https://myriad.test/popup', runScripts: 'dangerously', virtualConsole: new VirtualConsole(),
    beforeParse(window) {
      Object.defineProperty(window, 'parent', { value: { postMessage(message) {
        events.push({ action: message.action, content: window.document.getElementById('widget-root')?.textContent ?? '' })
      } } })
    },
  })
  try {
    await new Promise(resolve => setImmediate(resolve))
    assert.deepEqual(events.filter(event => event.action === 'tapp.ready'), [
      { action: 'tapp.ready', content: 'Popup:stats' },
    ])
    assert.equal(dom.window._TAPP_MODE, 'popup')
  } finally { dom.window.close() }
})
