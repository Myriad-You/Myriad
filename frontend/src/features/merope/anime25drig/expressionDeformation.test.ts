import type {
  Anime25DExpressionDeformationBinding,
  Anime25DExpressionDeformationFrame,
} from './expressionDeformation'
import type { MouthMorphState } from './mouthRuntime'
import type {
  Anime25DEyeAnchor,
  Anime25DFade,
  Anime25DPlaybackLayer,
} from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { cryEyeDisplayScale } from '../rig/cryEye'
import { dizzyEyeDisplayScale } from '../rig/dizzyEye'
import { squeezeEyeDisplayScale } from '../rig/squeezeEye'
import { IDENTITY_DRIVER } from './driver'
import {
  deformAnime25DExpressionPoint,
  resolveAnime25DExpressionDeformation,
} from './expressionDeformation'

const EYE: Anime25DEyeAnchor = {
  x0: 72,
  x1: 112,
  y0: 86,
  y1: 116,
  icx: 92,
  icy: 101,
  closeY: 103,
}

test('resolves every special geometry layer and keeps role-only eye fallbacks', () => {
  for (const binding of expressionBindings()) {
    assert.equal(
      resolveAnime25DExpressionDeformation(binding.source, Boolean(binding.eye)),
      binding.kind,
      binding.source.role,
    )
  }
  assert.equal(
    resolveAnime25DExpressionDeformation(
      { role: 'eye-dizzy', fade: null },
      true,
    ),
    'dizzy-eye',
  )
  assert.equal(
    resolveAnime25DExpressionDeformation(
      { role: 'eye-dizzy', fade: 'eyeDizzy' },
      false,
    ),
    null,
  )
  assert.equal(
    resolveAnime25DExpressionDeformation(
      { role: 'accessory', fade: null },
      false,
    ),
    null,
  )
})

test('pure expression stage matches the frozen player branches', () => {
  for (let frameIndex = 0; frameIndex < 120; frameIndex += 1) {
    const progress = frameIndex / 119
    const frame = expressionFrame(progress, frameIndex % 13 !== 0)
    const tearHorizontal = Math.sin(progress * 8.7) * 1.35
    const tearVertical = 0.4 + Math.cos(progress * 7.1) * 2.6
    for (const binding of expressionBindings()) {
      for (let row = 0; row <= 5; row += 1) {
        for (let column = 0; column <= 7; column += 1) {
          const source = binding.source
          const restX = source.x + (source.w * column) / 7
          const restY = source.y + (source.h * row) / 5
          const actual = { x: restX, y: restY }
          const expected = { ...actual }
          legacyDeformExpressionPoint(
            expected,
            restY,
            binding,
            tearHorizontal,
            tearVertical,
            frame,
          )
          deformAnime25DExpressionPoint(
            actual,
            restY,
            binding,
            tearHorizontal,
            tearVertical,
            frame,
          )
          assert.deepEqual(
            actual,
            expected,
            `${binding.kind} frame ${frameIndex} vertex ${row}:${column}`,
          )
        }
      }
    }
  }
})

function expressionBindings(): Anime25DExpressionDeformationBinding[] {
  return [
    eyeBinding('eye-dizzy', 'eyeDizzy', 'dizzy-eye', 38, 31),
    eyeBinding('eye-squeeze', 'eyeSqueeze', 'squeeze-eye', 34, 19),
    eyeBinding('eye-cry', 'eyeCry', 'cry-eye', 32, 51),
    eyeBinding('eye-silly-white', 'eyeSilly', 'silly-eye', 40, 30),
    eyeBinding('iris-silly', 'eyeSilly', 'silly-eye', 15, 15),
    eyeBinding('iris-silly', 'eyeSilly', 'silly-eye', 15, 15, 'R'),
    eyeBinding(
      'lovestruck-heart',
      'lovestruckHeart',
      'lovestruck-heart',
      18,
      17,
    ),
    eyeBinding(
      'lovestruck-heart',
      'lovestruckHeart',
      'lovestruck-heart',
      18,
      17,
      'R',
    ),
    binding('lovestruck-face-effect', 'lovestruckFace', 'lovestruck-face'),
    binding('lovestruck-drool', 'lovestruckDrool', 'lovestruck-drool'),
    binding('anger-mark', 'angerMark', 'anger-mark'),
    binding('speechless-sweat', 'speechlessSweat', 'speechless-sweat'),
    binding('nose', null, 'nose-lift'),
  ]
}

function eyeBinding(
  role: string,
  fade: Anime25DFade,
  kind: Anime25DExpressionDeformationBinding['kind'],
  width: number,
  height: number,
  side: Anime25DPlaybackLayer['side'] = 'L',
): Anime25DExpressionDeformationBinding {
  const source = expressionLayer(role, fade, width, height, side)
  return {
    kind,
    source,
    eye: EYE,
    centerX: source.x + source.w / 2,
    centerY: source.y + source.h / 2,
  }
}

function binding(
  role: string,
  fade: Anime25DFade | null,
  kind: Anime25DExpressionDeformationBinding['kind'],
): Anime25DExpressionDeformationBinding {
  const source = expressionLayer(role, fade, 41, 33, null)
  return {
    kind,
    source,
    centerX: source.x + source.w / 2,
    centerY: source.y + source.h / 2,
  }
}

function expressionLayer(
  role: string,
  fade: Anime25DFade | null,
  w: number,
  h: number,
  side: Anime25DPlaybackLayer['side'],
): Anime25DExpressionDeformationBinding['source'] {
  return { role, fade, side, x: 76.5, y: 82.25, w, h }
}

function expressionFrame(
  progress: number,
  withMotion: boolean,
): Anime25DExpressionDeformationFrame {
  const mouthMorph: MouthMorphState = {
    centerX: 128 + Math.sin(progress * 4.1) * 2,
    centerY: 170 + Math.cos(progress * 3.7) * 1.8,
    width: 42 + progress * 18,
    height: 14 + progress * 24,
    openMix: progress,
    wide: 0.2,
    round: 0.3,
    narrow: 0.1,
  }
  return {
    expression: {
      ...IDENTITY_DRIVER,
      eyeOpenL: progress,
      eyeOpenR: 1 - progress * 0.8,
      eyeX: Math.sin(progress * 5.2) * 0.7,
      eyeY: Math.cos(progress * 4.3) * 0.65,
      irisScale: 0.65 + progress * 0.6,
    },
    faceScale: 0.83,
    mouthMorph,
    stylizedMotion: withMotion
      ? {
          angerMarkOffsetY: Math.sin(progress * 7.1) * 1.2,
          angerMarkRotation: Math.cos(progress * 4.9) * 0.16,
          angerMarkScale: 0.7 + progress * 0.8,
          lovestruckDroolOffsetY: Math.cos(progress * 5.1) * 1.4,
          lovestruckFaceScale: 0.75 + progress * 0.5,
          lovestruckHeartScale: 0.6 + progress * 0.8,
          maniac: progress,
          sillyEyeScale: 0.35 + progress * 0.9,
          sillyIrisOffsetXL: Math.sin(progress * 5.3) * 0.12,
          sillyIrisOffsetXR: Math.cos(progress * 4.7) * 0.12,
          sillyIrisOffsetYL: Math.sin(progress * 3.9) * 0.1275,
          sillyIrisOffsetYR: Math.cos(progress * 5.7) * 0.1275,
          speechlessSweatOffsetX: Math.sin(progress * 4.4) * 1.8,
          speechlessSweatOffsetY: Math.cos(progress * 3.8) * 2.9,
          speechlessSweatRotation: -0.035 * progress,
          speechlessSweatScale: 0.65 + progress * 0.7,
        }
      : null,
  }
}

// Frozen copy of the former Anime25DPlayer special-expression sequence.
function legacyDeformExpressionPoint(
  point: { x: number; y: number },
  restY: number,
  binding: Anime25DExpressionDeformationBinding,
  tearHorizontal: number,
  tearVertical: number,
  frame: Anime25DExpressionDeformationFrame,
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
    const flowWeight = legacySmoothstep((localY - 0.31) / 0.62)
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
      const offsetX =
        source.side === 'L'
          ? motion.sillyIrisOffsetXL
          : motion.sillyIrisOffsetXR
      const offsetY =
        source.side === 'L'
          ? motion.sillyIrisOffsetYL
          : motion.sillyIrisOffsetYR
      point.x += offsetX * Math.max(1, eye!.x1 - eye!.x0)
      point.y += offsetY * Math.max(1, eye!.y1 - eye!.y0)
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
    const lidClose = legacySmoothstep((0.32 - eyeOpen) / 0.32)
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
    const noseWeight = legacySmoothstep(
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

function legacySmoothstep(value: number): number {
  const bounded = Math.max(0, Math.min(1, value))
  return bounded * bounded * (3 - 2 * bounded)
}
