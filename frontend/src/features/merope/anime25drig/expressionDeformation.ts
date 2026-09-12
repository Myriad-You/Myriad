import type { Anime25DDriver } from './driver'
import type { MouthMorphState } from './mouthRuntime'
import type { StylizedExpressionMotion } from './stylizedExpressionMotion'
import type {
  Anime25DEyeAnchor,
  Anime25DPlaybackLayer,
} from './types'
import { cryEyeDisplayScale } from '../rig/cryEye'
import { dizzyEyeDisplayScale } from '../rig/dizzyEye'
import { squeezeEyeDisplayScale } from '../rig/squeezeEye'

type ExpressionGeometryDriver = Pick<
  Anime25DDriver,
  'eyeOpenL' | 'eyeOpenR' | 'eyeX' | 'eyeY' | 'irisScale'
>

type ExpressionGeometryMotion = Pick<
  StylizedExpressionMotion,
  | 'angerMarkOffsetY'
  | 'angerMarkRotation'
  | 'angerMarkScale'
  | 'lovestruckDroolOffsetY'
  | 'lovestruckFaceScale'
  | 'lovestruckHeartScale'
  | 'maniac'
  | 'sillyEyeScale'
  | 'sillyIrisOffsetXL'
  | 'sillyIrisOffsetXR'
  | 'sillyIrisOffsetYL'
  | 'sillyIrisOffsetYR'
  | 'speechlessSweatOffsetX'
  | 'speechlessSweatOffsetY'
  | 'speechlessSweatRotation'
  | 'speechlessSweatScale'
>

export type Anime25DExpressionDeformationKind =
  | 'anger-mark'
  | 'cry-eye'
  | 'dizzy-eye'
  | 'lovestruck-drool'
  | 'lovestruck-face'
  | 'lovestruck-heart'
  | 'nose-lift'
  | 'silly-eye'
  | 'speechless-sweat'
  | 'squeeze-eye'

export interface Anime25DExpressionDeformationFrame {
  expression: Readonly<ExpressionGeometryDriver>
  faceScale: number
  mouthMorph: Readonly<MouthMorphState>
  stylizedMotion: Readonly<ExpressionGeometryMotion> | null
}

export interface Anime25DExpressionDeformationBinding {
  kind: Anime25DExpressionDeformationKind
  source: Pick<
    Anime25DPlaybackLayer,
    'fade' | 'h' | 'role' | 'side' | 'w' | 'x' | 'y'
  >
  eye?: Readonly<Anime25DEyeAnchor>
  centerX: number
  centerY: number
}

export interface Anime25DMutableExpressionPoint {
  x: number
  y: number
}

export function resolveAnime25DExpressionDeformation(
  source: Pick<Anime25DPlaybackLayer, 'fade' | 'role'>,
  hasEyeAnchor: boolean,
): Anime25DExpressionDeformationKind | null {
  if (
    (source.role === 'eye-dizzy' ||
      source.role === 'eye_dizzy' ||
      source.fade === 'eyeDizzy') &&
    hasEyeAnchor
  ) {
    return 'dizzy-eye'
  }
  if (
    (source.role === 'eye-squeeze' ||
      source.role === 'eye_squeeze' ||
      source.fade === 'eyeSqueeze') &&
    hasEyeAnchor
  ) {
    return 'squeeze-eye'
  }
  if (
    (source.role === 'eye-cry' ||
      source.role === 'eye_cry' ||
      source.fade === 'eyeCry') &&
    hasEyeAnchor
  ) {
    return 'cry-eye'
  }
  if (source.fade === 'eyeSilly' && hasEyeAnchor) return 'silly-eye'
  if (source.fade === 'lovestruckHeart' && hasEyeAnchor) {
    return 'lovestruck-heart'
  }
  if (source.fade === 'lovestruckFace') return 'lovestruck-face'
  if (source.fade === 'lovestruckDrool') return 'lovestruck-drool'
  if (source.fade === 'angerMark') return 'anger-mark'
  if (source.fade === 'speechlessSweat') return 'speechless-sweat'
  if (source.role === 'nose') return 'nose-lift'
  return null
}

export function deformAnime25DExpressionPoint(
  point: Anime25DMutableExpressionPoint,
  restY: number,
  binding: Readonly<Anime25DExpressionDeformationBinding>,
  tearHorizontal: number,
  tearVertical: number,
  frame: Readonly<Anime25DExpressionDeformationFrame>,
): void {
  const { kind, source } = binding
  const eye = binding.eye
  if (kind === 'dizzy-eye') {
    const scale = dizzyEyeDisplayScale(source.w, source.h, eye!)
    if (scale !== 1) {
      point.x = eye!.icx + (point.x - eye!.icx) * scale
      point.y = eye!.icy + (point.y - eye!.icy) * scale
    }
    return
  }
  if (kind === 'squeeze-eye') {
    const scale = squeezeEyeDisplayScale(source.w, eye!)
    if (scale !== 1) {
      point.x = binding.centerX + (point.x - binding.centerX) * scale
      point.y = binding.centerY + (point.y - binding.centerY) * scale
    }
    return
  }
  if (kind === 'cry-eye') {
    const scale = cryEyeDisplayScale(source.w, eye!)
    if (scale !== 1) {
      point.x = binding.centerX + (point.x - binding.centerX) * scale
      point.y = binding.centerY + (point.y - binding.centerY) * scale
    }
    const localY = (restY - source.y) / Math.max(1, source.h)
    const flowWeight = smoothstep((localY - 0.31) / 0.62)
    point.x += tearHorizontal * flowWeight
    point.y += tearVertical * flowWeight
    return
  }
  const motion = frame.stylizedMotion
  if (!motion) return
  if (kind === 'silly-eye') {
    const scale = 0.84 + motion.sillyEyeScale * 0.16
    point.x = eye!.icx + (point.x - eye!.icx) * scale
    point.y = eye!.icy + (point.y - eye!.icy) * scale
    if (source.role === 'iris-silly') {
      const irisOffsetX =
        source.side === 'L'
          ? motion.sillyIrisOffsetXL
          : motion.sillyIrisOffsetXR
      const irisOffsetY =
        source.side === 'L'
          ? motion.sillyIrisOffsetYL
          : motion.sillyIrisOffsetYR
      point.x += irisOffsetX * Math.max(1, eye!.x1 - eye!.x0)
      point.y += irisOffsetY * Math.max(1, eye!.y1 - eye!.y0)
    }
    return
  }
  if (kind === 'lovestruck-heart') {
    const eyeOpen =
      source.side === 'L'
        ? frame.expression.eyeOpenL
        : frame.expression.eyeOpenR
    point.x = eye!.icx + (point.x - eye!.icx) * frame.expression.irisScale
    point.y = eye!.icy + (point.y - eye!.icy) * frame.expression.irisScale
    point.x += frame.expression.eyeX * 11 * frame.faceScale
    point.y += frame.expression.eyeY * 6 * frame.faceScale
    const lidClose = smoothstep((0.32 - eyeOpen) / 0.32)
    point.y = eye!.closeY + (point.y - eye!.closeY) * (1 - 0.8 * lidClose)
    point.x =
      binding.centerX +
      (point.x - binding.centerX) * motion.lovestruckHeartScale
    point.y =
      binding.centerY +
      (point.y - binding.centerY) * motion.lovestruckHeartScale
    return
  }
  if (kind === 'lovestruck-face') {
    point.x =
      binding.centerX +
      (point.x - binding.centerX) * motion.lovestruckFaceScale
    point.y =
      binding.centerY +
      (point.y - binding.centerY) * motion.lovestruckFaceScale
    return
  }
  if (kind === 'lovestruck-drool') {
    const desiredX =
      frame.mouthMorph.centerX + frame.mouthMorph.width * 0.48
    const desiredY =
      frame.mouthMorph.centerY + frame.mouthMorph.height * 0.18
    point.x += desiredX - binding.centerX
    point.y +=
      desiredY -
      binding.centerY +
      motion.lovestruckDroolOffsetY * frame.faceScale
    return
  }
  if (kind === 'nose-lift') {
    const noseLift = motion.maniac * 10 * frame.faceScale
    const noseWeight = smoothstep(
      (restY - source.y) / Math.max(1, source.h),
    )
    point.y -= noseLift * noseWeight
    return
  }
  const angerMark = kind === 'anger-mark'
  const scale = angerMark
    ? motion.angerMarkScale
    : motion.speechlessSweatScale
  const rotation = angerMark
    ? motion.angerMarkRotation
    : motion.speechlessSweatRotation
  const offsetX = angerMark
    ? 0
    : motion.speechlessSweatOffsetX * frame.faceScale
  const offsetY =
    (angerMark
      ? motion.angerMarkOffsetY
      : motion.speechlessSweatOffsetY) * frame.faceScale
  const cosine = Math.cos(rotation)
  const sine = Math.sin(rotation)
  const localX = (point.x - binding.centerX) * scale
  const localY = (point.y - binding.centerY) * scale
  point.x = binding.centerX + localX * cosine - localY * sine + offsetX
  point.y = binding.centerY + localX * sine + localY * cosine + offsetY
}

function smoothstep(value: number): number {
  const bounded = Math.max(0, Math.min(1, value))
  return bounded * bounded * (3 - 2 * bounded)
}
