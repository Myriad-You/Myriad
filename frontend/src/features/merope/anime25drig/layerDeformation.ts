import type { Anime25DDriver } from './driver'
import type { Anime25DEyeAnchor, Anime25DPlaybackLayer } from './types'

export type Anime25DUpstreamFeatureKind =
  'eye-close' | 'eye-open-iris' | 'eye-open-lid' | 'eyebrow'

export interface Anime25DMutablePoint {
  x: number
  y: number
}

type UpstreamFeatureExpression = Pick<
  Anime25DDriver,
  | 'brow'
  | 'browAngL'
  | 'browAngR'
  | 'browAngSym'
  | 'eyeCAng'
  | 'eyeCY'
  | 'eyeOpenL'
  | 'eyeOpenR'
  | 'eyeScaleL'
  | 'eyeScaleR'
  | 'eyeX'
  | 'eyeY'
  | 'irisScale'
  | 'eyeSmile'
  | 'eyeWide'
>

export interface Anime25DUpstreamFeatureInput {
  kind: Anime25DUpstreamFeatureKind
  side: Anime25DPlaybackLayer['side']
  eye?: Anime25DEyeAnchor
  centerX: number
  centerY: number
  faceScale: number
  expression: Readonly<UpstreamFeatureExpression>
}

/** Identify only local feature branches that remain unchanged from upstream. */
export function resolveAnime25DUpstreamFeature(
  source: Pick<Anime25DPlaybackLayer, 'fade' | 'role'>,
  hasEyeAnchor: boolean,
): Anime25DUpstreamFeatureKind | null {
  if (
    (source.role === 'eye-close' || source.role === 'eye-close2') &&
    hasEyeAnchor
  ) {
    return 'eye-close'
}
  if (source.fade === 'eyeOpen' && hasEyeAnchor) {
    return source.role === 'irides' ? 'eye-open-iris' : 'eye-open-lid'
  }
  if (source.role === 'eyebrow') return 'eyebrow'
  return null
}

export function bindAnime25DUpstreamFeature(
  source: Pick<
    Anime25DPlaybackLayer,
    'fade' | 'h' | 'role' | 'side' | 'w' | 'x' | 'y'
  >,
  eye: Anime25DEyeAnchor | undefined,
  faceScale: number,
  expression: Readonly<UpstreamFeatureExpression>,
): Anime25DUpstreamFeatureInput | null {
  const kind = resolveAnime25DUpstreamFeature(source, Boolean(eye))
  if (!kind) return null
  return {
    kind,
    side: source.side,
    eye,
    centerX: source.x + source.w / 2,
    centerY: source.y + source.h / 2,
    faceScale,
    expression,
  }
}

export function deformAnime25DUpstreamFeaturePoint(
  point: Anime25DMutablePoint,
  input: Readonly<Anime25DUpstreamFeatureInput>,
  irisRebound?: Readonly<{ x: number; y: number }>,
): void {
  const { expression } = input
  const eyeOpen = input.side === 'L' ? expression.eyeOpenL : expression.eyeOpenR
  if (input.kind === 'eye-close') {
    const eye = input.eye!
    const scale =
      input.side === 'L' ? expression.eyeScaleL : expression.eyeScaleR
    if (scale !== 1) {
      const centerX = (eye.x0 + eye.x1) / 2
      const centerY = (eye.y0 + eye.y1) / 2
      point.x = centerX + (point.x - centerX) * scale
      point.y = centerY + (point.y - centerY) * scale
    }
    point.y -= eyeOpen * 3
    // A smiling closed eye arches up into ^: the lid line bows from its sag.
    const smile = smileAmount(expression.eyeSmile)
    if (smile > 0) {
      const halfWidth = Math.max(1, (eye.x1 - eye.x0) / 2)
      const across = (point.x - (eye.x0 + eye.x1) / 2) / halfWidth
      point.y -= smile * (eye.y1 - eye.y0) * CLOSED_SMILE_BOW * (1 - across * across)
    }
    point.y += expression.eyeCY * 14 * input.faceScale
    rotateAround(
      point,
      input.centerX,
      input.centerY,
      expression.eyeCAng * 0.3 * (input.side === 'L' ? 1 : -1),
    )
    return
  }
  if (input.kind === 'eye-open-iris') {
    const eye = input.eye!
    const visible = smoothstep((eyeOpen - 0.4) / 0.6)
    const scaleX = 1 + ((irisRebound?.x ?? 1) - 1) * visible
    const scaleY = 1 + ((irisRebound?.y ?? 1) - 1) * visible
    point.x = eye.icx + (point.x - eye.icx) * expression.irisScale * scaleX
    point.y = eye.icy + (point.y - eye.icy) * expression.irisScale * scaleY
    point.x += expression.eyeX * 11 * input.faceScale
    point.y += expression.eyeY * 6 * input.faceScale
    const closing = smoothstep((0.32 - eyeOpen) / 0.32)
    point.y = eye.closeY + (point.y - eye.closeY) * (1 - 0.8 * closing)
    // The iris only rides up a little; the rising lid, not a squash, hides its lower part.
    point.y -= smileAmount(expression.eyeSmile) * (eye.y1 - eye.y0) * SMILE_IRIS_LIFT
    // Wide eyes show white all round a tightened iris.
    const wide = smileAmount(expression.eyeWide)
    if (wide > 0) {
      const tighten = 1 - WIDE_IRIS_SHRINK * wide
      point.x = eye.icx + (point.x - eye.icx) * tighten
      point.y = eye.icy + (point.y - eye.icy) * tighten
    }
    return
  }
  if (input.kind === 'eye-open-lid') {
    const eye = input.eye!
    point.y = eye.closeY + (point.y - eye.closeY) * (1 - 0.85 * (1 - eyeOpen))
    smileLids(point, eye, smileAmount(expression.eyeSmile))
    wideLids(point, eye, smileAmount(expression.eyeWide))
    return
  }
  point.y += (-expression.brow * 9 + (1 - eyeOpen) * 3.5) * input.faceScale
  const rotation =
    (input.side === 'L'
      ? expression.browAngL + expression.browAngSym
      : expression.browAngR - expression.browAngSym) * 0.3
  rotateAround(point, input.centerX, input.centerY, rotation)
}

/** At a full smile the lower lid rises this share of the eye's height, at its middle. */
const SMILE_LOWER_LIFT = 0.36
/** ...and the upper lid comes down this much, so the eye narrows from both sides. */
const SMILE_UPPER_DROP = 0.08
const SMILE_IRIS_LIFT = 0.06
/** A fully smiling closed lid bows up by this share of the eye's height at its middle. */
const CLOSED_SMILE_BOW = 0.42

function smileAmount(value: number): number {
  return Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0
}

/**
 * A smile reaching the eyes: the cheeks push the lower lid up in an arch,
 * highest in the middle, and the upper lid settles a little. The white and
 * the lower lashes move; the iris, clipped by the white, is covered from below.
 */
function smileLids(point: Anime25DMutablePoint, eye: Anime25DEyeAnchor, smile: number): void {
  if (smile <= 0) return
  const height = Math.max(1, eye.y1 - eye.y0)
  const halfWidth = Math.max(1, (eye.x1 - eye.x0) / 2)
  const middle = eye.y0 + height * 0.45
  const across = Math.min(1, Math.abs(point.x - (eye.x0 + eye.x1) / 2) / (halfWidth * 1.25))
  const arch = 1 - 0.55 * across * across
  if (point.y > middle) {
    // Lower lashes sit below the white; they take the whole lift.
    const lower = smoothstep((point.y - middle) / (eye.y1 + height * 0.2 - middle))
    point.y -= smile * height * SMILE_LOWER_LIFT * lower * arch
  } else {
    const upper = smoothstep((middle - point.y) / (middle - eye.y0 + height * 0.3))
    point.y += smile * height * SMILE_UPPER_DROP * upper * arch
  }
}

/** Opened wide, the upper lid lifts this share of the eye's height, the lower drops a little. */
const WIDE_UPPER_LIFT = 0.2
const WIDE_LOWER_DROP = 0.06
const WIDE_IRIS_SHRINK = 0.12

function wideLids(point: Anime25DMutablePoint, eye: Anime25DEyeAnchor, wide: number): void {
  if (wide <= 0) return
  const height = Math.max(1, eye.y1 - eye.y0)
  const halfWidth = Math.max(1, (eye.x1 - eye.x0) / 2)
  const middle = eye.y0 + height * 0.5
  const across = Math.min(1, Math.abs(point.x - (eye.x0 + eye.x1) / 2) / (halfWidth * 1.25))
  const arch = 1 - 0.5 * across * across
  if (point.y < middle) {
    const upper = smoothstep((middle - point.y) / (middle - eye.y0 + height * 0.3))
    point.y -= wide * height * WIDE_UPPER_LIFT * upper * arch
  } else {
    const lower = smoothstep((point.y - middle) / (eye.y1 + height * 0.2 - middle))
    point.y += wide * height * WIDE_LOWER_DROP * lower * arch
  }
}

function rotateAround(
  point: Anime25DMutablePoint,
  centerX: number,
  centerY: number,
  radians: number,
): void {
  if (!radians) return
  const cosine = Math.cos(radians)
  const sine = Math.sin(radians)
  const relativeX = point.x - centerX
  const relativeY = point.y - centerY
  point.x = centerX + relativeX * cosine - relativeY * sine
  point.y = centerY + relativeX * sine + relativeY * cosine
}

function smoothstep(value: number): number {
  const bounded = value < 0 ? 0 : value > 1 ? 1 : value
  return bounded * bounded * (3 - 2 * bounded)
}
