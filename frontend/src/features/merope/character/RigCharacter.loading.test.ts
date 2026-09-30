import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', {
  paths: [require.resolve('isomorphic-dompurify')],
}))
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

test('static portraits become ready only after loading and report broken images', async () => {
  const dom = new JSDOM('<div id="root"></div>')
  const globals = { window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  const bundle = await build({
    entryPoints: [new URL('./RigCharacter.tsx', import.meta.url).pathname],
    bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'gpu-boundary', setup(builder) {
      builder.onResolve({ filter: /\/Anime25DCharacter$/ }, () => ({ path: 'gpu', namespace: 'test' }))
      builder.onLoad({ filter: /.*/, namespace: 'test' }, () => ({ contents: 'export default function Character() { return null }', loader: 'js' }))
    } }],
  })
  const module = { exports: {} as typeof import('./RigCharacter') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports'])(require, module, module.exports)
  let ready = 0
  let errors = 0
  const root = createRoot(dom.window.document.getElementById('root'))
  const render = (url: string) => root.render(createElement(module.exports.default, {
    activity: 'idle', mood: 0, manifest: null, fallbackUrl: url,
    onPlaybackReady: () => { ready++ }, onPlaybackError: () => { errors++ },
  }))
  try {
    await act(async () => render('/portrait.png'))
    assert.equal(ready, 0, 'mounting an image is not readiness')
    assert.equal(errors, 0)
    await act(async () => dom.window.document.querySelector('img').dispatchEvent(new dom.window.Event('load')))
    assert.equal(ready, 1)
    await act(async () => render('/broken.png'))
    assert.equal(ready, 1, 'a new URL must wait for its own load')
    await act(async () => dom.window.document.querySelector('img').dispatchEvent(new dom.window.Event('error')))
    assert.equal(errors, 1)
    Object.defineProperties(dom.window.HTMLImageElement.prototype, {
      complete: { configurable: true, get: () => true },
      naturalWidth: { configurable: true, get: () => 100 },
    })
    await act(async () => render('/cached.png'))
    assert.equal(ready, 2, 'already-loaded cached images are immediately ready')
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
