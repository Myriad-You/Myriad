import type { Anime25DDriver } from './driver'
import type { Anime25DMouthDeformationFrame } from './mouthDeformation'
import type { MouthMorphState } from './mouthRuntime'
import type { Anime25DFade, Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import {
  deformAnime25DFaceJawPoint,
  deformAnime25DMouthPoint,
  isAnime25DContinuousMouth,
  isAnime25DMouthDeformation,
  resolveAnime25DMouthDeformation,
} from './mouthDeformation'

const MOUTH = { x0: 101, y0: 157, x1: 157, y1: 186, cx: 129, cy: 171 }
const FACE = { x0: 54, y0: 28, x1: 202, y1: 246, cx: 128, cy: 128 }
const FADES: Anime25DFade[] = [
  'mouthOpen',
  'mouthWide',
  'mouthRound',
  'mouthNarrow',
  'mouthClose',
  'mouthCry',
  'mouthManiac',
  'mouthSilly',
]

test('classifies the exact continuous and locally deformed mouth sets', () => {
  for (const fade of FADES) {
    assert.equal(isAnime25DMouthDeformation(fade), true, fade)
    assert.equal(isAnime25DContinuousMouth(fade), fade !== 'mouthCry', fade)
  }
  assert.equal(isAnime25DMouthDeformation('eyeOpen'), false)
  assert.equal(isAnime25DMouthDeformation(null), false)
})

test('mouth stage preserves the frozen sequence outside ordinary expression curvature', () => {
  for (let frameIndex = 0; frameIndex < 120; frameIndex += 1) {
    const progress = frameIndex / 119
    const expression: Anime25DDriver = {
      ...IDENTITY_DRIVER,
      eyeCry: 0.5 + Math.sin(progress * 8.1) * 0.5,
      mouthCAng: Math.sin(progress * 5.3) * 0.8,
      mouthCY: Math.cos(progress * 4.7) * 0.72,
      mouthForm: Math.sin(progress * 9.2) * 0.9,
      mouthScale: 0.7 + progress * 0.6,
    }
    const morph: MouthMorphState = {
      centerX: 127.5 + Math.sin(progress * 3.2) * 2.4,
      centerY: 171 + Math.cos(progress * 2.7) * 2.1,
      width: 32 + progress * 31,
      height: 12 + progress * 38,
      openMix: progress,
      wide: Math.max(0, Math.sin(progress * Math.PI)),
      round: Math.max(0, Math.cos(progress * Math.PI * 1.3)),
      narrow: Math.max(0, Math.sin(progress * Math.PI * 2.1)) * 0.7,
    }
    const deformationFrame: Anime25DMouthDeformationFrame = {
      mouth: MOUTH,
      face: FACE,
      faceScale: 0.83,
      morph,
      expression,
      jawDrop: Math.sin(progress * Math.PI) * 4.2,
      jawOpen: Math.sin(progress * Math.PI),
      time: progress * 5,
      stylizedMotion:
        frameIndex % 11 === 0
          ? null
          : {
              maniacUpperMouthPulse: Math.sin(progress * Math.PI * 4) * 0.12,
              sillyMouthOpen: 0.15 + progress * 0.85,
            },
    }
    for (const [fadeIndex, fade] of FADES.entries()) {
      // Ordinary curvature now belongs to the shared live morph; preserve the
      // frozen oracle for all other stages and for special-expression artwork.
      expression.mouthForm = fade === 'mouthManiac' ? Math.sin(progress * 9.2) * 0.9 : 0
      const source = mouthLayer(fade, fadeIndex)
      for (let row = 0; row <= 5; row += 1) {
        for (let column = 0; column <= 7; column += 1) {
          const restX = source.x + (source.w * column) / 7
          const restY = source.y + (source.h * row) / 5
          const expected = { x: restX, y: restY }
          const actual = { ...expected }
          legacyDeformMouthPoint(
            expected,
            restX,
            restY,
            source,
            deformationFrame,
          )
          deformAnime25DMouthPoint(
            actual,
            restX,
            restY,
            source,
            deformationFrame,
            resolveAnime25DMouthDeformation(source.fade)!,
          )
          assert.deepEqual(
            actual,
            expected,
            `${fade} frame ${frameIndex} vertex ${row}:${column}`,
          )
        }
      }
    }
  }
})

test('ordinary mouth curvature is visible, symmetric and continuous across materials and opening', () => {
  const regular: Anime25DFade[] = ['mouthClose', 'mouthOpen', 'mouthWide', 'mouthRound', 'mouthNarrow']
  const sample = (fade: Anime25DFade, form: number, opening: number, x: number, scale = 1) => {
    const source = { fade, x: 100 * scale, y: 160 * scale, w: 60 * scale, h: 20 * scale }
    const restX = source.x + source.w * x
    const restY = source.y + source.h / 2
    const point = { x: restX, y: restY }
    deformAnime25DMouthPoint(point, restX, restY, source, {
      mouth: MOUTH, face: FACE, faceScale: scale,
      morph: { centerX: 130 * scale, centerY: 170 * scale, width: 60 * scale,
        height: 20 * scale, openMix: opening, round: opening, wide: 0, narrow: 0 },
      expression: { ...IDENTITY_DRIVER, mouthForm: form },
      jawDrop: 0, jawOpen: opening, time: 0, stylizedMotion: null,
    }, 'continuous')
    return point
  }
  for (const form of [-1, -0.55, 0.5, 1]) {
    let previous = 0
    for (let step = 0; step <= 100; step++) {
      const opening = step / 100
      const delta = (fade: Anime25DFade, x: number, scale = 1) =>
        sample(fade, form, opening, x, scale).y - sample(fade, 0, opening, x, scale).y
      const edge = delta('mouthClose', 0)
      const center = delta('mouthClose', 0.5)
      assert.ok((edge - center) * form < 0, 'negative form lowers corners; positive form raises them')
      assert.ok(Math.abs(edge - delta('mouthClose', 1)) < 1e-10)
      assert.ok(Math.abs(edge) < 11, 'bounded relative to the 60px mouth')
      if (step === 0 && form < 0) assert.ok(edge - center > 6, 'negative standing face must overcome a shallow painted smile')
      if (step > 0) assert.ok(Math.abs(edge - previous) < 0.15, 'opening cannot jump the curvature')
      previous = edge
      for (const fade of regular) {
        assert.ok(Math.abs(delta(fade, 0) - edge) < 1e-10, 'crossfading material cannot change the expression curve')
        assert.ok(Math.abs(delta(fade, 0, 2) - edge * 2) < 1e-10, 'asset scaling preserves shape')
      }
    }
  }
})

test('face-jaw coupling matches the frozen lower-face weighting', () => {
  for (let frameIndex = 0; frameIndex < 120; frameIndex += 1) {
    const progress = frameIndex / 119
    const frame = {
      mouth: MOUTH,
      face: FACE,
      jawDrop: progress * 4.6,
      jawOpen: progress,
    }
    for (let row = 0; row <= 16; row += 1) {
      const restY = FACE.y0 + ((FACE.y1 - FACE.y0) * row) / 16
      const actual = { x: 71 + row * 6.8, y: restY }
      const expected = { ...actual }
      legacyDeformFaceJawPoint(expected, restY, frame)
      deformAnime25DFaceJawPoint(actual, restY, frame)
      assert.deepEqual(actual, expected, `frame ${frameIndex} row ${row}`)
    }
  }
})

function mouthLayer(
  fade: Anime25DFade,
  index: number,
): Pick<Anime25DPlaybackLayer, 'fade' | 'h' | 'w' | 'x' | 'y'> {
  return {
    fade,
    x: 98 + index * 0.7,
    y: 156 - index * 0.4,
    w: 61 - index * 1.3,
    h: 24 + index * 2.2,
  }
}

// Frozen copy of the former Anime25DPlayer inline branch. It intentionally
// shares no production helpers with the extracted stage.
function legacyDeformMouthPoint(
  point: { x: number; y: number },
  restX: number,
  restY: number,
  source: Pick<Anime25DPlaybackLayer, 'fade' | 'h' | 'w' | 'x' | 'y'>,
  frame: Anime25DMouthDeformationFrame,
): void {
  const e = frame.expression
  const mouthMorph = frame.morph
  const morphingMouth = source.fade !== 'mouthCry'
  const mHalfW = (frame.mouth.x1 - frame.mouth.x0) / 2
  if (morphingMouth) {
    const localX =
      (restX - (source.x + source.w / 2)) / Math.max(1, source.w / 2)
    const localY =
      (restY - (source.y + source.h / 2)) / Math.max(1, source.h / 2)
    const xMagnitude = Math.min(1, Math.abs(localX))
    const yMagnitude = Math.min(1, Math.abs(localY))
    const ovalPinch =
      1 -
      mouthMorph.round * 0.13 * (0.28 + yMagnitude ** 1.35) +
      mouthMorph.wide * 0.035 * (1 - yMagnitude)
    point.x =
      mouthMorph.centerX + localX * (mouthMorph.width / 2) * ovalPinch
    const cornerCurve =
      (0.075 + mouthMorph.round * 0.14 - mouthMorph.wide * 0.025) *
      xMagnitude ** 1.65
    const cupidBow =
      mouthMorph.openMix *
      mouthMorph.height *
      0.034 *
      (1 - xMagnitude) ** 2
    const lowerFullness =
      mouthMorph.height *
      (0.018 + mouthMorph.openMix * 0.018) *
      (1 - xMagnitude ** 1.7)
    const upperRail =
      mouthMorph.centerY -
      mouthMorph.height / 2 +
      mouthMorph.height * cornerCurve -
      cupidBow
    const lowerRail =
      mouthMorph.centerY +
      mouthMorph.height / 2 -
      mouthMorph.height * cornerCurve * 0.82 +
      lowerFullness
    const verticalProgress = legacyClamp((localY + 1) / 2, 0, 1)
    const upperAnchoredProgress =
      verticalProgress ** (1 + mouthMorph.openMix * 0.12)
    const trackedY =
      upperRail + (lowerRail - upperRail) * upperAnchoredProgress
    const restingY =
      mouthMorph.centerY + localY * (mouthMorph.height / 2)
    const railInfluence = legacySmoothstep(mouthMorph.openMix)
    point.y = restingY + (trackedY - restingY) * railInfluence
  }
  if (
    (morphingMouth || source.fade === 'mouthCry') &&
    source.fade !== 'mouthSilly' &&
    e.mouthScale !== 1
  ) {
    point.x =
      frame.mouth.cx + (point.x - frame.mouth.cx) * e.mouthScale
    point.y =
      frame.mouth.cy + (point.y - frame.mouth.cy) * e.mouthScale
  }
  if (
    (morphingMouth || source.fade === 'mouthCry') &&
    source.fade !== 'mouthSilly'
  ) {
    const localJawY = legacyClamp(
      (restY - source.y) / Math.max(1, source.h),
      0,
      1,
    )
    const lipJawWeight =
      0.08 + legacySmoothstep((localJawY - 0.18) / 0.82) * 0.72
    point.y += frame.jawDrop * lipJawWeight
  }
  if (source.fade !== 'mouthCry' && source.fade !== 'mouthSilly') {
    const q = Math.abs(point.x - frame.mouth.cx) / (mHalfW + 4)
    let formScale = 1
    if (source.fade === 'mouthRound') formScale = 0.35
    else if (source.fade === 'mouthNarrow') formScale = 0.7
    else if (source.fade === 'mouthOpen') formScale = 0.8
    else if (source.fade === 'mouthClose') formScale = 0.65
    else if (source.fade === 'mouthManiac') formScale = 0.28
    point.y -=
      e.mouthForm * formScale * 6 * frame.faceScale * (q ** 1.5 - 0.35)
  }
  if (source.fade === 'mouthCry') {
    const localX = Math.abs(restX - frame.mouth.cx) / (mHalfW + 4)
    const sob = Math.sin(frame.time * 2.55 + 0.35)
    point.y += e.mouthCY * 14 * frame.faceScale
    point.y +=
      legacySmoothstep(e.eyeCry) *
      sob *
      0.42 *
      frame.faceScale *
      (0.45 + 0.55 * (1 - Math.min(1, localX)))
  }
  if (source.fade === 'mouthManiac') {
    point.y += e.mouthCY * 14 * frame.faceScale
    if (frame.stylizedMotion) {
      const localY = legacyClamp(
        (restY - source.y) / Math.max(1, source.h),
        0,
        1,
      )
      const upperMouthPulse = frame.stylizedMotion.maniacUpperMouthPulse
      const tongueRootAnchor =
        mouthMorph.centerY - mouthMorph.height * 0.045
      const scaledX =
        mouthMorph.centerX +
        (point.x - mouthMorph.centerX) * (1 - upperMouthPulse * 0.5)
      const scaledY =
        tongueRootAnchor +
        (point.y - tongueRootAnchor) * (1 + upperMouthPulse * 3.4)
      const upperMouthWeight =
        1 - legacySmoothstep((localY - 0.16) / 0.31)
      point.x += (scaledX - point.x) * upperMouthWeight
      point.y += (scaledY - point.y) * upperMouthWeight
    }
    legacyRotateAround(
      point,
      frame.mouth.cx,
      frame.mouth.cy,
      e.mouthCAng * 0.24,
    )
  }
  if (source.fade === 'mouthSilly' && frame.stylizedMotion) {
    const localX = legacyClamp(
      (restX - (source.x + source.w / 2)) / Math.max(1, source.w / 2),
      -1,
      1,
    )
    const localY = legacyClamp(
      (restY - (source.y + source.h / 2)) / Math.max(1, source.h / 2),
      -1,
      1,
    )
    const opening = legacyClamp(frame.stylizedMotion.sillyMouthOpen, 0, 1)
    const omegaLobe = Math.sin(Math.PI * Math.abs(localX))
    const omegaScale = Math.max(1, source.h)
    const closedX =
      mouthMorph.centerX + (point.x - mouthMorph.centerX) * 0.88
    const closedY =
      mouthMorph.centerY -
      omegaScale * 0.04 +
      omegaLobe * omegaScale * 0.12 +
      localY * omegaScale * 0.025
    point.x = closedX + (point.x - closedX) * opening
    point.y = closedY + (point.y - closedY) * opening
  }
  if (source.fade === 'mouthClose') {
    point.y += e.mouthCY * 14 * frame.faceScale
    legacyRotateAround(
      point,
      frame.mouth.cx,
      frame.mouth.cy,
      e.mouthCAng * 0.35,
    )
  }
}

function legacyDeformFaceJawPoint(
  point: { x: number; y: number },
  restY: number,
  frame: {
    mouth: typeof MOUTH
    face: typeof FACE
    jawDrop: number
    jawOpen: number
  },
): void {
  const jawStartY =
    frame.mouth.cy - (frame.face.y1 - frame.face.y0) * 0.025
  const jawWeight = legacySmoothstep(
    (restY - jawStartY) / Math.max(1, frame.face.y1 - jawStartY),
  )
  point.y += frame.jawDrop * jawWeight
  point.x +=
    (frame.face.cx - point.x) *
    frame.jawOpen *
    0.006 *
    jawWeight *
    jawWeight
}

function legacyRotateAround(
  point: { x: number; y: number },
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

function legacySmoothstep(value: number): number {
  const bounded = legacyClamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function legacyClamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
