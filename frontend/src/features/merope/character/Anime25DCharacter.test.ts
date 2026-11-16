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

/** A visual DOM installed as the globals React and the component read. */
function installDom() {
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
  return {
    dom,
    restore() {
      dom.window.close()
      for (const [key, descriptor] of previous) {
        if (descriptor) Object.defineProperty(globalThis, key, descriptor)
        else Reflect.deleteProperty(globalThis, key)
      }
    },
  }
}

/**
 * Runs the real React effects, replacing only the GPU/interaction boundaries:
 * the player class, the touch binding and the production hosts are the test's.
 */
async function loadCharacter(stubs: { touch: string, hosts: string }, injected: Record<string, unknown>) {
  const bundle = await build({
    entryPoints: [new URL('./Anime25DCharacter.tsx', import.meta.url).pathname],
    bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{
      name: 'character-test-boundaries',
      setup(builder) {
        // The real figure entries, with only the player and touch binding swapped out.
        const real = (entry: string) => JSON.stringify(require.resolve(`@myriad/figure/${entry}`))
        builder.onResolve({ filter: /^@myriad\/figure\/(anime25drig|interaction)$/ }, ({ path }) => ({ path: path.split('/').pop()!, namespace: 'test' }))
        builder.onResolve({ filter: /^\.\.\/(presence\/touchAppraisalHost|motion\/runtimeHost)$/ }, () => ({ path: 'host', namespace: 'test' }))
        builder.onLoad({ filter: /.*/, namespace: 'test' }, ({ path }) => ({
          contents: path === 'anime25drig'
            ? `export * from ${real('anime25drig')}\nexport const Anime25DPlayer = TestPlayer`
            : path === 'interaction'
              ? `export * from ${real('interaction')}\n${stubs.touch}`
              : stubs.hosts,
          loader: 'js',
          resolveDir: new URL('.', import.meta.url).pathname,
        }))
      },
    }],
  })
  const names = Object.keys(injected)
  const module = { exports: {} as { default: React.ComponentType<any> } }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports', ...names])(require, module, module.exports, ...Object.values(injected))
  return module.exports.default
}

test('outfit and callback changes keep the React player; stale loads cannot report ready or failure', async () => {
  const { dom, restore } = installDom()
  const packages: { atlas: string; resolve: () => void; reject: (error: Error) => void }[] = []
  let created = 0
  let disposed = 0
  class Player {
    capabilities: readonly string[] = []
    constructor() { created += 1 }
    replaceLivePackage(_playback: unknown, _manifest: unknown, atlas: string) {
      return new Promise<void>((resolve, reject) => packages.push({
        atlas,
        resolve: () => {
          this.capabilities = atlas === 'fourth' ? ['head-body', 'right-arm'] : ['head-body', 'torso-volume']
          resolve()
        },
        reject,
      }))
    }

    getMotionCapabilities() { return this.capabilities }
    setSpeechActive() {}
    setSpeechProsody() {}
    setSinging() {}
    setSingingTrack() {}
    setMusicSignal() {}
    setBearing() {}
    setScore() {}
    resize() {}
    tick() {}
    dispose() { disposed += 1 }
  }
  const Character = await loadCharacter({
    touch: 'export function bindCharacterTouch() { return { cancel() {}, unbind() {} } }',
    hosts: 'export function createTouchAppraisal() {} export function getProductionMotionRuntime() {}',
  }, { TestPlayer: Player })
  const root = createRoot(dom.window.document.getElementById('root'))
  const manifest = {}
  const playback = {}
  let ready = 0
  const capabilityReceipts: (readonly string[])[] = []
  const errors: string[] = []
  const render = (atlas: string, label: string) => root.render(createElement(Character, {
    activity: 'idle', mood: 0, manualControl: true, manifest, playback, atlasUrl: atlas,
    onPlaybackReady: (capabilities: readonly string[]) => {
      ready += 1
      capabilityReceipts.push([...capabilities])
    },
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
    assert.deepEqual(capabilityReceipts, [['head-body', 'torso-volume']])
    await act(async () => render('second', 'new callback'))
    assert.equal(packages.length, 2, 'callback-only rerender must not reload the atlas')
    await act(async () => render('third', 'third'))
    await act(async () => packages[2]!.reject(new Error('bad outfit')))
    assert.equal(created, 1, 'a failed hot swap keeps the previous live outfit')
    assert.equal(disposed, 0)
    assert.deepEqual(errors, [])
    assert.equal(ready, 1, 'a failed outfit cannot publish replacement capabilities')
    await act(async () => render('fourth', 'fourth'))
    await act(async () => packages[3]!.resolve())
    assert.equal(created, 1, 'a successful outfit swap reuses the same player')
    assert.equal(ready, 2, 'the live replacement must refresh executable capabilities')
    assert.deepEqual(capabilityReceipts, [
      ['head-body', 'torso-volume'], ['head-body', 'right-arm'],
    ])
    // Genuine GPU loss still recreates the player, with a bounded retry budget.
    for (let i = 0; i < 3; i++) {
      await act(async () => {
        dom.window.document.querySelector('canvas').dispatchEvent(new dom.window.Event('webglcontextlost', { cancelable: true }))
      })
    }
    assert.equal(created, 3)
    assert.equal(disposed, 2)
    assert.deepEqual(errors, ['fourth'], 'terminal GPU errors call the latest owner')
  } finally {
    await act(async () => root.unmount())
    restore()
  }
  assert.equal(disposed, created)
})

test('signing in or out cancels a held touch; a released binding no longer hears it', async () => {
  const { dom, restore } = installDom()
  const log = { binds: 0, cancels: 0, unbinds: 0, released: 0 }
  // The binding's cancel ends the touch the way a pointercancel would: through the reaction cancel.
  const touch = {
    bind(cancelReaction: () => void) {
      log.binds += 1
      return { cancel() { log.cancels += 1; cancelReaction() }, unbind() { log.unbinds += 1 } }
    },
  }
  const hosts = {
    runtime: { touch: { release() { log.released += 1 }, notePresented() {}, update() {}, current() { return null }, version() { return 0 } } },
    appraisal: { cancel() {}, dispose() {}, observe() {} },
  }
  const loads: (() => void)[] = []
  class Player {
    replaceLivePackage() { return new Promise<void>(resolve => loads.push(resolve)) }
    getMotionCapabilities() { return [] }
    getMotionPolicy() { return { expression: 'idle', mouth: 'idle' } }
    getPresentedTouch() { return null }
    hitTestTouch() { return null }
    setTarget() {}
    setSpeechActive() {}
    setSpeechProsody() {}
    setSinging() {}
    setSingingTrack() {}
    setMusicSignal() {}
    setBearing() {}
    setScore() {}
    resize() {}
    tick() {}
    dispose() {}
  }
  const Character = await loadCharacter({
    touch: 'export function bindCharacterTouch(_canvas, _hit, _onTouch, cancelReaction) { return TouchLog.bind(cancelReaction) }',
    hosts: 'export function createTouchAppraisal() { return Hosts.appraisal } export function getProductionMotionRuntime() { return Hosts.runtime }',
  }, { TestPlayer: Player, TouchLog: touch, Hosts: hosts })
  const root = createRoot(dom.window.document.getElementById('root'))
  const render = (touchEnabled: boolean) => root.render(createElement(Character, {
    activity: 'idle', mood: 0, manifest: {}, playback: {}, atlasUrl: 'atlas', touchEnabled,
  }))
  const signIn = () => dom.window.dispatchEvent(new dom.window.CustomEvent('auth-state-changed', {
    detail: { isAuthenticated: true, isAdmin: false },
  }))
  try {
    await act(async () => render(true))
    assert.equal(log.binds, 0, 'touch binds only once the outfit is live')
    await act(async () => loads[0]!())
    assert.equal(log.binds, 1)
    await act(async () => signIn())
    assert.equal(log.cancels, 1)
    assert.equal(log.released, 1, 'the cancelled touch releases her touch reaction')
    await act(async () => render(false))
    assert.equal(log.unbinds, 1)
    await act(async () => signIn())
    assert.equal(log.cancels, 1, 'an unbound surface no longer listens for sign-in changes')
  } finally {
    await act(async () => root.unmount())
    restore()
  }
})
