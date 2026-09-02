import type { Anime25DMouthMaterial, Anime25DMouthProfile } from './types'
import { ANIME25D_MOUTH_MATERIALS } from './mouthProfile'

export type SpeechMouthMaterial = Anime25DMouthMaterial

export interface MouthTransitionInput {
  mouthOpen: number
  mouthWide: number
  mouthRound: number
  mouthNarrow: number
  maniac: number
  mouthSeal: number
  mouthEase: number
}

export interface MouthTransitionSample {
  material: SpeechMouthMaterial
  from: SpeechMouthMaterial
  to: SpeechMouthMaterial
  bridge: number
  widthScale: number
  heightScale: number
  shapeNeutralization: number
  centerOffsetX: number
  centerOffsetY: number
}

const MATERIALS = ANIME25D_MOUTH_MATERIALS

/**
 * Chooses one visible raster mouth while preserving a continuous shared mesh.
 * The two strongest visemes form a dominance bridge, so a texture swap lands
 * near their common pose instead of at an arbitrary global threshold.
 */
export class MouthTransitionController {
  private readonly scores = new Float32Array(MATERIALS.length)
  private readonly widthScales = new Float32Array(MATERIALS.length ** 2)
  private readonly heightScales = new Float32Array(MATERIALS.length ** 2)
  private readonly neutralizations = new Float32Array(MATERIALS.length ** 2)
  private readonly centerOffsetsX = new Float32Array(MATERIALS.length ** 2)
  private readonly centerOffsetsY = new Float32Array(MATERIALS.length ** 2)
  private readonly output: MouthTransitionSample = {
    material: 'mouthClose',
    from: 'mouthClose',
    to: 'mouthClose',
    bridge: 0,
    widthScale: 1,
    heightScale: 1,
    shapeNeutralization: 0,
    centerOffsetX: 0,
    centerOffsetY: 0,
  }

  private activeIndex = 0

  constructor(profile: Readonly<Anime25DMouthProfile>) {
    for (let material = 0; material < MATERIALS.length; material += 1) {
      const index = pairIndex(material, material)
      this.widthScales[index] = 1
      this.heightScales[index] = 1
    }
    for (const bridge of profile.bridges) {
      const first = MATERIALS.indexOf(bridge.first)
      const second = MATERIALS.indexOf(bridge.second)
      if (first < 0 || second < 0 || first === second) continue
      for (const index of [
        pairIndex(first, second),
        pairIndex(second, first),
      ]) {
        this.widthScales[index] = bridge.widthScale
        this.heightScales[index] = bridge.heightScale
        this.neutralizations[index] = bridge.neutralization
        this.centerOffsetsX[index] = bridge.centerOffsetX
        this.centerOffsetsY[index] = bridge.centerOffsetY
      }
    }
  }

  sample(input: MouthTransitionInput): Readonly<MouthTransitionSample> {
    resolveMouthMaterialScores(input, this.scores)
    let strongest = 0
    let runnerUp = 1
    if (this.scores[runnerUp] > this.scores[strongest]) {
      strongest = 1
      runnerUp = 0
    }
    for (let index = 2; index < this.scores.length; index += 1) {
      if (this.scores[index] > this.scores[strongest]) {
        runnerUp = strongest
        strongest = index
      } else if (this.scores[index] > this.scores[runnerUp]) {
        runnerUp = index
      }
    }

    const profileIndex = pairIndex(strongest, runnerUp)
    if (
      strongest !== this.activeIndex &&
      this.scores[strongest] >
        this.scores[this.activeIndex] +
          switchMargin(MATERIALS[strongest], MATERIALS[this.activeIndex])
    ) {
      this.activeIndex = strongest
    }

    const strongestScore = this.scores[strongest]
    const runnerUpScore = this.scores[runnerUp]
    const pairTotal = strongestScore + runnerUpScore
    const balance =
      pairTotal > 1e-5
        ? 1 - Math.abs(strongestScore - runnerUpScore) / pairTotal
        : 0
    const bridge = smootherstep((balance - 0.28) / 0.72)

    this.output.material = MATERIALS[this.activeIndex]
    this.output.from = MATERIALS[runnerUp]
    this.output.to = MATERIALS[strongest]
    this.output.bridge = bridge
    this.output.widthScale = 1 - (1 - this.widthScales[profileIndex]) * bridge
    this.output.heightScale = 1 - (1 - this.heightScales[profileIndex]) * bridge
    this.output.shapeNeutralization =
      this.neutralizations[profileIndex] * bridge
    this.output.centerOffsetX = this.centerOffsetsX[profileIndex] * bridge
    this.output.centerOffsetY = this.centerOffsetsY[profileIndex] * bridge
    return this.output
  }
}

function dominantMouthMaterial(
  input: MouthTransitionInput,
): SpeechMouthMaterial {
  const scores = new Float32Array(MATERIALS.length)
  resolveMouthMaterialScores(input, scores)
  let strongest = 0
  for (let index = 1; index < scores.length; index += 1) {
    if (scores[index] > scores[strongest]) strongest = index
  }
  return MATERIALS[strongest]
}

/** Regular viseme under a maniac mix, so the laugh can fade into speech or rest. */
export function regularMouthMaterial(
  input: MouthTransitionInput,
  active: SpeechMouthMaterial | undefined,
): SpeechMouthMaterial {
  if (active && active !== 'mouthManiac') return active
  return dominantMouthMaterial({ ...input, maniac: 0 })
}

function resolveMouthMaterialScores(
  input: MouthTransitionInput,
  output: Float32Array,
): void {
  const seal = clamp01(input.mouthSeal)
  const presence = mouthMaterialPresence(input) * (1 - smootherstep(seal))
  const shapeTotal =
    clamp01(input.mouthWide) +
    clamp01(input.mouthRound) +
    clamp01(input.mouthNarrow)
  const shapeScale = shapeTotal > 1 ? 1 / shapeTotal : 1
  const wide = clamp01(input.mouthWide) * shapeScale
  const round = clamp01(input.mouthRound) * shapeScale
  const narrow = clamp01(input.mouthNarrow) * shapeScale
  const ordinary = Math.max(0, 1 - wide - round - narrow)
  const maniac = smootherstep(clamp01(input.maniac))
  const regular = 1 - maniac
  output[0] = (1 - presence) * regular
  output[1] = presence * ordinary * regular
  output[2] = presence * wide * regular
  output[3] = presence * round * regular
  output[4] = presence * narrow * regular
  output[5] = maniac
}

function mouthMaterialPresence(input: MouthTransitionInput): number {
  return smootherstep(
    (clamp01(input.mouthOpen) - 0.035) /
      (0.14 + clamp01(input.mouthEase) * 0.04),
  )
}

function switchMargin(
  first: SpeechMouthMaterial,
  second: SpeechMouthMaterial,
): number {
  if (first === 'mouthClose' || second === 'mouthClose') return 0.04
  if (first === 'mouthManiac' || second === 'mouthManiac') return 0.055
  const wideRound =
    (first === 'mouthWide' && second === 'mouthRound') ||
    (first === 'mouthRound' && second === 'mouthWide')
  if (wideRound || first === 'mouthRound' || second === 'mouthRound')
    return 0.08
  if (first === 'mouthNarrow' || second === 'mouthNarrow') return 0.07
  return 0.08
}

function pairIndex(first: number, second: number): number {
  return first * MATERIALS.length + second
}

function smootherstep(value: number): number {
  const bounded = clamp01(value)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(0, Math.min(1, value))
}
