import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

test('persona skeleton covers slow portraits; timeout exposes a working retry', async () => {
  const dom = new JSDOM('<div id="root"></div>')
  dom.window.matchMedia = () => ({ matches: true })
  const globals = { window: dom.window, document: dom.window.document, HTMLImageElement: dom.window.HTMLImageElement, HTMLCanvasElement: dom.window.HTMLCanvasElement, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value })
  const timers = new Map<number, { callback: (...args: unknown[]) => void; args: unknown[]; delay: number }>()
  let id = 0
  dom.window.setTimeout = (callback, delay, ...args) => { timers.set(++id, { callback, delay, args }); return id }
  dom.window.clearTimeout = timer => timers.delete(timer)
  const advance = async (max: number) => {
    for (const [key, timer] of [...timers]) {
      if (timer.delay > max) continue
      timers.delete(key)
      await act(async () => timer.callback(...timer.args))
    }
  }
  let requests = 0
  const face = async () => { requests++; return { manifest: null, portraitUrl: '/portrait.png' } }
  // Keep widget, presence, skeleton, and static-image lifecycle real; isolate external stores and GPU.
  const boundaries: Record<string, string> = {
    I18nContext: `export const useI18n = () => ({t: {common: {loading: 'Loading', retry: 'Retry'}, widgets: {agentPersona: 'Persona'}, merope: {title: 'Persona', loadFailed: 'Load failed', assetEmpty: 'Empty'}}}); export function withI18nNamespace(_names, Component) { return Component }`,
    AuthContext: 'export const useAuth = () => ({hasChecked: false})',
    api: 'export const getSiteFace = TestFace',
    agent: 'export const agentService = {}',
    agentStatusStore: "export const useAgentStatusKind = () => 'idle'",
    useRigMotionLifecycle: 'export const useRigMotionLifecycle = () => {}',
    liveFacePlayback: 'export const useLiveFacePlayback = () => true; export const notifyLiveFaceUnmounted = () => {}; export const LIVE_FACE_PLAYBACK_PRIORITY = {widget: 1}',
    widgetFaceSlot: 'export const useMeropeWidgetFaceSlot = () => true',
    Anime25DCharacter: 'export default function Character() { return null }',
    publicName: "export const loadPublicPersonaName = async () => null; export const publicPersonaName = () => null; export const PERSONA_DEFAULT_NAME = ''; export const PERSONA_OFF_NAME = ''",
  }
  const bundle = await build({
    entryPoints: [new URL('./MeropeWidget.tsx', import.meta.url).pathname],
    bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external', loader: { '.css': 'empty' },
    plugins: [{ name: 'widget-boundaries', setup(builder) {
      builder.onResolve({ filter: /\// }, ({ path }) => {
        const key = path.split('/').at(-1)!
        if (key in boundaries) return { path: key, namespace: 'test' }
      })
      builder.onLoad({ filter: /.*/, namespace: 'test' }, ({ path }) => ({ contents: boundaries[path], loader: 'js' }))
    } }],
  })
  const module = { exports: {} as typeof import('./MeropeWidget') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports', 'TestFace'])(require, module, module.exports, face)
  const root = createRoot(dom.window.document.getElementById('root'))
  const props = { config: { id: 'persona', size: '4x4' } } as Parameters<typeof module.exports.MeropeWidget>[0]
  try {
    await act(async () => root.render(createElement(module.exports.MeropeWidget, props)))
    assert.equal(requests, 1)
    assert.ok(dom.window.document.querySelector('img'))
    assert.ok(dom.window.document.querySelector('.widget-skeleton-cover'))
    await advance(12_000)
    await advance(1000)
    assert.match(dom.window.document.body.textContent, /Load failed/)
    const retry = dom.window.document.querySelector('button')
    assert.ok(retry, 'a timed-out portrait must offer retry')
    await act(async () => retry.click())
    assert.equal(requests, 2)
    assert.ok(dom.window.document.querySelector('img'), 'retry remounts the portrait')
    assert.ok(dom.window.document.querySelector('.widget-skeleton-cover'))
    await act(async () => dom.window.document.querySelector('img').dispatchEvent(new dom.window.Event('load')))
    await advance(1000)
    assert.equal(dom.window.document.querySelector('.widget-skeleton-cover'), null)
    assert.doesNotMatch(dom.window.document.body.textContent, /Load failed/)
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
