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
  | 'mouthOpen'
  | 'mouthClose'

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
  face: { x0: number; y0: number; x1: number; y1: number; cx: number; cy: number }
  neckPivot: { x: number; y: number }
  neckTop: number
  neckBottom: number
  bodyPivot: { x: number; y: number }
  mouth: { x0: number; y0: number; x1: number; y1: number; cx: number; cy: number }
  faceScale: number
  eyeL?: Anime25DEyeAnchor
  eyeR?: Anime25DEyeAnchor
}

export type Anime25DChestProfileSource =
  'ai-vision' | 'geometry-fallback' | 'gender-policy'

/** Import-time chest region. Runtime consumes this without further AI work. */
export interface Anime25DChestProfile {
  version: 1
  enabled: boolean
  source: Anime25DChestProfileSource
  centerX: number
  centerY: number
  radiusX: number
  radiusY: number
  visibleScale: number
  motionScale: number
  frequencyScale: number
  confidence: number
}

export interface Anime25DPlayback {
  kind: typeof ANIME25D_PLAYBACK_KIND
  version: number
  engine: typeof ANIME25D_PROJECT_NAME
  engineUrl: typeof ANIME25D_PROJECT_URL
  license: typeof ANIME25D_LICENSE
  copyright: string
  pixelCanvas: { width: number; height: number }
  layers: Anime25DPlaybackLayer[]
  anchors: Anime25DPlaybackAnchors
  chestProfile?: Anime25DChestProfile
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
    (chestProfile === undefined ||
      isAnime25DChestProfile(chestProfile, canvas.width, canvas.height))
  )
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
    profile.version === 1 &&
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
