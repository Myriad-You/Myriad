import {
  ANIME25D_COPYRIGHT,
  ANIME25D_LICENSE,
  ANIME25D_PLAYBACK_KIND,
  ANIME25D_PLAYBACK_VERSION,
  ANIME25D_PROJECT_NAME,
  ANIME25D_PROJECT_URL,
} from './credit'

export type Anime25DFade =
  | 'eyeOpen'
  | 'eyeClose'
  | 'eyeDizzy'
  | 'eyeSqueeze'
  | 'eyeCry'
  | 'eyeSilly'
  | 'lovestruckHeart'
  | 'lovestruckFace'
  | 'lovestruckDrool'
  | 'maniacEyeShadow'
  | 'maniacMouthShadow'
  | 'angerMark'
  | 'speechlessSweat'
  | 'mouthOpen'
  | 'mouthWide'
  | 'mouthRound'
  | 'mouthNarrow'
  | 'mouthClose'
  | 'mouthCry'
  | 'mouthManiac'
  | 'mouthSilly'

export type Anime25DGroup = 'head' | 'body'

export interface Anime25DStrand {
  x: number
  rootY: number
  tipY: number
}

export interface Anime25DPlaybackLayer {
  name: string
  role: string
  /** Draw-order index from Anime2.5DRig (`L.z`). Hair spring phase uses this. */
  z?: number
  depth: number
  group: Anime25DGroup
  phys: 'hair' | null
  fade: Anime25DFade | null
  side: 'L' | 'R' | null
  x: number
  y: number
  w: number
  h: number
  atlas: { x: number; y: number; w: number; h: number }
  strands: Anime25DStrand[]
}

export interface Anime25DEyeAnchor {
  x0: number
  y0: number
  x1: number
  y1: number
  icx: number
  icy: number
  closeY: number
}

export interface Anime25DPlaybackAnchors {
  face: {
    x0: number
    y0: number
    x1: number
    y1: number
    cx: number
    cy: number
  }
  neckPivot: { x: number; y: number }
  neckTop: number
  neckBottom: number
  bodyPivot: { x: number; y: number }
  mouth: {
    x0: number
    y0: number
    x1: number
    y1: number
    cx: number
    cy: number
  }
  faceScale: number
  eyeL?: Anime25DEyeAnchor
  eyeR?: Anime25DEyeAnchor
}

export type Anime25DChestProfileSource =
  'ai-vision' | 'geometry-fallback' | 'gender-policy'

/** Import-time chest region. Runtime consumes this without further AI work. */
export interface Anime25DChestProfile {
  version: 2
  enabled: boolean
  source: Anime25DChestProfileSource
  centerX: number
  centerY: number
  radiusX: number
  radiusY: number
  visibleScale: number
  motionScale: number
  frequencyScale: number
  /** 0 is freely moving; 1 is visually locked to structured support. */
  supportScale: number
  /** Fraction of local soft-tissue motion visible on the outer garment. */
  garmentMotionScale: number
  confidence: number
}

export interface Anime25DShellCurvePoint {
  /** Vertical progress from forehead (0) to chin (1). */
  v: number
  /** Additional normalized depth outside the base ellipsoid. */
  z: number
}

export interface Anime25DShellEllipsoid {
  centerX: number
  centerY: number
  radiusX: number
  radiusY: number
  radiusZ: number
}

export interface Anime25DHairlinePinProfile {
  enabled: boolean
  /**
   * Imported assets follow their strand roots unless an authored profile uses
   * the fork's calibrated rectangle.
   */
  mode: 'rectangle' | 'strand-roots'
  /** Centre and half extents expressed in head-radius units. */
  centerX: number
  centerY: number
  halfWidth: number
  halfHeight: number
  feather: number
}

export interface Anime25DTorsoShellProfile {
  enabled: boolean
  /** Local share multiplied by the parent shell blend. */
  blend: number
  /** Vertical elliptic-cylinder axis and radii in playback pixels. */
  centerX: number
  radiusX: number
  radiusZ: number
  /**
   * How much of a head turn this body comes around with. The fork carries the
   * same per-model control; a stiff pose or a structured garment turns less
   * than a soft one at the same head angle.
   *
   * Optional for manifests compiled before the torso follow became per-model.
   */
  yawFollowScale?: number
}

export interface Anime25DShellProfile {
  version: 1
  source: 'anchor-derived' | 'authored'
  enabled: boolean
  blend: number
  head: Anime25DShellEllipsoid
  faceProfile: {
    enabled: boolean
    startY: number
    endY: number
    points: Anime25DShellCurvePoint[]
  }
  hair: Anime25DShellEllipsoid & {
    frontGap: number
    frontBulge: number
    backDepth: number
    crownRound: number
    hairlinePin: Anime25DHairlinePinProfile
  }
  torso: Anime25DTorsoShellProfile
}

export type Anime25DMouthMaterial =
  | 'mouthClose'
  | 'mouthOpen'
  | 'mouthWide'
  | 'mouthRound'
  | 'mouthNarrow'
  | 'mouthManiac'

export interface Anime25DMouthSilhouette {
  material: Anime25DMouthMaterial
  centerX: number
  centerY: number
  width: number
  height: number
  leftCornerY: number
  rightCornerY: number
  fillRatio: number
  aperture: number
}

export interface Anime25DMouthBridgeTuning {
  first: Anime25DMouthMaterial
  second: Anime25DMouthMaterial
  widthScale: number
  heightScale: number
  neutralization: number
  centerOffsetX: number
  centerOffsetY: number
}

/** Import-time alpha-contour analysis. Runtime performs no image sampling. */
export interface Anime25DMouthProfile {
  version: 1
  source: 'alpha-contour' | 'bounds-fallback'
  silhouettes: Anime25DMouthSilhouette[]
  bridges: Anime25DMouthBridgeTuning[]
}

export interface Anime25DPlayback {
  kind: typeof ANIME25D_PLAYBACK_KIND
  version: typeof ANIME25D_PLAYBACK_VERSION
  engine: typeof ANIME25D_PROJECT_NAME
  engineUrl: typeof ANIME25D_PROJECT_URL
  license: typeof ANIME25D_LICENSE
  copyright: string
  pixelCanvas: { width: number; height: number }
  layers: Anime25DPlaybackLayer[]
  anchors: Anime25DPlaybackAnchors
  mouthProfile: Anime25DMouthProfile
  chestProfile: Anime25DChestProfile
  shellProfile: Anime25DShellProfile
}

export function anime25DPlaybackSource(): Pick<
  Anime25DPlayback,
  'kind' | 'version' | 'engine' | 'engineUrl' | 'license' | 'copyright'
> {
  return {
    kind: ANIME25D_PLAYBACK_KIND,
    version: ANIME25D_PLAYBACK_VERSION,
    engine: ANIME25D_PROJECT_NAME,
    engineUrl: ANIME25D_PROJECT_URL,
    license: ANIME25D_LICENSE,
    copyright: ANIME25D_COPYRIGHT,
  }
}

export function isAnime25DPlayback(value: unknown): value is Anime25DPlayback {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  const canvas = record.pixelCanvas as Record<string, unknown> | undefined
  const chestProfile = record.chestProfile
  const shellProfile = record.shellProfile
  const mouthProfile = record.mouthProfile
  return (
    record.kind === ANIME25D_PLAYBACK_KIND &&
    record.version === ANIME25D_PLAYBACK_VERSION &&
    record.engine === ANIME25D_PROJECT_NAME &&
    Array.isArray(record.layers) &&
    record.layers.length > 0 &&
    typeof canvas?.width === 'number' &&
    typeof canvas.height === 'number' &&
    canvas.width > 0 &&
    canvas.height > 0 &&
    Boolean(record.anchors) &&
    typeof record.anchors === 'object' &&
    isAnime25DMouthProfile(mouthProfile, canvas.width, canvas.height) &&
    isAnime25DChestProfile(chestProfile, canvas.width, canvas.height) &&
    isAnime25DShellProfile(shellProfile, canvas.width, canvas.height)
  )
}

export function isAnime25DShellProfile(
  value: unknown,
  canvasWidth: number,
  canvasHeight: number,
): value is Anime25DShellProfile {
  if (!value || typeof value !== 'object') return false
  const profile = value as Record<string, unknown>
  const head = profile.head as Record<string, unknown> | undefined
  const faceProfile = profile.faceProfile as Record<string, unknown> | undefined
  const hair = profile.hair as Record<string, unknown> | undefined
  const pin = hair?.hairlinePin as Record<string, unknown> | undefined
  const torso = profile.torso as Record<string, unknown> | undefined
  const points = faceProfile?.points
  if (
    profile.version !== 1 ||
    (profile.source !== 'anchor-derived' && profile.source !== 'authored') ||
    typeof profile.enabled !== 'boolean' ||
    !numberInRange(profile.blend, 0, 1) ||
    !isShellEllipsoid(head, canvasWidth, canvasHeight) ||
    typeof faceProfile?.enabled !== 'boolean' ||
    !numberInRange(faceProfile.startY, 0, canvasHeight) ||
    !numberInRange(faceProfile.endY, 0, canvasHeight * 1.2) ||
    (faceProfile.endY as number) <= (faceProfile.startY as number) ||
    !Array.isArray(points) ||
    points.length !== 5 ||
    !isShellEllipsoid(hair, canvasWidth, canvasHeight) ||
    !numberInRange(hair?.frontGap, 0, 0.6) ||
    !numberInRange(hair?.frontBulge, 0, 1.5) ||
    !numberInRange(hair?.backDepth, 0, 1) ||
    !numberInRange(hair?.crownRound, 0, 1) ||
    typeof pin?.enabled !== 'boolean' ||
    (pin.mode !== 'rectangle' && pin.mode !== 'strand-roots') ||
    !numberInRange(pin.centerX, -1.5, 1.5) ||
    !numberInRange(pin.centerY, -1.5, 1.5) ||
    !numberInRange(pin.halfWidth, 0.01, 2) ||
    !numberInRange(pin.halfHeight, 0.01, 2) ||
    !numberInRange(pin.feather, 0, 0.5) ||
    typeof torso?.enabled !== 'boolean' ||
    !numberInRange(torso?.blend, 0, 1) ||
    !numberInRange(torso?.centerX, 0, canvasWidth) ||
    !numberInRange(torso?.radiusX, 1, canvasWidth) ||
    !numberInRange(torso?.radiusZ, 1, canvasWidth) ||
    (torso?.yawFollowScale !== undefined &&
      !numberInRange(torso.yawFollowScale, 0, 1))
  ) {
    return false
  }
  let previousV = -1
  for (const point of points) {
    if (!point || typeof point !== 'object') return false
    const curvePoint = point as Record<string, unknown>
    if (
      !numberInRange(curvePoint.v, 0, 1) ||
      !numberInRange(curvePoint.z, -0.4, 0.8) ||
      (curvePoint.v as number) <= previousV
    ) {
      return false
    }
    previousV = curvePoint.v as number
  }
  return true
}

function isShellEllipsoid(
  value: Record<string, unknown> | undefined,
  canvasWidth: number,
  canvasHeight: number,
): boolean {
  return Boolean(
    value &&
    numberInRange(value.centerX, 0, canvasWidth) &&
    numberInRange(value.centerY, 0, canvasHeight) &&
    numberInRange(value.radiusX, 1, canvasWidth) &&
    numberInRange(value.radiusY, 1, canvasHeight) &&
    numberInRange(value.radiusZ, 1, canvasWidth),
  )
}

const MOUTH_MATERIALS: readonly Anime25DMouthMaterial[] = [
  'mouthClose',
  'mouthOpen',
  'mouthWide',
  'mouthRound',
  'mouthNarrow',
  'mouthManiac',
]

function isAnime25DMouthProfile(
  value: unknown,
  canvasWidth: number,
  canvasHeight: number,
): value is Anime25DMouthProfile {
  if (!value || typeof value !== 'object') return false
  const profile = value as Record<string, unknown>
  if (
    profile.version !== 1 ||
    (profile.source !== 'alpha-contour' &&
      profile.source !== 'bounds-fallback') ||
    !Array.isArray(profile.silhouettes) ||
    profile.silhouettes.length !== MOUTH_MATERIALS.length ||
    !Array.isArray(profile.bridges) ||
    profile.bridges.length !== 15
  ) {
    return false
  }
  const materials = new Set<Anime25DMouthMaterial>()
  for (const value of profile.silhouettes) {
    if (!value || typeof value !== 'object') return false
    const silhouette = value as Record<string, unknown>
    if (
      !MOUTH_MATERIALS.includes(silhouette.material as Anime25DMouthMaterial) ||
      materials.has(silhouette.material as Anime25DMouthMaterial) ||
      !numberInRange(silhouette.centerX, -canvasWidth, canvasWidth * 2) ||
      !numberInRange(silhouette.centerY, -canvasHeight, canvasHeight * 2) ||
      !numberInRange(silhouette.width, 0.25, canvasWidth) ||
      !numberInRange(silhouette.height, 0.25, canvasHeight) ||
      !numberInRange(silhouette.leftCornerY, -canvasHeight, canvasHeight * 2) ||
      !numberInRange(
        silhouette.rightCornerY,
        -canvasHeight,
        canvasHeight * 2,
      ) ||
      !numberInRange(silhouette.fillRatio, 0, 1) ||
      !numberInRange(silhouette.aperture, 0, 4)
    ) {
      return false
    }
    materials.add(silhouette.material as Anime25DMouthMaterial)
  }
  const pairs = new Set<string>()
  for (const value of profile.bridges) {
    if (!value || typeof value !== 'object') return false
    const bridge = value as Record<string, unknown>
    const first = bridge.first as Anime25DMouthMaterial
    const second = bridge.second as Anime25DMouthMaterial
    if (
      !MOUTH_MATERIALS.includes(first) ||
      !MOUTH_MATERIALS.includes(second) ||
      first === second
    ) {
      return false
    }
    const pair = [first, second].sort().join(':')
    if (
      pairs.has(pair) ||
      !numberInRange(bridge.widthScale, 0.75, 1) ||
      !numberInRange(bridge.heightScale, 0.65, 1) ||
      !numberInRange(bridge.neutralization, 0, 1) ||
      !numberInRange(bridge.centerOffsetX, -canvasWidth / 4, canvasWidth / 4) ||
      !numberInRange(bridge.centerOffsetY, -canvasHeight / 4, canvasHeight / 4)
    ) {
      return false
    }
    pairs.add(pair)
  }
  return materials.size === MOUTH_MATERIALS.length && pairs.size === 15
}

function isAnime25DChestProfile(
  value: unknown,
  canvasWidth: number,
  canvasHeight: number,
): value is Anime25DChestProfile {
  if (!value || typeof value !== 'object') return false
  const profile = value as Record<string, unknown>
  const sources: Anime25DChestProfileSource[] = [
    'ai-vision',
    'geometry-fallback',
    'gender-policy',
  ]
  return (
    profile.version === 2 &&
    typeof profile.enabled === 'boolean' &&
    typeof profile.source === 'string' &&
    sources.includes(profile.source as Anime25DChestProfileSource) &&
    numberInRange(profile.centerX, 0, canvasWidth) &&
    numberInRange(profile.centerY, 0, canvasHeight) &&
    numberInRange(profile.radiusX, 1, canvasWidth / 2) &&
    numberInRange(profile.radiusY, 1, canvasHeight / 2) &&
    numberInRange(profile.visibleScale, 0, 1) &&
    numberInRange(profile.motionScale, 0, 1.25) &&
    numberInRange(profile.frequencyScale, 0.75, 1.25) &&
    numberInRange(profile.supportScale, 0, 1) &&
    numberInRange(profile.garmentMotionScale, 0, 1) &&
    numberInRange(profile.confidence, 0, 1)
  )
}

function numberInRange(
  value: unknown,
  minimum: number,
  maximum: number,
): boolean {
  return (
    typeof value === 'number' &&
    Number.isFinite(value) &&
    value >= minimum &&
    value <= maximum
  )
}
