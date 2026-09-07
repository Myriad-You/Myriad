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
  )
    return 'eye-close'
  if (source.fade === 'eyeOpen' && hasEyeAnchor) {
    return source.role === 'irides' ? 'eye-open-iris' : 'eye-open-lid'
  }
  if (source.role === 'eyebrow') return 'eyebrow'
  return null
}

/** Build once per layer; the driver object is mutated in place by the player. */
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

/**
 * Mutates one point with the exact eye/eyebrow-local sequence from upstream
 * `deform`. Global head, breath, and body transforms remain separate stages.
 */
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
    return
  }
  if (input.kind === 'eye-open-lid') {
    const eye = input.eye!
    point.y = eye.closeY + (point.y - eye.closeY) * (1 - 0.85 * (1 - eyeOpen))
    return
  }
  point.y += (-expression.brow * 9 + (1 - eyeOpen) * 3.5) * input.faceScale
  const rotation =
    (input.side === 'L'
      ? expression.browAngL + expression.browAngSym
      : expression.browAngR - expression.browAngSym) * 0.3
  rotateAround(point, input.centerX, input.centerY, rotation)
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
