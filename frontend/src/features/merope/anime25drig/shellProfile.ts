import type {
  Anime25DPlayback,
  Anime25DPlaybackAnchors,
  Anime25DPlaybackLayer,
  Anime25DShellCurvePoint,
  Anime25DShellEllipsoid,
  Anime25DShellProfile,
  Anime25DTorsoShellProfile,
} from './types'

const DEFAULT_CURVE: readonly Anime25DShellCurvePoint[] = [
  { v: 0.06, z: 0.1 },
  { v: 0.42, z: 0.02 },
  { v: 0.62, z: 0.3 },
  { v: 0.78, z: 0.06 },
  { v: 0.97, z: 0.14 },
]

type ShellProfileSource = Pick<Anime25DPlayback, 'anchors' | 'layers'>

export function deriveAnime25DShellProfile(
  playback: Readonly<ShellProfileSource>,
): Anime25DShellProfile {
  const { anchors, layers } = playback
  const faceWidth = Math.max(1, anchors.face.x1 - anchors.face.x0)
  const faceHeight = Math.max(1, anchors.face.y1 - anchors.face.y0)
  const head = {
    centerX: anchors.face.cx,
    centerY: anchors.face.y0 + faceHeight * 0.45,
    radiusX: faceWidth * 0.62,
    radiusY: faceHeight * 0.72,
    radiusZ: faceWidth * 0.45,
  }
  const hair = deriveHairShellEllipsoid(head, layers)
  const profileStartY = anchors.face.y0
  const profileEndY = anchors.face.y1 + faceHeight * 0.12
  return {
    version: 1,
    source: 'anchor-derived',
    enabled: true,
    blend: 0.5,
    head,
    faceProfile: {
      enabled: true,
      startY: profileStartY,
      endY: profileEndY,
      points: deriveFaceCurve(anchors, layers, profileStartY, profileEndY),
    },
    hair: {
      ...hair,
      frontGap: 0.18,
      frontBulge: 1,
      backDepth: 0.35,
      crownRound: deriveCrownRound(head, layers),
      hairlinePin: {
        enabled: layers.some((layer) => layer.role === 'front-hair'),
        mode: 'strand-roots',
        centerX: 0,
        centerY: -0.45,
        halfWidth: 1.1,
        halfHeight: 0.32,
        feather: 0.06,
      },
    },
    torso: deriveAnime25DTorsoShellProfile(playback),
  }
}

/**
 * Fit the default scalp shell gently toward trustworthy hair-layer bounds.
 * The narrow clamps intentionally keep a noisy accessory or oversized layer
 * from changing the renderer's geometry model.
 */
function deriveHairShellEllipsoid(
  head: Anime25DShellEllipsoid,
  layers: readonly Anime25DPlaybackLayer[],
): Anime25DShellEllipsoid {
  const fallback = {
    centerX: head.centerX,
    centerY: head.centerY - head.radiusY * 0.06,
    radiusX: head.radiusX * 1.1,
    radiusY: head.radiusY * 1.1,
    radiusZ: head.radiusZ * 1.05,
  }
  const hairLayers = layers.filter(
    (layer) =>
      (layer.role === 'front-hair' || layer.role === 'back-hair') &&
      layer.w > 0 &&
      layer.h > 0 &&
      Number.isFinite(layer.x) &&
      Number.isFinite(layer.y),
  )
  if (hairLayers.length === 0) return fallback
  const left = Math.min(...hairLayers.map((layer) => layer.x))
  const right = Math.max(...hairLayers.map((layer) => layer.x + layer.w))
  const top = Math.min(...hairLayers.map((layer) => layer.y))
  const bottom = Math.max(...hairLayers.map((layer) => layer.y + layer.h))
  const boundedCenterX = clamp(
    (left + right) / 2,
    head.centerX - head.radiusX * 0.12,
    head.centerX + head.radiusX * 0.12,
  )
  const boundedTop = clamp(
    top,
    head.centerY - head.radiusY * 1.28,
    head.centerY - head.radiusY * 0.65,
  )
  const boundedBottom = clamp(
    bottom,
    head.centerY + head.radiusY * 0.55,
    head.centerY + head.radiusY * 1.15,
  )
  const fittedRadiusX = clamp(
    (right - left) / 2,
    head.radiusX * 1.05,
    head.radiusX * 1.28,
  )
  const fittedRadiusY = clamp(
    (boundedBottom - boundedTop) / 2,
    head.radiusY * 1.02,
    head.radiusY * 1.24,
  )
  return {
    centerX: mix(fallback.centerX, boundedCenterX, 0.2),
    centerY: mix(fallback.centerY, (boundedTop + boundedBottom) / 2, 0.2),
    radiusX: mix(fallback.radiusX, fittedRadiusX, 0.25),
    radiusY: mix(fallback.radiusY, fittedRadiusY, 0.25),
    radiusZ: fallback.radiusZ,
  }
}

/**
 * Crown wrap is enabled only when several distributed strand roots confirm
 * that the top of the front-hair layer is actual scalp hair, not an ornament.
 */
function deriveCrownRound(
  head: Anime25DShellEllipsoid,
  layers: readonly Anime25DPlaybackLayer[],
): number {
  const frontHair = layers.filter(
    (layer) => layer.role === 'front-hair' && layer.w > 0 && layer.h > 0,
  )
  const roots = frontHair.flatMap((layer) =>
    layer.strands.filter(
      (strand) =>
        Number.isFinite(strand.x) &&
        Number.isFinite(strand.rootY) &&
        strand.rootY >= layer.y - head.radiusY * 0.08 &&
        strand.rootY <= layer.y + layer.h * 0.72,
    ),
  )
  if (roots.length < 4 || frontHair.length === 0) return 0
  const rootLeft = Math.min(...roots.map((strand) => strand.x))
  const rootRight = Math.max(...roots.map((strand) => strand.x))
  const rootCoverage = (rootRight - rootLeft) / Math.max(1, head.radiusX * 2)
  if (rootCoverage < 0.45) return 0
  const frontTop = Math.min(...frontHair.map((layer) => layer.y))
  const headTop = head.centerY - head.radiusY
  const crownCoverage = clamp(
    (headTop + head.radiusY * 0.18 - frontTop) / (head.radiusY * 0.3),
    0,
    1,
  )
  if (crownCoverage < 0.2) return 0
  return clamp(0.08 + crownCoverage * 0.14, 0, 0.22)
}

function deriveAnime25DTorsoShellProfile(
  playback: Readonly<ShellProfileSource>,
): Anime25DTorsoShellProfile {
  const faceWidth = Math.max(
    1,
    playback.anchors.face.x1 - playback.anchors.face.x0,
  )
  return {
    enabled: playback.layers.some(
      (layer) => layer.role === 'topwear' || layer.role === 'bottomwear',
    ),
    blend: 0.5,
    centerX: playback.anchors.neckPivot.x,
    radiusX: Math.max(1, faceWidth * 0.95),
    radiusZ: Math.max(1, faceWidth * 0.55),
  }
}

function deriveFaceCurve(
  anchors: Readonly<Anime25DPlaybackAnchors>,
  layers: readonly Anime25DPlaybackLayer[],
  startY: number,
  endY: number,
): Anime25DShellCurvePoint[] {
  const span = Math.max(1, endY - startY)
  const progress = (value: number) => clamp((value - startY) / span, 0, 1)
  const eyeRootY =
    anchors.eyeL && anchors.eyeR
      ? (anchors.eyeL.icy + anchors.eyeR.icy) / 2
      : anchors.face.y0 + (anchors.face.y1 - anchors.face.y0) * 0.42
  const noseLayer = layers.find((layer) => layer.role === 'nose')
  const noseY = noseLayer
    ? noseLayer.y + noseLayer.h * 0.65
    : eyeRootY + (anchors.mouth.cy - eyeRootY) * 0.55
  const candidate = [
    0.06,
    progress(eyeRootY),
    progress(noseY),
    progress(anchors.mouth.y0),
    progress(anchors.face.y1),
  ]
  if (!strictlyOrdered(candidate, 0.025)) {
    return DEFAULT_CURVE.map((point) => ({ ...point }))
  }
  return candidate.map((v, index) => ({
    v,
    z: DEFAULT_CURVE[index].z,
  }))
}

function strictlyOrdered(
  values: readonly number[],
  minimumGap: number,
): boolean {
  for (let index = 1; index < values.length; index += 1) {
    if (values[index] - values[index - 1] < minimumGap) return false
  }
  return true
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
