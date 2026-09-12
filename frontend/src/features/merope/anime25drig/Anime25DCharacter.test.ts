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

test('outfit and callback changes keep the React player; stale loads cannot report ready or failure', async () => {
  const dom = new JSDOM('<div id="root"></div>', { pretendToBeVisual: true })
  const globals = {
    window: dom.window,
    document: dom.window.document,
    ResizeObserver: class { observe() {} disconnect() {} },
    IS_REACT_ACT_ENVIRONMENT: true,
  }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  const packages: { atlas: string; resolve: () => void; reject: (error: Error) => void }[] = []
  let created = 0
  let disposed = 0
  class Player {
    constructor() { created += 1 }
    replaceLivePackage(_playback: unknown, _manifest: unknown, atlas: string) {
      return new Promise<void>((resolve, reject) => packages.push({ atlas, resolve, reject }))
    }

    setSpeechActive() {}
    setSpeechProsody() {}
    setSinging() {}
    setSingingTrack() {}
    setMusicSignal() {}
    setBearing() {}
    resize() {}
    tick() {}
    dispose() { disposed += 1 }
  }
  // Run the real React effects, replacing only the GPU/interaction boundaries.
  const bundle = await build({
    entryPoints: [new URL('./Anime25DCharacter.tsx', import.meta.url).pathname],
    bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{
      name: 'character-test-boundaries',
      setup(builder) {
        builder.onResolve({ filter: /^\.\/player$/ }, () => ({ path: 'player', namespace: 'test' }))
        builder.onResolve({ filter: /^\.\.\/(interaction\/(bindTouch|touchAppraisalHost)|motion\/runtimeHost)$/ }, () => ({ path: 'interaction', namespace: 'test' }))
        builder.onLoad({ filter: /.*/, namespace: 'test' }, ({ path }) => ({
          contents: path === 'player'
            ? 'export const Anime25DPlayer = TestPlayer'
            : 'export function bindCharacterTouch() {} export function createTouchAppraisal() {} export function getProductionMotionRuntime() {}',
          loader: 'js',
        }))
      },
    }],
  })
  const module = { exports: {} as { default: React.ComponentType<any> } }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports', 'TestPlayer'])(require, module, module.exports, Player)
  const Character = module.exports.default
  const root = createRoot(dom.window.document.getElementById('root'))
  const manifest = {}
  const playback = {}
  let ready = 0
  const errors: string[] = []
  const render = (atlas: string, label: string) => root.render(createElement(Character, {
    activity: 'idle', mood: 0, manualControl: true, manifest, playback, atlasUrl: atlas,
    onPlaybackReady: () => { ready += 1 },
    onPlaybackError: () => { errors.push(label) },
  }))
  try {
    await act(async () => render('first', 'first'))
    await act(async () => render('second', 'second'))
    assert.equal(created, 1)
    assert.equal(disposed, 0)
    assert.deepEqual(packages.map(item => item.atlas), ['first', 'second'])
    await act(async () => packages[0]!.reject(new Error('stale first outfit')))
    assert.deepEqual(errors, [])
    assert.equal(ready, 0)
    await act(async () => packages[1]!.resolve())
    assert.equal(ready, 1)
    await act(async () => render('second', 'new callback'))
    assert.equal(packages.length, 2, 'callback-only rerender must not reload the atlas')
    await act(async () => render('third', 'third'))
    await act(async () => packages[2]!.reject(new Error('bad outfit')))
    assert.equal(created, 1, 'a failed hot swap keeps the previous live outfit')
    assert.equal(disposed, 0)
    assert.deepEqual(errors, [])
    // Genuine GPU loss still recreates the player, with a bounded retry budget.
    for (let i = 0; i < 3; i++) {
      await act(async () => {
        dom.window.document.querySelector('canvas').dispatchEvent(new dom.window.Event('webglcontextlost', { cancelable: true }))
      })
    }
    assert.equal(created, 3)
    assert.equal(disposed, 2)
    assert.deepEqual(errors, ['third'], 'terminal GPU errors call the latest owner')
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
  assert.equal(disposed, created)
})
