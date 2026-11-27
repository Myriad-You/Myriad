// A page without a browser for the contract's lifecycle tests: a canvas whose WebGL2 context
// accepts every call and counts them, images that load at once, and the window, document and
// animation frames the figure reaches for. An atlas at an address ending in /unreachable.png
// fails to load. Everything installed is put back by the returned function.

/** What the figure asked of the GPU, by call name. */
export type GlCalls = Map<string, number>

export interface HeadlessCanvas extends EventTarget {
  width: number
  height: number
  style: Record<string, string>
  dataset: Record<string, string>
  isConnected: boolean
  getContext: (kind: string) => unknown
  getBoundingClientRect: () => { left: number, top: number, width: number, height: number }
  setPointerCapture: (id: number) => void
  hasPointerCapture: (id: number) => boolean
  releasePointerCapture: (id: number) => void
}

const SIZES = new Set(['MAX_TEXTURE_SIZE', 'MAX_VERTEX_ATTRIBS', 'MAX_TEXTURE_IMAGE_UNITS', 'MAX_SAMPLES', 'MAX_RENDERBUFFER_SIZE'])
const ATLAS_EDGE = 256

function recordingGl(calls: GlCalls, canvas: unknown): unknown {
  const constants = new Map<string, number>()
  const constantOf = (name: string): number => {
    if (!constants.has(name))
      constants.set(name, constants.size + 1)
    return constants.get(name)!
  }
  const methods = new Map<string, (...args: unknown[]) => unknown>()
  const method = (name: string) => (...args: unknown[]): unknown => {
    calls.set(name, (calls.get(name) ?? 0) + 1)
    if (name === 'getShaderParameter' || name === 'getProgramParameter')
      return true
    if (name === 'checkFramebufferStatus')
      return constantOf('FRAMEBUFFER_COMPLETE')
    if (name === 'getParameter')
      return SIZES.has([...constants].find(([, value]) => value === args[0])?.[0] ?? '') ? 4096 : null
    if (name === 'getAttribLocation' || name === 'getError')
      return 0
    if (name === 'getUniformLocation' || name.startsWith('create'))
      return {}
    if (name === 'isContextLost')
      return false
    return null
  }
  return new Proxy({ canvas, drawingBufferWidth: 300, drawingBufferHeight: 400 } as Record<string, unknown>, {
    get(target, name) {
      if (typeof name !== 'string' || name in target)
        return target[name as string]
      if (/^[A-Z][A-Z0-9_]*$/.test(name))
        return constantOf(name)
      if (!methods.has(name))
        methods.set(name, method(name))
      return methods.get(name)
    },
  })
}

function context2d(canvas: { width: number, height: number }): unknown {
  // Every crop reads as an opaque ellipse on clear ground.
  const held: Record<string, unknown> = {
    canvas,
    getImageData(_x: number, _y: number, w: number, h: number) {
      if (!(w >= 1 && h >= 1) || w * h > 64_000_000)
        throw new RangeError(`canvas ${w}×${h}`)
      const data = new Uint8ClampedArray(w * h * 4)
      for (let y = 0; y < h; y += 1) {
        for (let x = 0; x < w; x += 1) {
          const dx = (x + 0.5) / w * 2 - 1
          const dy = (y + 0.5) / h * 2 - 1
          if (dx * dx + dy * dy > 1)
            continue
          data.set([200, 160, 140, 255], (y * w + x) * 4)
        }
      }
      return { data, width: w, height: h }
    },
    createImageData: (w: number, h: number) => ({ data: new Uint8ClampedArray(w * h * 4), width: w, height: h }),
    measureText: () => ({ width: 10 }),
  }
  return new Proxy(held, {
    get: (target, name) => (name in target ? target[name as string] : () => {}),
    set: (target, name, value) => {
      target[name as string] = value
      return true
    },
  })
}

export interface Headless {
  /** A canvas for the figure to draw on; its WebGL2 calls land in `gl`. */
  canvas: HeadlessCanvas
  gl: GlCalls
  /** Runs the animation frames requested so far. */
  runFrames: () => void
  restore: () => void
}

/** Installs the page; the figure's canvas is the one returned. */
export function installHeadlessPage(): Headless {
  const previous = new Map<string, PropertyDescriptor | undefined>()
  const set = (name: string, value: unknown): void => {
    previous.set(name, Object.getOwnPropertyDescriptor(globalThis, name))
    Object.defineProperty(globalThis, name, { value, configurable: true, writable: true })
  }
  const gl: GlCalls = new Map()
  const captured = new Set<number>()
  class Canvas extends EventTarget {
    width = 300
    height = 150
    style: Record<string, string> = {}
    dataset: Record<string, string> = {}
    isConnected = true
    getContext: (kind: string) => unknown = () => context2d(this)
    getBoundingClientRect() { return { left: 0, top: 0, width: 300, height: 400 } }
    setPointerCapture(id: number) { captured.add(id) }
    hasPointerCapture(id: number) { return captured.has(id) }
    releasePointerCapture(id: number) { captured.delete(id) }
  }
  class AtlasImage {
    width = ATLAS_EDGE
    height = ATLAS_EDGE
    naturalWidth = ATLAS_EDGE
    naturalHeight = ATLAS_EDGE
    complete = false
    crossOrigin = ''
    decoding = ''
    onload: (() => void) | null = null
    onerror: (() => void) | null = null
    private url = ''
    get src() { return this.url }
    set src(url: string) {
      this.url = url
      if (!url)
        return
      // An address ending in /unreachable.png fails as an image the server never sent would.
      const fails = url.endsWith('/unreachable.png')
      this.complete = !fails
      queueMicrotask(() => (fails ? this.onerror?.() : this.onload?.()))
    }

    decode() { return Promise.resolve() }
  }
  const frames = new Map<number, (time: number) => void>()
  let nextFrame = 1
  const page = new EventTarget()
  const document = Object.assign(new EventTarget(), {
    hidden: false,
    visibilityState: 'visible',
    createElement: () => new Canvas(),
    elementFromPoint: () => canvas,
  })
  set('HTMLCanvasElement', Canvas)
  set('Image', AtlasImage)
  set('document', document)
  set('window', Object.assign(page, { devicePixelRatio: 1 }))
  set('requestAnimationFrame', (callback: (time: number) => void) => {
    const id = nextFrame++
    frames.set(id, callback)
    return id
  })
  set('cancelAnimationFrame', (id: number) => { frames.delete(id) })
  const canvas = new Canvas() as unknown as HeadlessCanvas
  const context = recordingGl(gl, canvas)
  canvas.getContext = (kind: string) => (kind === 'webgl2' ? context : null)
  return {
    canvas,
    gl,
    runFrames: () => {
      const due = [...frames]
      frames.clear()
      for (const [, callback] of due) callback(performance.now())
    },
    restore: () => {
      for (const [name, descriptor] of previous) {
        if (descriptor)
          Object.defineProperty(globalThis, name, descriptor)
        else
          delete (globalThis as Record<string, unknown>)[name]
      }
    },
  }
}

/** The sum of the calls whose names pass `match`. */
export function countCalls(gl: GlCalls, match: (name: string) => boolean): number {
  let total = 0
  for (const [name, count] of gl) {
    if (match(name))
      total += count
  }
  return total
}
