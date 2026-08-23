import type { Anime25DPlayback, Anime25DPlaybackLayer } from './types'
import { frontHairUpperParallaxScale } from './hairPhysics'

const VERTEX_SHADER = `#version 300 es
in vec2 a_pos;
in vec2 a_uv;
uniform vec2 u_view;
out vec2 v_uv;
void main() {
  vec2 clip = vec2(a_pos.x / u_view.x * 2.0 - 1.0, 1.0 - a_pos.y / u_view.y * 2.0);
  gl_Position = vec4(clip, 0.0, 1.0);
  v_uv = a_uv;
}`

const FRAGMENT_SHADER = `#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_texture;
uniform float u_cut;
uniform float u_opacity;
out vec4 out_color;
void main() {
  vec4 color = texture(u_texture, v_uv);
  if (color.a < u_cut) discard;
  out_color = color * u_opacity;
}`

/** Parameter block copied from Anime2.5DRig `P` / `auto` in index.html. */
export interface Anime25DDriver {
  angleX: number
  angleY: number
  angleZ: number
  eyeOpenL: number
  eyeOpenR: number
  eyeX: number
  eyeY: number
  brow: number
  mouthOpen: number
  mouthForm: number
  mouthCY: number
  body: number
  physAmp: number
  soft: number
  browAngL: number
  browAngR: number
  browAngSym: number
  bangL: number
  bangC: number
  bangR: number
  armY: number
  armPos: number
  bust: number
  bustY: number
  irisScale: number
  mouthEase: number
  eyeEase: number
  fhAmp: number
  fhSoft: number
  eyeCY: number
  eyeCAng: number
  mouthCAng: number
  eyeScaleL: number
  eyeScaleR: number
  mouthScale: number
  idle: boolean
  blink: boolean
  rand: boolean
  talk: boolean
  mouse: boolean
  phys: boolean
}

interface HairSpring {
  x: number
  v: number
  dx: number
}

interface HairStrandSpring {
  stiff: HairSpring
  soft: HairSpring
  phase: number
}

interface GpuLayer {
  source: Anime25DPlaybackLayer
  rest: Float32Array
  deformed: Float32Array
  uvs: Float32Array
  indices: Uint16Array
  cols: number
  rows: number
  vao: WebGLVertexArrayObject
  vertexBuffer: WebGLBuffer
  indexBuffer: WebGLBuffer
  indexCount: number
  texture: WebGLTexture
  frontHair: boolean
  frontHairParallaxScale: Float32Array | null
  strandWeights: Float32Array | null
  alongStrand: Float32Array | null
  bangWeights: Float32Array | null
  springs: HairStrandSpring[] | null
}

export const DEFAULT_FRONT_HAIR_SWAY = 1
export const DEFAULT_REAR_HAIR_SWAY = 0.5

export const IDENTITY_DRIVER: Anime25DDriver = {
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  eyeOpenL: 1,
  eyeOpenR: 1,
  eyeX: 0,
  eyeY: 0,
  brow: 0,
  mouthOpen: 0,
  mouthForm: 0,
  mouthCY: 0,
  body: 0,
  physAmp: DEFAULT_REAR_HAIR_SWAY,
  soft: 2,
  browAngL: 0,
  browAngR: 0,
  browAngSym: 0,
  bangL: 0,
  bangC: 0,
  bangR: 0,
  armY: 0,
  armPos: 0,
  bust: 2.5,
  bustY: 1,
  irisScale: 1,
  mouthEase: 0.45,
  eyeEase: 0.3,
  fhAmp: DEFAULT_FRONT_HAIR_SWAY,
  fhSoft: 0.4,
  eyeCY: 0,
  eyeCAng: 0,
  mouthCAng: 0,
  eyeScaleL: 1,
  eyeScaleR: 1,
  mouthScale: 1,
  idle: true,
  blink: true,
  rand: true,
  talk: true,
  mouse: false,
  phys: true,
}

/** Settings workbench: automations off so each slider can be seen. */
export const WORKBENCH_DRIVER: Anime25DDriver = {
  ...IDENTITY_DRIVER,
  idle: false,
  rand: false,
  talk: false,
  blink: true,
  mouse: false,
  phys: true,
}

const DRIVER_LIMITS: Partial<Record<keyof Anime25DDriver, readonly [number, number]>> = {
  angleX: [-1, 1], angleY: [-1, 1], angleZ: [-1, 1],
  eyeOpenL: [0, 1], eyeOpenR: [0, 1], eyeX: [-1, 1], eyeY: [-1, 1],
  brow: [-1, 1], mouthOpen: [0, 1], mouthForm: [-1, 1], mouthCY: [-1, 1],
  body: [-1, 1], physAmp: [0, 3], soft: [0, 3],
  browAngL: [-1, 1], browAngR: [-1, 1], browAngSym: [-1, 1],
  bangL: [-1, 1], bangC: [-1, 1], bangR: [-1, 1], armY: [-1, 1], armPos: [-1, 1],
  bust: [0, 4], bustY: [-3, 3], irisScale: [0.5, 1.3],
  mouthEase: [0, 1], eyeEase: [0, 1], fhAmp: [0, 3], fhSoft: [0, 2],
  eyeCY: [-1, 1], eyeCAng: [-1, 1], mouthCAng: [-1, 1],
  eyeScaleL: [0.5, 1.5], eyeScaleR: [0.5, 1.5], mouthScale: [0.5, 1.5],
}

export function sanitizeDriverPatch(
  partial: Partial<Anime25DDriver>,
): Partial<Anime25DDriver> {
  const sanitized: Partial<Anime25DDriver> = {}
  const output = sanitized as Record<string, unknown>
  for (const [rawKey, rawValue] of Object.entries(partial)) {
    const key = rawKey as keyof Anime25DDriver
    const identityValue = IDENTITY_DRIVER[key]
    if (typeof identityValue === 'boolean') {
      if (typeof rawValue === 'boolean') output[rawKey] = rawValue
      continue
    }
    if (typeof rawValue !== 'number' || !Number.isFinite(rawValue)) continue
    const limits = DRIVER_LIMITS[key]
    output[rawKey] = limits
      ? Math.max(limits[0], Math.min(limits[1], rawValue))
      : rawValue
  }
  return sanitized
}

export interface Anime25DDebugSnapshot {
  layerCount: number
  hairLayerCount: number
  strandCount: number
  eyeOpenLayers: number
  eyeCloseLayers: number
  mouthOpenLayers: number
  mouthCloseLayers: number
  canvas: { width: number; height: number }
  current: Anime25DDriver
}

export class Anime25DPlayer {
  private readonly gl: WebGL2RenderingContext
  private readonly playback: Anime25DPlayback
  private readonly program: WebGLProgram
  private readonly viewLocation: WebGLUniformLocation
  private readonly opacityLocation: WebGLUniformLocation
  private readonly cutLocation: WebGLUniformLocation
  private layers: GpuLayer[] = []
  private readonly current: Anime25DDriver = { ...IDENTITY_DRIVER }
  private readonly target: Anime25DDriver = { ...IDENTITY_DRIVER }
  private time = 0
  private blinkT = -1
  private nextBlink = 1.8
  private nextRnd = 0
  private readonly rnd = { ax: 0, ay: 0, az: 0, bd: 0, ex: 0, ey: 0 }
  private talkOn = false
  private talkV = 0
  private talkTgt = 0
  private nextTalkState = 0
  private nextSyl = 0
  private readonly bounce = { x: 0, v: 0, dy: 0 }
  private readonly mouse = { x: 0, y: 0, inside: false }
  private disposed = false
  private viewWidth = 1
  private viewHeight = 1

  constructor(canvas: HTMLCanvasElement, playback: Anime25DPlayback) {
    const gl = canvas.getContext('webgl2', {
      alpha: true,
      premultipliedAlpha: true,
      stencil: true,
      antialias: true,
    })
    if (!gl) throw new Error('WebGL2 is required for Anime2.5DRig playback')
    this.gl = gl
    this.playback = playback
    this.program = compileProgram(gl)
    this.viewLocation = requiredUniform(gl, this.program, 'u_view')
    this.opacityLocation = requiredUniform(gl, this.program, 'u_opacity')
    this.cutLocation = requiredUniform(gl, this.program, 'u_cut')
    gl.useProgram(this.program)
    gl.uniform1i(requiredUniform(gl, this.program, 'u_texture'), 0)
    gl.enable(gl.BLEND)
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA)
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
  }

  async loadAtlas(url: string): Promise<void> {
    const image = await loadImage(url)
    this.layers = this.playback.layers.map((layer, index) =>
      this.createLayer(layer, image, index),
    )
  }

  setTarget(partial: Partial<Anime25DDriver>): void {
    Object.assign(this.target, sanitizeDriverPatch(partial))
  }

  replaceTarget(driver: Anime25DDriver): void {
    Object.assign(this.target, IDENTITY_DRIVER, sanitizeDriverPatch(driver))
  }

  getTarget(): Anime25DDriver {
    return { ...this.target }
  }

  getCurrent(): Anime25DDriver {
    return { ...this.current }
  }

  blinkNow(): void {
    this.blinkT = 0
    this.nextBlink = this.time + 1.6 + Math.random() * 3.8
  }

  setMouse(x: number, y: number, inside: boolean): void {
    this.mouse.x = x
    this.mouse.y = y
    this.mouse.inside = inside
  }

  debugSnapshot(): Anime25DDebugSnapshot {
    const layers = this.playback.layers
    return {
      layerCount: layers.length,
      hairLayerCount: layers.filter((layer) => layer.phys === 'hair').length,
      strandCount: layers.reduce((sum, layer) => sum + layer.strands.length, 0),
      eyeOpenLayers: layers.filter((layer) => layer.fade === 'eyeOpen').length,
      eyeCloseLayers: layers.filter((layer) => layer.fade === 'eyeClose').length,
      mouthOpenLayers: layers.filter((layer) => layer.fade === 'mouthOpen').length,
      mouthCloseLayers: layers.filter((layer) => layer.fade === 'mouthClose')
        .length,
      canvas: { ...this.playback.pixelCanvas },
      current: this.getCurrent(),
    }
  }

  resize(cssWidth: number, cssHeight: number, devicePixelRatio: number): void {
    const dpr = Math.max(1, Math.min(2, devicePixelRatio))
    const { width: pixelWidth, height: pixelHeight } = this.playback.pixelCanvas
    const canvas = this.gl.canvas
    const bufferWidth = Math.max(1, Math.round(pixelWidth * dpr))
    const bufferHeight = Math.max(1, Math.round(pixelHeight * dpr))
    if (canvas instanceof HTMLCanvasElement) {
      canvas.width = bufferWidth
      canvas.height = bufferHeight
      const scale = Math.min(
        Math.max(1, cssWidth) / pixelWidth,
        Math.max(1, cssHeight) / pixelHeight,
      )
      canvas.style.width = `${pixelWidth * scale}px`
      canvas.style.height = `${pixelHeight * scale}px`
    }
    this.viewWidth = pixelWidth
    this.viewHeight = pixelHeight
    this.gl.viewport(0, 0, bufferWidth, bufferHeight)
  }

  tick(deltaSeconds: number): void {
    if (this.disposed || this.layers.length === 0) return
    const dt = Math.min(0.05, Math.max(0.001, deltaSeconds))
    this.time += dt
    this.smoothDriver(dt)
    this.updateSprings(dt)
    this.deform()
    this.draw()
  }

  captureFrame(): string | null {
    const canvas = this.gl.canvas
    return canvas instanceof HTMLCanvasElement ? canvas.toDataURL('image/png') : null
  }

  dispose(): void {
    this.disposed = true
    const { gl } = this
    for (const layer of this.layers) {
      gl.deleteBuffer(layer.vertexBuffer)
      gl.deleteBuffer(layer.indexBuffer)
      gl.deleteVertexArray(layer.vao)
      gl.deleteTexture(layer.texture)
    }
    gl.deleteProgram(this.program)
    this.layers = []
  }

  private smoothDriver(dt: number): void {
    const now = this.time * 1000
    const t = this.time
    const tgt: Anime25DDriver = { ...this.target }
    if (this.target.mouse && this.mouse.inside) {
      tgt.angleX = clamp(this.mouse.x * 0.9, -1, 1)
      tgt.angleY = clamp(-this.mouse.y * 0.7, -1, 1)
      tgt.eyeX = clamp(this.mouse.x * 1.2, -1, 1)
      tgt.eyeY = clamp(-this.mouse.y * 0.8, -1, 1)
    }
    if (this.target.idle) {
      tgt.angleX += 0.13 * Math.sin(t * 0.42) + 0.05 * Math.sin(t * 1.13)
      tgt.angleY += 0.08 * Math.sin(t * 0.31 + 1.7)
      tgt.angleZ += 0.07 * Math.sin(t * 0.23 + 0.5)
      tgt.body += 0.1 * Math.sin(t * 0.19 + 2.1)
    }
    if (this.target.rand) {
      if (this.time > this.nextRnd) {
        this.nextRnd = this.time + 1.4 + Math.random() * 2.6
        this.rnd.ax = (Math.random() * 2 - 1) * 0.55
        this.rnd.ay = (Math.random() * 2 - 1) * 0.4
        this.rnd.az = (Math.random() * 2 - 1) * 0.35
        this.rnd.bd = (Math.random() * 2 - 1) * 0.3
        this.rnd.ex = (Math.random() * 2 - 1) * 0.6
        this.rnd.ey = (Math.random() * 2 - 1) * 0.35
      }
      tgt.angleX = clamp(tgt.angleX + this.rnd.ax, -1, 1)
      tgt.angleY = clamp(tgt.angleY + this.rnd.ay, -1, 1)
      tgt.angleZ = clamp(tgt.angleZ + this.rnd.az, -1, 1)
      tgt.body = clamp(tgt.body + this.rnd.bd, -1, 1)
      tgt.eyeX = clamp(tgt.eyeX + this.rnd.ex, -1, 1)
      tgt.eyeY = clamp(tgt.eyeY + this.rnd.ey, -1, 1)
    }
    if (this.target.talk) {
      if (now > this.nextTalkState) {
        this.talkOn = !this.talkOn
        this.nextTalkState =
          now + (this.talkOn ? 1200 + Math.random() * 2200 : 600 + Math.random() * 1800)
      }
      if (this.talkOn && now > this.nextSyl) {
        this.nextSyl = now + 70 + Math.random() * 110
        this.talkTgt = Math.random() < 0.25 ? 0.04 : 0.25 + Math.random() * 0.75
      }
      if (!this.talkOn) this.talkTgt = 0
      this.talkV += (this.talkTgt - this.talkV) * Math.min(1, dt * 22)
      tgt.mouthOpen = Math.max(tgt.mouthOpen, this.talkV)
    }
    if (this.target.blink) {
      if (this.blinkT < 0 && this.time > this.nextBlink) {
        this.blinkT = 0
        this.nextBlink = this.time + 1.6 + Math.random() * 3.8
        if (Math.random() < 0.18) this.nextBlink = this.time + 0.28
      }
      if (this.blinkT >= 0) {
        this.blinkT += dt
        const elapsed = this.blinkT
        let open = 1
        if (elapsed < 0.08) open = 1 - elapsed / 0.08
        else if (elapsed < 0.42) open = 0
        else if (elapsed < 0.58) open = (elapsed - 0.42) / 0.16
        else {
          open = 1
          this.blinkT = -1
        }
        tgt.eyeOpenL = Math.min(tgt.eyeOpenL, open)
        tgt.eyeOpenR = Math.min(tgt.eyeOpenR, open)
      }
    }
    const rate = Math.min(1, dt * 14)
    const flags = ['idle', 'blink', 'rand', 'talk', 'mouse', 'phys'] as const
    for (const key of Object.keys(IDENTITY_DRIVER) as Array<keyof Anime25DDriver>) {
      if (flags.includes(key as (typeof flags)[number])) {
        this.current[key] = this.target[key] as never
        continue
      }
      const from = this.current[key] as number
      const to = tgt[key] as number
      ;(this.current[key] as number) = from + (to - from) * rate
    }
  }

  private updateSprings(dt: number): void {
    const { anchors } = this.playback
    const faceScale = anchors.faceScale
    const e = this.current
    const breath = 0.5 + 0.5 * Math.sin((this.time * Math.PI * 2) / 3.4)
    const bustTgt = (breath * 3.0 - e.angleY * 6.0 + e.body * 4.0) * faceScale
    const bounceAccel = -140 * (this.bounce.x - bustTgt) - 4.2 * this.bounce.v
    this.bounce.v += bounceAccel * dt
    this.bounce.x += this.bounce.v * dt
    this.bounce.dy = -(this.bounce.x - bustTgt) * 3.0
    if (!e.phys) return
    const headDX =
      (e.angleX * 14 + e.angleZ * 0.07 * (anchors.neckPivot.y - anchors.face.cy)) *
      faceScale
    const time = this.time
    const windAmp = e.idle ? 1 : 0
    for (const layer of this.layers) {
      if (!layer.springs) continue
      for (const spring of layer.springs) {
        const wind =
          windAmp *
          (1.8 * Math.sin(time * 0.8 + spring.phase) +
            1.0 * Math.sin(time * 1.9 + spring.phase * 2.3))
        const target = headDX + wind * faceScale
        stepHairSpring(spring.stiff, target, 70, 9, 2.2, dt)
        stepHairSpring(spring.soft, target, 16, 1.3, 3, dt)
      }
    }
  }

  private deform(): void {
    const A = this.playback.anchors
    const e = this.current
    const fs = A.faceScale
    const t = this.time
    const breath = 0.5 + 0.5 * Math.sin((t * Math.PI * 2) / 3.4)
    const breathHead = 0.5 + 0.5 * Math.sin((t * Math.PI * 2) / 3.4 - 0.6)
    const npx = A.neckPivot.x
    const npy = A.neckPivot.y
    const bpx = A.bodyPivot.x
    const bpy = A.bodyPivot.y
    const az = e.angleZ * 0.07
    const cz = Math.cos(az)
    const sz = Math.sin(az)
    const ab = e.body * 0.028
    const cb = Math.cos(ab)
    const sb = Math.sin(ab)
    const chestCx = npx
    const chestCy = A.neckBottom + (A.face.y1 - A.face.y0) * 0.6
    const chestRx = (A.face.x1 - A.face.x0) * 0.6
    const chestRy = (A.face.y1 - A.face.y0) * 0.45
    const mHalfW = (A.mouth.x1 - A.mouth.x0) / 2
    for (const layer of this.layers) {
      const rest = layer.rest
      const deformed = layer.deformed
      const vertexCount = rest.length / 2
      const source = layer.source
      const bn = layerBaseName(source.role)
      const eye = source.side === 'L' ? A.eyeL : source.side === 'R' ? A.eyeR : undefined
      const vOpen = source.side === 'L' ? e.eyeOpenL : e.eyeOpenR
      const bcx = source.x + source.w / 2
      const bcy = source.y + source.h / 2
      const isHead = source.group === 'head'
      const nS = layer.springs?.length ?? 0
      for (let vertex = 0; vertex < vertexCount; vertex += 1) {
        const index = vertex * 2
        let x = rest[index]
        let y = rest[index + 1]
        if (eye && bn === 'eye_close') {
          const scale = source.side === 'L' ? e.eyeScaleL : e.eyeScaleR
          if (scale !== 1) {
            const cxE = (eye.x0 + eye.x1) / 2
            const cyE = (eye.y0 + eye.y1) / 2
            x = cxE + (x - cxE) * scale
            y = cyE + (y - cyE) * scale
          }
        }
        if (bn === 'mouth_open' || bn === 'mouth_close') {
          if (e.mouthScale !== 1) {
            x = A.mouth.cx + (x - A.mouth.cx) * e.mouthScale
            y = A.mouth.cy + (y - A.mouth.cy) * e.mouthScale
          }
        }
        if (source.fade === 'eyeOpen' && eye) {
          if (bn === 'irides') {
            x = eye.icx + (x - eye.icx) * e.irisScale
            y = eye.icy + (y - eye.icy) * e.irisScale
            x += e.eyeX * 11 * fs
            y += e.eyeY * 6 * fs
            const tl = smoothstep((0.32 - vOpen) / 0.32)
            y = eye.closeY + (y - eye.closeY) * (1 - 0.8 * tl)
          } else {
            y = eye.closeY + (y - eye.closeY) * (1 - 0.85 * (1 - vOpen))
          }
        }
        if (source.fade === 'eyeClose' && eye) {
          y -= vOpen * 3
          y += e.eyeCY * 14 * fs
          const thE = e.eyeCAng * 0.3 * (source.side === 'L' ? 1 : -1)
          if (thE) {
            const ct = Math.cos(thE)
            const st = Math.sin(thE)
            const rx = x - bcx
            const ry = y - bcy
            x = bcx + rx * ct - ry * st
            y = bcy + rx * st + ry * ct
          }
        }
        if (bn === 'eyebrow') {
          y += (-e.brow * 9 + (1 - vOpen) * 3.5) * fs
          const th =
            (source.side === 'L'
              ? e.browAngL + e.browAngSym
              : e.browAngR - e.browAngSym) * 0.3
          if (th) {
            const ct = Math.cos(th)
            const st = Math.sin(th)
            const rx = x - bcx
            const ry = y - bcy
            x = bcx + rx * ct - ry * st
            y = bcy + rx * st + ry * ct
          }
        }
        if (source.fade === 'mouthOpen') {
          y = A.mouth.y0 + (y - A.mouth.y0) * (0.5 + 0.5 * e.mouthOpen)
          const q = Math.abs(x - A.mouth.cx) / (mHalfW + 4)
          y -= e.mouthForm * 6 * fs * (q ** 1.5 - 0.35)
        }
        if (source.fade === 'mouthClose') {
          y += e.mouthCY * 14 * fs
          const thM = e.mouthCAng * 0.35
          if (thM) {
            const ct = Math.cos(thM)
            const st = Math.sin(thM)
            const rx = x - A.mouth.cx
            const ry = y - A.mouth.cy
            x = A.mouth.cx + rx * ct - ry * st
            y = A.mouth.cy + rx * st + ry * ct
          }
        }
        if (bn === 'face' && y > A.mouth.cy) {
          y +=
            e.mouthOpen *
            6 *
            fs *
            smoothstep((y - A.mouth.cy) / (A.face.y1 - A.mouth.cy))
        }
        let hw = isHead ? 1 : source.group === 'body' ? 0.16 : 0
        if (bn === 'neck') {
          hw =
            0.55 *
            smoothstep(
              (A.neckBottom - y) / Math.max(1, A.neckBottom - A.neckTop),
            )
        }
        if (hw > 0) {
          const rx = x - npx
          const ry = y - npy
          const rx2 = rx * cz - ry * sz
          const ry2 = rx * sz + ry * cz
          x += (rx2 - rx) * hw
          y += (ry2 - ry) * hw
          const depthOffset =
            (source.depth - 1) * (layer.frontHairParallaxScale?.[vertex] ?? 1)
          x +=
            hw *
            fs *
            (e.angleX * (14 + 40 * depthOffset) +
              e.angleX * (npy - y) * 0.028)
          y +=
            hw *
            fs *
            (-e.angleY * (9 + 30 * depthOffset) -
              e.angleY * depthOffset * (y - A.face.cy) * 0.05)
        }
        y -= (source.group === 'body' ? breath * 2.0 : breathHead * 1.6) * fs
        if (bn === 'topwear' && y < chestCy) {
          y -=
            breath *
            2.2 *
            fs *
            smoothstep((chestCy - y) / (chestRy * 2))
        }
        if (bn === 'topwear') x = npx + (x - npx) * (1 + breath * 0.003)
        if (bn === 'topwear') {
          const gx = (x - chestCx) / chestRx
          const gy = (y - (chestCy + e.bustY * 70 * fs)) / chestRy
          y += this.bounce.dy * e.bust * Math.exp(-(gx * gx + gy * gy))
        }
        if (bn === 'handwear') {
          const w = smoothstep(((y - source.y) / source.h) * 1.15)
          y -= e.armY * 30 * fs * w
          y += e.armPos * 40 * fs
          x += e.armY * 6 * fs * w * (x < npx ? 1 : -1)
        }
        if (layer.bangWeights && layer.alongStrand) {
          const along = layer.alongStrand[vertex]
          const m = along ** 1.4 * 22 * fs
          x +=
            (e.bangL * layer.bangWeights[vertex * 3] +
              e.bangC * layer.bangWeights[vertex * 3 + 1] +
              e.bangR * layer.bangWeights[vertex * 3 + 2]) *
            m
        }
        if (
          nS &&
          layer.springs &&
          layer.strandWeights &&
          layer.alongStrand &&
          e.phys
        ) {
          const along = layer.alongStrand[vertex]
          const front = layer.frontHair
          const u = front ? Math.min(1, along * 1.6) : along
          const amp = u ** (front ? 1.8 : 2.1) * (front ? e.fhAmp : e.physAmp)
          const softMix = u ** 1.2 * (front ? e.fhSoft : e.soft)
          let dx = 0
          for (let strand = 0; strand < nS; strand += 1) {
            const weight = layer.strandWeights[vertex * nS + strand]
            if (weight < 0.001) continue
            const spring = layer.springs[strand]
            dx +=
              weight *
              (spring.stiff.dx * (1 - softMix) + spring.soft.dx * softMix)
          }
          const offset = dx * amp
          x += offset
          y += Math.abs(offset) * 0.12
        }
        deformed[index] = x
        deformed[index + 1] = y
      }
      if (Math.abs(ab) > 1e-4) {
        for (let index = 0; index < deformed.length; index += 2) {
          const rx = deformed[index] - bpx
          const ry = deformed[index + 1] - bpy
          deformed[index] = bpx + rx * cb - ry * sb
          deformed[index + 1] = bpy + rx * sb + ry * cb
        }
      }
      this.gl.bindBuffer(this.gl.ARRAY_BUFFER, layer.vertexBuffer)
      this.gl.bufferSubData(this.gl.ARRAY_BUFFER, 0, packVertices(deformed, layer.uvs))
    }
  }

  private draw(): void {
    const { gl } = this
    gl.clearColor(0, 0, 0, 0)
    gl.clear(gl.COLOR_BUFFER_BIT | gl.STENCIL_BUFFER_BIT)
    gl.useProgram(this.program)
    gl.uniform2f(this.viewLocation, this.viewWidth, this.viewHeight)
    gl.activeTexture(gl.TEXTURE0)
    for (const layer of this.layers) {
      const opacity = fadeOpacity(layer.source, this.current)
      if (opacity < 0.004 && !layer.source.name.startsWith('eyewhite')) continue
      const eyewhite = layer.source.name.startsWith('eyewhite')
      const iris = layer.source.name.startsWith('irides')
      gl.bindTexture(gl.TEXTURE_2D, layer.texture)
      gl.uniform1f(this.opacityLocation, opacity)
      gl.bindVertexArray(layer.vao)
      if (eyewhite) {
        gl.enable(gl.STENCIL_TEST)
        gl.stencilFunc(gl.ALWAYS, 1, 0xff)
        gl.stencilOp(gl.KEEP, gl.KEEP, gl.REPLACE)
        gl.uniform1f(this.cutLocation, 0.25)
        gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
        gl.disable(gl.STENCIL_TEST)
        gl.uniform1f(this.cutLocation, 0)
      } else if (iris) {
        gl.enable(gl.STENCIL_TEST)
        gl.stencilFunc(gl.EQUAL, 1, 0xff)
        gl.stencilOp(gl.KEEP, gl.KEEP, gl.KEEP)
        gl.uniform1f(this.cutLocation, 0)
        gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
        gl.disable(gl.STENCIL_TEST)
      } else {
        gl.uniform1f(this.cutLocation, 0)
        gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
      }
    }
    gl.bindVertexArray(null)
  }

  private createLayer(
    source: Anime25DPlaybackLayer,
    atlasImage: HTMLImageElement,
    layerIndex: number,
  ): GpuLayer {
    const cell = (source.phys ? 30 : 42) * Math.max(0.6, this.playback.pixelCanvas.width / 768)
    const cols = Math.max(2, Math.round(source.w / cell))
    const rows = Math.max(2, Math.round(source.h / cell))
    const rest = new Float32Array((cols + 1) * (rows + 1) * 2)
    const uvs = new Float32Array(rest.length)
    let cursor = 0
    for (let row = 0; row <= rows; row += 1) {
      const v = row / rows
      for (let col = 0; col <= cols; col += 1) {
        const u = col / cols
        rest[cursor] = source.x + source.w * u
        rest[cursor + 1] = source.y + source.h * v
        uvs[cursor] = u
        uvs[cursor + 1] = v
        cursor += 2
      }
    }
    const indices = new Uint16Array(cols * rows * 6)
    let write = 0
    for (let row = 0; row < rows; row += 1) {
      for (let col = 0; col < cols; col += 1) {
        const topLeft = row * (cols + 1) + col
        const topRight = topLeft + 1
        const bottomLeft = topLeft + cols + 1
        const bottomRight = bottomLeft + 1
        indices.set([topLeft, topRight, bottomLeft, topRight, bottomRight, bottomLeft], write)
        write += 6
      }
    }
    const packed = packVertices(rest, uvs)
    const { gl } = this
    const vao = gl.createVertexArray()
    const vertexBuffer = gl.createBuffer()
    const indexBuffer = gl.createBuffer()
    if (!vao || !vertexBuffer || !indexBuffer) {
      throw new Error('Anime2.5DRig mesh buffers failed')
    }
    const position = gl.getAttribLocation(this.program, 'a_pos')
    const uv = gl.getAttribLocation(this.program, 'a_uv')
    gl.bindVertexArray(vao)
    gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer)
    gl.bufferData(gl.ARRAY_BUFFER, packed, gl.DYNAMIC_DRAW)
    gl.enableVertexAttribArray(position)
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 16, 0)
    gl.enableVertexAttribArray(uv)
    gl.vertexAttribPointer(uv, 2, gl.FLOAT, false, 16, 8)
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer)
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW)
    gl.bindVertexArray(null)
    const vertexCount = (cols + 1) * (rows + 1)
    const hair = attachHairPhysics(
      source,
      rest,
      vertexCount,
      this.playback.anchors.face,
      typeof source.z === 'number' && Number.isFinite(source.z)
        ? source.z
        : layerIndex,
    )
    return {
      source,
      rest,
      deformed: rest.slice(),
      uvs,
      indices,
      cols,
      rows,
      vao,
      vertexBuffer,
      indexBuffer,
      indexCount: indices.length,
      texture: cropLayerTexture(gl, atlasImage, source),
      ...hair,
    }
  }
}

function cropLayerTexture(
  gl: WebGL2RenderingContext,
  atlas: HTMLImageElement,
  source: Anime25DPlaybackLayer,
): WebGLTexture {
  const sx = Math.max(0, Math.round(source.atlas.x * atlas.width))
  const sy = Math.max(0, Math.round(source.atlas.y * atlas.height))
  const sw = Math.max(1, Math.round(source.atlas.w * atlas.width))
  const sh = Math.max(1, Math.round(source.atlas.h * atlas.height))
  const crop = document.createElement('canvas')
  crop.width = sw
  crop.height = sh
  const context = crop.getContext('2d')
  if (!context) throw new Error('Anime2.5DRig layer crop failed')
  context.drawImage(atlas, sx, sy, sw, sh, 0, 0, sw, sh)
  const texture = gl.createTexture()
  if (!texture) throw new Error('Anime2.5DRig layer texture failed')
  gl.bindTexture(gl.TEXTURE_2D, texture)
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, crop)
  return texture
}

function layerBaseName(role: string): string {
  if (role === 'front-hair') return 'front hair'
  if (role === 'back-hair') return 'back hair'
  return role.replace(/-/g, '_')
}

function fadeOpacity(layer: Anime25DPlaybackLayer, driver: Anime25DDriver): number {
  if (!layer.fade) return 1
  if (layer.fade === 'eyeOpen' || layer.fade === 'eyeClose') {
    const open = layer.side === 'L' ? driver.eyeOpenL : driver.eyeOpenR
    const faded = smoothstep((open - (0.1 + driver.eyeEase * 0.45)) / 0.15)
    return layer.fade === 'eyeOpen' ? faded : 1 - faded
  }
  if (layer.fade === 'mouthOpen' || layer.fade === 'mouthClose') {
    const faded = smoothstep(
      (driver.mouthOpen - (0.05 + driver.mouthEase * 0.35)) / 0.12,
    )
    return layer.fade === 'mouthOpen' ? faded : 1 - faded
  }
  return 1
}

function attachHairPhysics(
  source: Anime25DPlaybackLayer,
  rest: Float32Array,
  vertexCount: number,
  face: Anime25DPlayback['anchors']['face'],
  layerZ: number,
): Pick<
  GpuLayer,
  | 'frontHair'
  | 'frontHairParallaxScale'
  | 'strandWeights'
  | 'alongStrand'
  | 'bangWeights'
  | 'springs'
> {
  const frontHair = source.role === 'front-hair'
  const strands = source.strands
  if (strands.length === 0) {
    return {
      frontHair,
      frontHairParallaxScale: null,
      strandWeights: null,
      alongStrand: null,
      bangWeights: null,
      springs: null,
    }
  }
  const strandCount = strands.length
  let spacing = 120
  if (strandCount > 1) {
    const gaps = []
    for (let index = 1; index < strandCount; index += 1) {
      gaps.push(strands[index].x - strands[index - 1].x)
    }
    gaps.sort((left, right) => left - right)
    spacing = gaps[gaps.length >> 1]
  }
  const sigma = spacing * 0.6
  const frontHairParallaxScale = frontHair
    ? new Float32Array(vertexCount)
    : null
  const strandWeights = new Float32Array(vertexCount * strandCount)
  const alongStrand = new Float32Array(vertexCount)
  for (let vertex = 0; vertex < vertexCount; vertex += 1) {
    const x = rest[vertex * 2]
    const y = rest[vertex * 2 + 1]
    if (frontHairParallaxScale) {
      frontHairParallaxScale[vertex] = frontHairUpperParallaxScale(
        y,
        source,
        face,
      )
    }
    let total = 0
    for (let strand = 0; strand < strandCount; strand += 1) {
      const weight = Math.exp(-(((x - strands[strand].x) / sigma) ** 2))
      strandWeights[vertex * strandCount + strand] = weight
      total += weight
    }
    let rootY = 0
    let tipY = 0
    if (total > 1e-6) {
      for (let strand = 0; strand < strandCount; strand += 1) {
        const weight = strandWeights[vertex * strandCount + strand] / total
        strandWeights[vertex * strandCount + strand] = weight
        rootY += weight * strands[strand].rootY
        tipY += weight * strands[strand].tipY
      }
    } else {
      strandWeights[vertex * strandCount] = 1
      rootY = strands[0].rootY
      tipY = strands[0].tipY
    }
    alongStrand[vertex] = clamp((y - rootY) / Math.max(1, tipY - rootY), 0, 1)
  }
  let bangWeights: Float32Array | null = null
  if (frontHair) {
    const faceWidth = face.x1 - face.x0
    const leftSplit = face.cx - faceWidth * 0.22
    const rightSplit = face.cx + faceWidth * 0.22
    bangWeights = new Float32Array(vertexCount * 3)
    for (let vertex = 0; vertex < vertexCount; vertex += 1) {
      const x = rest[vertex * 2]
      const left = smoothstep((x - leftSplit) / 36 + 0.5)
      const right = smoothstep((x - rightSplit) / 36 + 0.5)
      bangWeights[vertex * 3] = 1 - left
      bangWeights[vertex * 3 + 1] = left * (1 - right)
      bangWeights[vertex * 3 + 2] = right
    }
  }
  return {
    frontHair,
    frontHairParallaxScale,
    strandWeights,
    alongStrand,
    bangWeights,
    springs: strands.map((_, index) => ({
      stiff: { x: 0, v: 0, dx: 0 },
      soft: { x: 0, v: 0, dx: 0 },
      phase: index * 1.37 + layerZ,
    })),
  }
}

function stepHairSpring(
  spring: HairSpring,
  target: number,
  stiffness: number,
  damping: number,
  pull: number,
  dt: number,
): void {
  const acceleration = -stiffness * (spring.x - target) - damping * spring.v
  spring.v += acceleration * dt
  spring.x += spring.v * dt
  spring.dx = -(spring.x - target) * pull
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function packVertices(positions: Float32Array, uvs: Float32Array): Float32Array {
  const packed = new Float32Array(positions.length * 2)
  for (let index = 0; index < positions.length; index += 2) {
    const write = index * 2
    packed[write] = positions[index]
    packed[write + 1] = positions[index + 1]
    packed[write + 2] = uvs[index]
    packed[write + 3] = uvs[index + 1]
  }
  return packed
}

function compileProgram(gl: WebGL2RenderingContext): WebGLProgram {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER)
  const fragment = compileShader(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER)
  const program = gl.createProgram()
  if (!program) throw new Error('Anime2.5DRig program failed')
  gl.attachShader(program, vertex)
  gl.attachShader(program, fragment)
  gl.linkProgram(program)
  gl.deleteShader(vertex)
  gl.deleteShader(fragment)
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(gl.getProgramInfoLog(program) || 'Anime2.5DRig link failed')
  }
  return program
}

function compileShader(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type)
  if (!shader) throw new Error('Anime2.5DRig shader failed')
  gl.shaderSource(shader, source)
  gl.compileShader(shader)
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(shader) || 'Anime2.5DRig shader compile failed'
    gl.deleteShader(shader)
    throw new Error(log)
  }
  return shader
}

function requiredUniform(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  name: string,
): WebGLUniformLocation {
  const location = gl.getUniformLocation(program, name)
  if (!location) throw new Error(`Missing uniform ${name}`)
  return location
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image()
    image.crossOrigin = 'anonymous'
    image.onload = () => resolve(image)
    image.onerror = () => reject(new Error('Anime2.5DRig atlas failed to load'))
    image.src = url
  })
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
