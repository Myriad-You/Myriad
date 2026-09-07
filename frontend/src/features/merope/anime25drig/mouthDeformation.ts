import type { Anime25DDriver } from './driver'
import type { MouthMorphState } from './mouthRuntime'
import type { StylizedExpressionMotion } from './stylizedExpressionMotion'
import type { Anime25DPlayback, Anime25DPlaybackLayer } from './types'

type MouthDeformationExpression = Pick<
  Anime25DDriver,
  'eyeCry' | 'mouthCAng' | 'mouthCY' | 'mouthForm' | 'mouthScale'
>

type MouthStylizedMotion = Pick<
  StylizedExpressionMotion,
  'maniacUpperMouthPulse' | 'sillyMouthOpen'
>

export interface Anime25DMouthDeformationFrame {
  mouth: Readonly<Anime25DPlayback['anchors']['mouth']>
  face: Readonly<Anime25DPlayback['anchors']['face']>
  faceScale: number
  morph: Readonly<MouthMorphState>
  expression: Readonly<MouthDeformationExpression>
  jawDrop: number
  jawOpen: number
  time: number
  stylizedMotion: Readonly<MouthStylizedMotion> | null
}

export interface Anime25DMutableMouthPoint {
  x: number
  y: number
}

export type Anime25DMouthDeformationKind = 'continuous' | 'cry'

const CONTINUOUS_MOUTH_FADES = new Set<Anime25DPlaybackLayer['fade']>([
  'mouthOpen',
  'mouthWide',
  'mouthRound',
  'mouthNarrow',
  'mouthClose',
  'mouthManiac',
  'mouthSilly',
])

/** The generated mouth materials that share Myriad's continuous morph mesh. */
export function isAnime25DContinuousMouth(
  fade: Anime25DPlaybackLayer['fade'],
): boolean {
  return CONTINUOUS_MOUTH_FADES.has(fade)
}

/** Every mouth layer whose local geometry is owned by the Myriad extension. */
export function isAnime25DMouthDeformation(
  fade: Anime25DPlaybackLayer['fade'],
): boolean {
  return resolveAnime25DMouthDeformation(fade) !== null
}

/** Resolve once per layer so the vertex loop does not repeat set lookups. */
export function resolveAnime25DMouthDeformation(
  fade: Anime25DPlaybackLayer['fade'],
): Anime25DMouthDeformationKind | null {
  if (fade === 'mouthCry') return 'cry'
  return isAnime25DContinuousMouth(fade) ? 'continuous' : null
}

/**
 * Applies the complete Myriad mouth extension in its established order.
 * The rest coordinate is deliberately separate from the staged point because
 * rail weights, sob falloff, and special-mouth masks are authored in rest space.
 */
export function deformAnime25DMouthPoint(
  point: Anime25DMutableMouthPoint,
  restX: number,
  restY: number,
  source: Pick<Anime25DPlaybackLayer, 'fade' | 'h' | 'w' | 'x' | 'y'>,
  frame: Readonly<Anime25DMouthDeformationFrame>,
  kind: Anime25DMouthDeformationKind,
): void {
  const { expression, morph, mouth, stylizedMotion } = frame
  const continuous = kind === 'continuous'
  if (continuous) {
    const localX =
      (restX - (source.x + source.w / 2)) / Math.max(1, source.w / 2)
    const localY =
      (restY - (source.y + source.h / 2)) / Math.max(1, source.h / 2)
    const xMagnitude = Math.min(1, Math.abs(localX))
    const yMagnitude = Math.min(1, Math.abs(localY))
    const ovalPinch =
      1 -
      morph.round * 0.13 * (0.28 + yMagnitude ** 1.35) +
      morph.wide * 0.035 * (1 - yMagnitude)
    point.x = morph.centerX + localX * (morph.width / 2) * ovalPinch
    const cornerCurve =
      (0.075 + morph.round * 0.14 - morph.wide * 0.025) * xMagnitude ** 1.65
    const cupidBow =
      morph.openMix * morph.height * 0.034 * (1 - xMagnitude) ** 2
    const lowerFullness =
      morph.height *
      (0.018 + morph.openMix * 0.018) *
      (1 - xMagnitude ** 1.7)
    const upperRail =
      morph.centerY -
      morph.height / 2 +
      morph.height * cornerCurve -
      cupidBow
    const lowerRail =
      morph.centerY +
      morph.height / 2 -
      morph.height * cornerCurve * 0.82 +
      lowerFullness
    const verticalProgress = clamp((localY + 1) / 2, 0, 1)
    const upperAnchoredProgress =
      verticalProgress ** (1 + morph.openMix * 0.12)
    const trackedY =
      upperRail + (lowerRail - upperRail) * upperAnchoredProgress
    const restingY = morph.centerY + localY * (morph.height / 2)
    const railInfluence = smoothstep(morph.openMix)
    point.y = restingY + (trackedY - restingY) * railInfluence
  }
  if (
    (continuous || source.fade === 'mouthCry') &&
    source.fade !== 'mouthSilly' &&
    expression.mouthScale !== 1
  ) {
    point.x = mouth.cx + (point.x - mouth.cx) * expression.mouthScale
    point.y = mouth.cy + (point.y - mouth.cy) * expression.mouthScale
  }
  if (
    (continuous || source.fade === 'mouthCry') &&
    source.fade !== 'mouthSilly'
  ) {
    const localJawY = clamp((restY - source.y) / Math.max(1, source.h), 0, 1)
    const lipJawWeight =
      0.08 + smoothstep((localJawY - 0.18) / 0.82) * 0.72
    point.y += frame.jawDrop * lipJawWeight
  }
  if (
    source.fade === 'mouthOpen' ||
    source.fade === 'mouthWide' ||
    source.fade === 'mouthRound' ||
    source.fade === 'mouthNarrow' ||
    source.fade === 'mouthClose' ||
    source.fade === 'mouthManiac'
  ) {
    if (source.fade === 'mouthManiac') {
      // Authored extreme expression keeps its own small curvature response.
      const halfWidth = (mouth.x1 - mouth.x0) / 2
      const q = Math.abs(point.x - mouth.cx) / (halfWidth + 4)
      point.y -= expression.mouthForm * 0.28 * 6 * frame.faceScale * (q ** 1.5 - 0.35)
    } else {
      // Ordinary materials share one curve throughout the crossfade. Measure
      // it in the live mouth's space, not an anchor or a fixed pixel gain:
      // the old closed-mouth attenuation left authored smiles smiling even
      // with a negative bearing. Width-relative curvature scales with assets.
      const localX = clamp((restX - source.x - source.w / 2) / Math.max(1, source.w / 2), -1, 1)
      const articulation = 1 - morph.openMix * (0.2 + morph.round * 0.45 + morph.narrow * 0.1)
      const amplitude = morph.width * expression.mouthScale * 0.28 * articulation
      point.y -= expression.mouthForm * amplitude * (Math.abs(localX) ** 1.5 - 0.4)
    }
  }
  if (source.fade === 'mouthCry') {
    const halfWidth = (mouth.x1 - mouth.x0) / 2
    const localX = Math.abs(restX - mouth.cx) / (halfWidth + 4)
    const sob = Math.sin(frame.time * 2.55 + 0.35)
    point.y += expression.mouthCY * 14 * frame.faceScale
    point.y +=
      smoothstep(expression.eyeCry) *
      sob *
      0.42 *
      frame.faceScale *
      (0.45 + 0.55 * (1 - Math.min(1, localX)))
  }
  if (source.fade === 'mouthManiac') {
    point.y += expression.mouthCY * 14 * frame.faceScale
    if (stylizedMotion) {
      const localY = clamp((restY - source.y) / Math.max(1, source.h), 0, 1)
      const upperMouthPulse = stylizedMotion.maniacUpperMouthPulse
      const tongueRootAnchor = morph.centerY - morph.height * 0.045
      const scaledX =
        morph.centerX +
        (point.x - morph.centerX) * (1 - upperMouthPulse * 0.5)
      const scaledY =
        tongueRootAnchor +
        (point.y - tongueRootAnchor) * (1 + upperMouthPulse * 3.4)
      const upperMouthWeight = 1 - smoothstep((localY - 0.16) / 0.31)
      point.x += (scaledX - point.x) * upperMouthWeight
      point.y += (scaledY - point.y) * upperMouthWeight
    }
    rotateAround(point, mouth.cx, mouth.cy, expression.mouthCAng * 0.24)
  }
  if (source.fade === 'mouthSilly' && stylizedMotion) {
    const localX = clamp(
      (restX - (source.x + source.w / 2)) / Math.max(1, source.w / 2),
      -1,
      1,
    )
    const localY = clamp(
      (restY - (source.y + source.h / 2)) / Math.max(1, source.h / 2),
      -1,
      1,
    )
    const opening = clamp(stylizedMotion.sillyMouthOpen, 0, 1)
    const omegaLobe = Math.sin(Math.PI * Math.abs(localX))
    const omegaScale = Math.max(1, source.h)
    const closedX = morph.centerX + (point.x - morph.centerX) * 0.88
    const closedY =
      morph.centerY -
      omegaScale * 0.04 +
      omegaLobe * omegaScale * 0.12 +
      localY * omegaScale * 0.025
    point.x = closedX + (point.x - closedX) * opening
    point.y = closedY + (point.y - closedY) * opening
  }
  if (source.fade === 'mouthClose') {
    point.y += expression.mouthCY * 14 * frame.faceScale
    rotateAround(point, mouth.cx, mouth.cy, expression.mouthCAng * 0.35)
  }
}

/** Couples the imported face silhouette to the same smoothed jaw state. */
export function deformAnime25DFaceJawPoint(
  point: Anime25DMutableMouthPoint,
  restY: number,
  frame: Pick<Anime25DMouthDeformationFrame, 'face' | 'jawDrop' | 'jawOpen' | 'mouth'>,
): void {
  const jawStartY =
    frame.mouth.cy - (frame.face.y1 - frame.face.y0) * 0.025
  const jawWeight = smoothstep(
    (restY - jawStartY) / Math.max(1, frame.face.y1 - jawStartY),
  )
  point.y += frame.jawDrop * jawWeight
  point.x +=
    (frame.face.cx - point.x) * frame.jawOpen * 0.006 * jawWeight * jawWeight
}

function rotateAround(
  point: Anime25DMutableMouthPoint,
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
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
