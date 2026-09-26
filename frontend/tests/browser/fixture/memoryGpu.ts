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
  const bufferBytes = new Map<unknown, number>()
  const textureBytes = new Map<unknown, Map<number, number>>()
  const boundBuffers = new Map<unknown, unknown>()
  const boundTextures = new Map<string, unknown>()
  let textureUnit: number = gl.TEXTURE0
  const bytes = () => ({
    buffers: Array.from(bufferBytes.values()).reduce((a, b) => a + b, 0),
    textures: Array.from(textureBytes.values()).reduce((sum, levels) => sum + Array.from(levels.values()).reduce((a, b) => a + b, 0), 0),
  })
  const peak = { buffers: 0, textures: 0 }
  const measurePeak = () => { const current = bytes(); peak.buffers = Math.max(peak.buffers, current.buffers); peak.textures = Math.max(peak.textures, current.textures) }
  const wrap = (key: string, after: (args: unknown[]) => void) => {
    const target = gl as unknown as Record<string, (...args: unknown[]) => unknown>
    const original = target[key]
    target[key] = (...args) => { const result = original.apply(gl, args); after(args); return result }
    restore.push(() => { target[key] = original })
  }
  wrap('bindBuffer', ([target, buffer]) => boundBuffers.set(target, buffer))
  wrap('bufferData', ([target, data, , offset = 0, length]) => {
    const buffer = boundBuffers.get(target)
    if (!buffer) return
    const view = data as { byteLength: number; BYTES_PER_ELEMENT?: number }
    bufferBytes.set(buffer, typeof data === 'number' ? data : length !== undefined ? Number(length) * (view.BYTES_PER_ELEMENT || 1) : view.byteLength - Number(offset) * (view.BYTES_PER_ELEMENT || 1))
    measurePeak()
  })
  wrap('activeTexture', ([unit]) => { textureUnit = Number(unit) })
  wrap('bindTexture', ([target, texture]) => boundTextures.set(`${textureUnit}:${target}`, texture))
  wrap('texImage2D', args => {
    const [target, level] = args
    const texture = boundTextures.get(`${textureUnit}:${target}`)
    if (!texture) return
    const source = args[5] as { width: number; height: number }
    const width = args.length >= 9 ? Number(args[3]) : source.width
    const height = args.length >= 9 ? Number(args[4]) : source.height
    const format = args.length >= 9 ? args[6] : args[3]
    const type = args.length >= 9 ? args[7] : args[4]
    // Production atlas uploads are RGBA8. Mipmaps are generateMipmap on the GPU,
    // not extra texImage2D levels, so this probe still accounts requested level-0 bytes.
    if (format !== gl.RGBA || type !== gl.UNSIGNED_BYTE) throw new Error('GPU byte probe needs a format accounting update')
    const levels = textureBytes.get(texture) || new Map<number, number>()
    levels.set(Number(level), width * height * 4)
    textureBytes.set(texture, levels)
    measurePeak()
  })
  for (const kind of kinds) {
    for (const action of ['create', 'delete'] as const) {
      const key = `${action}${kind}`
      const target = gl as unknown as Record<string, (...args: unknown[]) => unknown>
      const original = target[key]
      target[key] = (...args) => {
        const result = original.apply(gl, args)
        if (action === 'create' && result) live[kind].add(result)
        if (action === 'delete') {
          live[kind].delete(args[0])
          if (kind === 'Buffer') bufferBytes.delete(args[0])
          if (kind === 'Texture') textureBytes.delete(args[0])
        }
        return result
      }
      restore.push(() => { target[key] = original })
    }
  }
  return {
    counts: () => Object.fromEntries(kinds.map(k => [k, live[k].size])),
    bytes,
    peak: () => ({ ...peak }),
    restore: () => restore.forEach(fn => fn()),
  }
}

async function gpuCycles(cycles: number, failure = '', atlasSize = 0) {
  const { playback, url: originalUrl } = await assets
  let url = originalUrl
  if (atlasSize) {
    const source = new Image()
    source.src = originalUrl
    await source.decode()
    const enlarged = document.createElement('canvas')
    enlarged.width = enlarged.height = atlasSize
    enlarged.getContext('2d')!.drawImage(source, 0, 0, atlasSize, atlasSize)
    const blob = await new Promise<Blob>((resolve, reject) => enlarged.toBlob(value => value ? resolve(value) : reject(new Error('Atlas encoding failed')), 'image/png'))
    url = URL.createObjectURL(blob)
    enlarged.width = enlarged.height = 0
    source.src = ''
  }
  const canvas = document.createElement('canvas')
  document.body.append(canvas)
  const gl = canvas.getContext('webgl2', { stencil: true, preserveDrawingBuffer: true })!
  if (!gl) throw new Error('WebGL2 unavailable')
  const debug = gl.getExtension('WEBGL_debug_renderer_info')
  const renderer = {
    userAgent: navigator.userAgent, version: gl.getParameter(gl.VERSION),
    renderer: gl.getParameter(gl.RENDERER), vendor: gl.getParameter(gl.VENDOR),
    unmaskedRenderer: debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : null,
    maxTextureSize: gl.getParameter(gl.MAX_TEXTURE_SIZE),
  }
  const tracked = instrument(gl)
  const snapshots = []
  let rejected = 0
  let unchanged = true
  let drawingBufferBytes = 0
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
        drawingBufferBytes = Math.max(1, gl.drawingBufferWidth) * Math.max(1, gl.drawingBufferHeight) * 4
        player.tick(1 / 60)
        const before = read()
        const active = tracked.counts()
        const activeBytes = tracked.bytes()
        const upload = gl.texImage2D
        if (failure === 'upload') gl.texImage2D = () => { throw new Error('Injected texture upload failure') }
        try { await player.loadAtlas(url) } catch { rejected++ }
        finally { gl.texImage2D = upload }
        if (failure) {
          // Redraw the retained outfit without advancing animation time.
          ;(player as unknown as { draw: () => void }).draw()
          unchanged &&= before === read()
        }
        snapshots.push({ active, afterReplacement: tracked.counts(), activeBytes, afterReplacementBytes: tracked.bytes() })
      } finally { player.dispose() }
      snapshots.push(tracked.counts())
    }
    return {
      snapshots, final: tracked.counts(), rejected, unchanged, glError: gl.getError(),
      finalBytes: tracked.bytes(), peakBytes: tracked.peak(), drawingBufferBytes, renderer,
      byteMetric: 'Requested RGBA8 texels and bufferData payload bytes; excludes driver overhead, framebuffers, decoded images, CPU copies and physical residency',
    }
  } finally {
    tracked.restore()
    canvas.remove()
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    if (url !== originalUrl) URL.revokeObjectURL(url)
  }
}

async function glassCycles(cycles: number) {
  let largestMap = 0
  let supported = false
  let fallbackStyle = ''
  for (let i = 0; i < cycles; i++) {
    const engine = createHyalite()
    supported = engine.supported()
    const panel = document.createElement('div')
    panel.style.cssText = 'width: 240px; height: 120px; border-radius: 24px; backdrop-filter: var(--hyalite, blur(5.1px)); -webkit-backdrop-filter: var(--hyalite, blur(5.1px))'
    document.body.append(panel)
    engine.attach(panel, { materialize: 0, onBuild: info => { largestMap = Math.max(largestMap, info.mapSize[0] * info.mapSize[1]) } })
    if (!supported) fallbackStyle = getComputedStyle(panel).getPropertyValue('backdrop-filter') || getComputedStyle(panel).getPropertyValue('-webkit-backdrop-filter')
    engine.detach(panel)
    engine.dispose()
    panel.remove()
  }
  await new Promise<void>(resolve => requestAnimationFrame(() => resolve()))
  return { largestMap, supported, fallbackStyle, filters: document.querySelectorAll('filter').length, canvases: document.querySelectorAll('canvas').length }
}

const contextTrackers = new Map<WebGL2RenderingContext, ReturnType<typeof instrument>>()
function currentCharacterResources() {
  const counts = { Buffer: 0, Texture: 0, Program: 0, Shader: 0, VertexArray: 0 }
  const bytes = { buffers: 0, textures: 0 }
  for (const tracked of contextTrackers.values()) {
    const live = tracked.counts()
    for (const key of Object.keys(counts) as Array<keyof typeof counts>) counts[key] += live[key]
    const payload = tracked.bytes()
    bytes.buffers += payload.buffers
    bytes.textures += payload.textures
  }
  return { contexts: contextTrackers.size, counts, bytes }
}
let lastCharacterResources = currentCharacterResources()
let character: { host: HTMLDivElement; root: ReturnType<typeof createRoot>; restoreContextFactory: () => void } | null = null
let ticks = 0
const originalTick = Anime25DPlayer.prototype.tick
Anime25DPlayer.prototype.tick = function (dt) { ticks++; return originalTick.call(this, dt) }
const style = document.createElement('style')
style.textContent = '.merope-rig { display: block; width: 256px; height: 300px; }'
document.head.append(style)

async function mountCharacter() {
  const { manifest, playback, url } = await assets
  if (character) unmountCharacter()
  const getContext = HTMLCanvasElement.prototype.getContext
  HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, id: string, ...args: unknown[]) {
    const context = Reflect.apply(getContext, this, [id, ...args])
    if (id === 'webgl2' && context && !contextTrackers.has(context)) contextTrackers.set(context, instrument(context))
    return context
  } as typeof getContext
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  character = { host, root, restoreContextFactory: () => { HTMLCanvasElement.prototype.getContext = getContext } }
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
  lastCharacterResources = currentCharacterResources()
  character.restoreContextFactory()
  for (const tracked of contextTrackers.values()) tracked.restore()
  contextTrackers.clear()
  character = null
}

const api = {
  gpuCycles, glassCycles, mountCharacter, unmountCharacter,
  ticks: () => ticks,
  characterResources: () => character ? currentCharacterResources() : lastCharacterResources,
  offscreen: (hidden: boolean) => { character!.host.style.transform = hidden ? 'translateY(-10000px)' : '' },
}
declare global { interface Window { memoryGpu: typeof api } }
window.memoryGpu = api
