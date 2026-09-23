import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import ts from 'typescript'

const source = ts.createSourceFile('preload.ts', readFileSync(new URL('./usePreload.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true)
const node = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === 'usePreload')!
const script = ts.transpile(node.getText(source).replace('export ', ''), { target: ts.ScriptTarget.ES2022 })

for (const reason of ['abort', 'timeout'] as const) {
  test(`music preload releases audio and settles on ${reason}`, async (context) => {
    context.mock.timers.enable({ apis: ['setTimeout'] })
    const audio = new class extends EventTarget {
      src = ''; load() {} pause() {} removeAttribute() { this.src = '' }
    }()
    let loader: (signal: AbortSignal) => Promise<void> = async () => {}
    let cancelled = 0
    const effects: Array<() => void | (() => void)> = []
    const dependencies = {
      useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [initial, () => {}],
      useCallback: (callback: unknown) => callback, useEffect: (effect: () => void) => effects.push(effect),
      createPreloadAudioElement: () => audio, ensureSpectrumSafePlaybackUrl: () => 'https://music.test/audio',
      getMusicProxyFallbackUrl: () => null, pickAdjacentIndex: () => 0, pickShuffleIndex: () => 0,
      globalResourceLoader: { cancelTask: () => { cancelled++ } },
      loadResource: { low: (_id: string, next: typeof loader) => { loader = next } },
    }
    const hook = compileFunction(`${script}; return usePreload;`, Object.keys(dependencies))(...Object.values(dependencies))
    const api = hook({ enabled: true, playlist: [{ id: 'a' }], volume: 1, setPlaylist: () => {}, neteaseProxyFallbackTriedRef: { current: new Set() } })
    const cleanup = effects.map(effect => effect())
    api.preloadNextSong(0, true)
    const controller = new AbortController()
    const pending = loader(controller.signal)
    const rejection = assert.rejects(pending)
    assert.ok(audio.src)
    if (reason === 'abort') controller.abort()
    else context.mock.timers.tick(15_000)
    await rejection
    assert.equal(audio.src, '')
    cleanup.forEach(fn => { if (typeof fn === 'function') fn() })
    assert.equal(cancelled, 1)
  })
}

test('music preload does nothing when disabled', () => {
  let scheduled = 0
  const dependencies = {
    useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [initial, () => {}],
    useCallback: (callback: unknown) => callback, useEffect: () => {},
    createPreloadAudioElement: () => null, ensureSpectrumSafePlaybackUrl: () => 'https://music.test/audio',
    getMusicProxyFallbackUrl: () => null, pickAdjacentIndex: () => 0, pickShuffleIndex: () => 0,
    globalResourceLoader: { cancelTask: () => {} },
    loadResource: { low: () => { scheduled++ } },
  }
  const hook = compileFunction(`${script}; return usePreload;`, Object.keys(dependencies))(...Object.values(dependencies))
  const api = hook({ enabled: false, playlist: [{ id: 'a' }], volume: 1, setPlaylist: () => {}, neteaseProxyFallbackTriedRef: { current: new Set() } })
  api.preloadAudioRef.current = {}
  api.preloadNextSong(0, true)
  assert.equal(scheduled, 0)
})
