import type { MeropeRigManifest } from '../../../src/features/merope/rig/types'
import { createElement } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import Anime25DCharacter from '../../../src/features/merope/anime25drig/Anime25DCharacter'
import { Anime25DPlayer } from '../../../src/features/merope/anime25drig/player'
import { anime25DImportCopy } from '../../../src/features/merope/rig/anime25dImportCopy'
import { prepareAnime25DRigPsd } from '../../../src/features/merope/rig/anime25dImporter'
import { syntheticSeeThroughPsd } from '../../../src/features/merope/rig/anime25dImporter.fixture'
import { createHyalite } from '../../../src/utils/liquidGlass/vendor/hyalite'

const psd = syntheticSeeThroughPsd()
psd.height = 300
const assets = prepareAnime25DRigPsd(psd, '/unused.png', anime25DImportCopy()).then(prepared => {
  const manifest: MeropeRigManifest = {
    ...prepared.source, schemaVersion: 1, quality: 'layered-2d', textures: [], parts: [],
  }
  return { manifest, playback: prepared.source.anime25dPlayback!, url: URL.createObjectURL(prepared.atlas) }
})

function instrument(gl: WebGL2RenderingContext) {
  const kinds = ['Buffer', 'Texture', 'Program', 'Shader', 'VertexArray'] as const
  const live = Object.fromEntries(kinds.map(k => [k, new Set<unknown>()]))
  const restore: Array<() => void> = []
  for (const kind of kinds) {
    for (const action of ['create', 'delete'] as const) {
      const key = `${action}${kind}`
      const target = gl as unknown as Record<string, (...args: unknown[]) => unknown>
      const original = target[key]
      target[key] = (...args) => {
        const result = original.apply(gl, args)
        if (action === 'create' && result) live[kind].add(result)
        if (action === 'delete') live[kind].delete(args[0])
        return result
      }
      restore.push(() => { target[key] = original })
    }
  }
  return {
    counts: () => Object.fromEntries(kinds.map(k => [k, live[k].size])),
    restore: () => restore.forEach(fn => fn()),
  }
}

async function gpuCycles(cycles: number, failure = '') {
  const { playback, url } = await assets
  const canvas = document.createElement('canvas')
  document.body.append(canvas)
  const gl = canvas.getContext('webgl2', { stencil: true, preserveDrawingBuffer: true })!
  const tracked = instrument(gl)
  const snapshots = []
  let rejected = 0
  let unchanged = true
  const read = () => {
    const data = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4)
    gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, data)
    let hash = 2166136261
    for (const byte of data) hash = Math.imul(hash ^ byte, 16777619)
    return hash >>> 0
  }
  try {
    for (let i = 0; i < cycles; i++) {
      if (failure === 'constructor') {
        const original = gl.getUniformLocation
        gl.getUniformLocation = () => null
        try {
          const unexpected = new Anime25DPlayer(canvas, playback)
          unexpected.dispose()
        } catch { rejected++ }
        finally { gl.getUniformLocation = original }
        snapshots.push(tracked.counts())
        continue
      }
      const player = new Anime25DPlayer(canvas, playback)
      try {
        await player.loadAtlas(url)
        player.resize(256, 300, 1)
        player.tick(1 / 60)
        const before = read()
        const active = tracked.counts()
        const upload = gl.texImage2D
        if (failure === 'upload') gl.texImage2D = () => { throw new Error('Injected texture upload failure') }
        try { await player.loadAtlas(url) } catch { rejected++ }
        finally { gl.texImage2D = upload }
        if (failure) {
          // Redraw the retained outfit without advancing animation time.
          ;(player as unknown as { draw: () => void }).draw()
          unchanged &&= before === read()
        }
        snapshots.push({ active, afterReplacement: tracked.counts() })
      } finally { player.dispose() }
      snapshots.push(tracked.counts())
    }
    return { snapshots, final: tracked.counts(), rejected, unchanged, glError: gl.getError() }
  } finally {
    tracked.restore()
    canvas.remove()
    gl.getExtension('WEBGL_lose_context')?.loseContext()
  }
}

async function glassCycles(cycles: number) {
  let largestMap = 0
  for (let i = 0; i < cycles; i++) {
    const engine = createHyalite()
    const panel = document.createElement('div')
    panel.style.cssText = 'width: 240px; height: 120px; border-radius: 24px'
    document.body.append(panel)
    engine.attach(panel, { materialize: 0, onBuild: info => { largestMap = Math.max(largestMap, info.mapSize[0] * info.mapSize[1]) } })
    engine.detach(panel)
    engine.dispose()
    panel.remove()
  }
  await new Promise<void>(resolve => requestAnimationFrame(() => resolve()))
  return { largestMap, filters: document.querySelectorAll('filter').length, canvases: document.querySelectorAll('canvas').length }
}

let character: { host: HTMLDivElement; root: ReturnType<typeof createRoot> } | null = null
let ticks = 0
const originalTick = Anime25DPlayer.prototype.tick
Anime25DPlayer.prototype.tick = function (dt) { ticks++; return originalTick.call(this, dt) }
const style = document.createElement('style')
style.textContent = '.merope-rig { display: block; width: 256px; height: 300px; }'
document.head.append(style)

async function mountCharacter() {
  const { manifest, playback, url } = await assets
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  character = { host, root }
  await new Promise<void>((resolve, reject) => {
    flushSync(() => root.render(createElement(Anime25DCharacter, {
      activity: 'idle', mood: 0, manifest, playback, atlasUrl: url, manualControl: true,
      onPlaybackReady: resolve, onPlaybackError: reject,
    })))
  })
}

function unmountCharacter() {
  if (!character) return
  flushSync(() => character!.root.unmount())
  character.host.remove()
  character = null
}

const api = {
  gpuCycles, glassCycles, mountCharacter, unmountCharacter,
  ticks: () => ticks,
  offscreen: (hidden: boolean) => { character!.host.style.transform = hidden ? 'translateY(-10000px)' : '' },
}
declare global { interface Window { memoryGpu: typeof api } }
window.memoryGpu = api
