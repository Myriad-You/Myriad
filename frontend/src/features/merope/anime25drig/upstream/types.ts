/** Anime2.5DRig `lib/rigger.js` boundary. Independent of Myriad post-processing types. */

export type UpstreamPixelArray =
  | Uint8ClampedArray
  | Uint8Array
  | Uint16Array
  | Float32Array

export interface UpstreamPixelImage {
  width: number
  height: number
  data: UpstreamPixelArray
}

export interface UpstreamRgbaImage extends UpstreamPixelImage {
  data: Uint8ClampedArray
}

export interface UpstreamPsdLayer {
  name?: string
  left?: number
  top?: number
  right?: number
  bottom?: number
  imageData?: UpstreamPixelImage
  canvas?: unknown
}

export interface UpstreamPsd {
  width: number
  height: number
  children?: UpstreamPsdLayer[]
}

export interface UpstreamGenericParts {
  eyeL?: UpstreamRgbaImage | null
  eyeR?: UpstreamRgbaImage | null
  mouth?: UpstreamRgbaImage | null
}

export interface UpstreamGenericPartsApi {
  get: (key: string) => UpstreamRgbaImage | null
}

export interface UpstreamRiggerOptions {
  generic?: UpstreamGenericParts
}

export type UpstreamLayerGroup = 'head' | 'body'
export type UpstreamLayerPhysics = 'hair' | null
export type UpstreamLayerSide = 'L' | 'R' | null
export type UpstreamLayerFade =
  | 'eyeOpen'
  | 'eyeClose'
  | 'mouthOpen'
  | 'mouthClose'
  | null

export interface UpstreamHairStrand {
  x: number
  rootY: number
  tipY: number
}

export interface UpstreamRigLayer {
  name: string
  x: number
  y: number
  w: number
  h: number
  z: number
  depth: number
  group: UpstreamLayerGroup
  phys: UpstreamLayerPhysics
  fade: UpstreamLayerFade
  side: UpstreamLayerSide
  strands: UpstreamHairStrand[] | null
  synthetic?: true
  img: UpstreamRgbaImage
}

export interface UpstreamFaceAnchor {
  cx: number
  cy: number
  x0: number
  x1: number
  y0: number
  y1: number
}

export interface UpstreamEyeAnchor {
  x0: number
  x1: number
  y0: number
  y1: number
  icx: number
  icy: number
  closeY: number
}

export interface UpstreamMouthAnchor {
  x0: number
  x1: number
  y0: number
  y1: number
  cx: number
  cy: number
}

export interface UpstreamRigAnchors {
  face: UpstreamFaceAnchor
  eyeL?: UpstreamEyeAnchor
  eyeR?: UpstreamEyeAnchor
  mouth: UpstreamMouthAnchor
  neckPivot: { cx: number; cy: number }
  neckTop: number
  neckBottom: number
  bodyPivot: { cx: number; cy: number }
  faceScale: number
  hairRootY: number
}

export interface UpstreamRig {
  canvas: { w: number; h: number }
  layers: UpstreamRigLayer[]
  anchors: UpstreamRigAnchors
  warnings: string[]
  synth: { eye: boolean; mouth: boolean }
}

export interface UpstreamCleanStats {
  noisy: number
  layers: number
}

export interface UpstreamComponentLabels {
  lab: Int32Array
  count: number
  sizes: number[]
  sumX: number[]
}

export interface UpstreamPeak {
  x: number
  prom: number
}

export interface UpstreamRiggerInternals {
  findPeaks: (
    values: ArrayLike<number>,
    minDist: number,
    minProm: number,
  ) => UpstreamPeak[]
  detectStrands: (
    alpha: Uint8Array,
    width: number,
    height: number,
    minSep: number,
    wanted: number,
  ) => UpstreamHairStrand[]
  labelComponents: (
    alpha: Uint8Array,
    width: number,
    height: number,
    threshold: number,
  ) => UpstreamComponentLabels
  cleanAlpha: (
    alpha: Uint8Array,
    width: number,
    height: number,
    minPixels: number,
  ) => Uint8Array
}

export interface UpstreamRiggerApi {
  buildRig: (psd: UpstreamPsd, options?: UpstreamRiggerOptions) => UpstreamRig
  cleanPsdLayers: (psd: UpstreamPsd) => UpstreamCleanStats
  normName: (value: string) => string
  baseName: (value: string) => string
  flattenPsdToImg: (psd: UpstreamPsd) => UpstreamRgbaImage | null
  splitImgLR: (
    image: UpstreamRgbaImage,
  ) => { l: UpstreamRgbaImage; r: UpstreamRgbaImage } | null
  _internals: UpstreamRiggerInternals
}

/**
 * Mutable runtime state modeled after the pinned upstream WebGL loop. These
 * types stay separate from Myriad's extended driver so module tests can cover
 * the compatibility behavior without extension fields.
 */
export interface UpstreamRuntimeParameters {
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
}

export interface UpstreamRuntimeExpression extends UpstreamRuntimeParameters {
  breath: number
  breathHead: number
}

export interface UpstreamRuntimeSpringValue {
  x: number
  v: number
  dx: number
}

export interface UpstreamRuntimeStrandSpring {
  stiff: UpstreamRuntimeSpringValue
  soft: UpstreamRuntimeSpringValue
  phase: number
}

export interface UpstreamRuntimeBustSpring {
  x: number
  v: number
  dy: number
}

export interface UpstreamRuntimeLayer {
  name: string
  bn: string
  group: UpstreamLayerGroup
  side: UpstreamLayerSide
  fade: UpstreamLayerFade
  x: number
  y: number
  w: number
  h: number
  depth: number
  base: Float32Array
  cur: Float32Array
  strands?: UpstreamHairStrand[] | null
  sw?: Float32Array
  su?: Float32Array
  spr?: UpstreamRuntimeStrandSpring[]
  bw?: Float32Array
}

/**
 * CPU-visible result of the mesh setup performed by upstream `applyRig`.
 * GPU handles are deliberately excluded; `uv` and `indices` preserve the
 * payloads that the original function uploads to WebGL buffers.
 */
export interface UpstreamRuntimeBoundLayer extends UpstreamRuntimeLayer {
  z: number
  phys: UpstreamLayerPhysics
  synthetic?: true
  uv: Float32Array
  indices: Uint16Array
  nIdx: number
}

export interface UpstreamRuntimeRigBinding {
  canvas: { w: number; h: number }
  anchors: UpstreamRigAnchors
  faceScale: number
  neckPivot: { cx: number; cy: number }
  bodyPivot: { cx: number; cy: number }
  faceCenter: { x: number; y: number }
  chest: { cx: number; cy: number; rx: number; ry: number }
  layers: UpstreamRuntimeBoundLayer[]
}

export type UpstreamRuntimeStencilMode = 'none' | 'write' | 'test'

export interface UpstreamRuntimeDrawCommand {
  layerIndex: number
  name: string
  alpha: number
  alphaCut: number
  stencil: UpstreamRuntimeStencilMode
}

export interface UpstreamRuntimeFrame {
  anchors: UpstreamRigAnchors
  faceScale: number
  neckPivot: { cx: number; cy: number }
  bodyPivot: { cx: number; cy: number }
  faceCenter: { x: number; y: number }
  chest: { cx: number; cy: number; rx: number; ry: number }
  physicsEnabled: boolean
  bustDisplacement: number
}

export interface UpstreamRuntimeTickAutomation {
  idle: boolean
  blink: boolean
}

export interface UpstreamRuntimeTickState {
  lastTimeMs: number
  blinkElapsed: number
  nextBlinkAtMs: number
  cameraPhysicsScale: number
  current: UpstreamRuntimeParameters
  expression: UpstreamRuntimeExpression
  bounce: UpstreamRuntimeBustSpring
}

export interface UpstreamRuntimeTickInput {
  nowMs: number
  target: Readonly<UpstreamRuntimeParameters>
  automation: Readonly<UpstreamRuntimeTickAutomation>
  cameraLive: boolean
  layers: readonly UpstreamRuntimeLayer[]
  frame: Readonly<UpstreamRuntimeFrame>
  random: () => number
}
