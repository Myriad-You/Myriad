import type { RigMode } from './useRigImport'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { activeRigMode, useOpeningRigMode } from './rigMode'

function manifest(master: string, extra: Record<string, unknown> = {}) {
  return {
    sourceMasterAssetId: master,
    anime25dPlayback: { layers: [], ...extra },
  } as never
}

test('the mode the rig was made in, for its own portrait only', () => {
  assert.equal(activeRigMode(null, '/a.png'), null)
  assert.equal(activeRigMode({ sourceMasterAssetId: '/a.png' }, '/a.png'), null, 'no playback')
  assert.equal(activeRigMode(manifest('/a.png'), '/b.png'), null, 'a rig of another portrait')
  assert.equal(activeRigMode(manifest('/a.png'), '/a.png'), 'plain')
  assert.equal(activeRigMode(manifest('/a.png', { turnKeyforms: {} }), '/a.png'), 'enhanced', 'keyed before the record')
  assert.equal(activeRigMode(manifest('/a.png', { enhancement: { turn: false, expressions: true } }), '/a.png'), 'enhanced')
  assert.equal(activeRigMode(manifest('/a.png', { enhancement: { turn: false, expressions: false } }), '/a.png'), 'plain')
  // A figure keyed by its body alone.
  assert.equal(activeRigMode(manifest('/a.png', { enhancement: { turn: false, expressions: false, body: true } }), '/a.png'), 'enhanced')
  assert.equal(activeRigMode(manifest('/a.png', { enhancement: { turn: false, expressions: false, arms: true } }), '/a.png'), 'enhanced')
})

/** Renders the panel's opening over a sequence of what it is given; returns the modes it set, in order. */
async function opened(steps: Array<{ activeMode: RigMode | null; splitting: boolean } | 'leave'>): Promise<RigMode[]> {
  const require = createRequire(import.meta.url)
  const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
  const dom = new JSDOM('<div id="root"></div>', { url: 'https://test.invalid' })
  const prior = new Map<string, PropertyDescriptor | undefined>()
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true })) {
    prior.set(key, Object.getOwnPropertyDescriptor(globalThis, key))
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  const set: RigMode[] = []
  const setMode = (mode: RigMode) => { set.push(mode) }
  function Panel({ activeMode, splitting }: { activeMode: RigMode | null; splitting: boolean }) {
    useOpeningRigMode(activeMode, splitting, setMode)
    return null
  }
  let root = createRoot(dom.window.document.getElementById('root')!)
  try {
    for (const step of steps) {
      if (step === 'leave') {
        // The panel is left (another tab of the workbench) and come back to: it appears anew.
        await act(async () => root.unmount())
        root = createRoot(dom.window.document.getElementById('root')!)
        continue
      }
      await act(async () => root.render(createElement(Panel, step)))
    }
    return set
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of prior) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
}

test('the panel opens on the mode the active rig was made in, once it is known', async () => {
  assert.deepEqual(await opened([{ activeMode: 'enhanced', splitting: false }]), ['enhanced'])
  // Known later than the panel appears; told again, it is not opened again.
  assert.deepEqual(await opened([
    { activeMode: null, splitting: false },
    { activeMode: 'plain', splitting: false },
    { activeMode: 'plain', splitting: false },
  ]), ['plain'])
})

test('a panel opened on a plain rig while a split runs stays on the split', async () => {
  // The split was picked up after a reload (the hook above the panel chose the enhanced mode),
  // then the panel is opened: the plain rig's mode must not take it back.
  assert.deepEqual(await opened([{ activeMode: 'plain', splitting: true }]), [])
  // The active rig's mode comes in later than the split's status.
  assert.deepEqual(await opened([
    { activeMode: null, splitting: true },
    { activeMode: 'plain', splitting: true },
  ]), [])
  // Begun here, the panel left and come back to while it runs.
  assert.deepEqual(await opened([
    { activeMode: 'plain', splitting: false },
    { activeMode: 'plain', splitting: true },
    'leave',
    { activeMode: 'plain', splitting: true },
  ]), ['plain'])
})

test("a split's end does not open the panel anew: the mode stays where the split was", async () => {
  assert.deepEqual(await opened([
    { activeMode: 'plain', splitting: true },
    { activeMode: 'plain', splitting: false },
  ]), [])
  // What it made put on, the rig's mode is the split's.
  assert.deepEqual(await opened([
    { activeMode: 'plain', splitting: true },
    { activeMode: 'plain', splitting: false },
    { activeMode: 'enhanced', splitting: false },
  ]), ['enhanced'])
})
