export interface PoseSituation {
  speaking: boolean
  singing: boolean
  thinking: boolean
  pointerDriven: boolean
  automation: boolean
  /** 0–1 peak of exclusive sticker faces. */
  sticker: number
}

export interface PoseOccupancy {
  glance: number
  random: number
  coSpeech: number
  groove: number
  thinking: number
  speechMouth: number
  grooveMouth: number
}

const ZERO: PoseOccupancy = {
  glance: 0,
  random: 0,
  coSpeech: 0,
  groove: 0,
  thinking: 0,
  speechMouth: 0,
  grooveMouth: 0,
}

export const GLANCE_IDLE = 1
export const GLANCE_SPEAKING = 0.45
export const GLANCE_SINGING = 0.5
export const RANDOM_IDLE = 1
export const RANDOM_SPEAKING = 0.22
export const RANDOM_SINGING = 0.45
export const COSPEECH_SPEAKING = 0.7
export const COSPEECH_SINGING = 0.2
export const GROOVE_SINGING = 1
export const THINKING_IDLE = 1
export const THINKING_BUSY = 0.25
export const STICKER_KEEP_GLANCE = 0.32
export const STICKER_KEEP_RANDOM = 0.22
export const STICKER_KEEP_COSPEECH = 0.22
export const STICKER_KEEP_THINKING = 0.18

const OCCUPANCY_ATTACK_RATE = 18
const OCCUPANCY_RELEASE_RATE = 6.2
const KEYS = [
  'glance',
  'random',
  'coSpeech',
  'groove',
  'thinking',
  'speechMouth',
  'grooveMouth',
] as const

/** Weights tilt; they do not exclusive-zero living sources. */
export function occupancyTargets(situation: PoseSituation): PoseOccupancy {
  const sticker = clamp01(situation.sticker)
  const speaking = situation.speaking
  const singing = situation.singing
  const glance = !situation.automation
    ? 0
    : situation.pointerDriven
      ? 0.1
      : speaking
        ? GLANCE_SPEAKING
        : singing
          ? GLANCE_SINGING
          : GLANCE_IDLE
  const random = !situation.automation
    ? 0
    : situation.pointerDriven
      ? 0
      : speaking
        ? RANDOM_SPEAKING
        : singing
          ? RANDOM_SINGING
          : RANDOM_IDLE
  const coSpeech = speaking ? COSPEECH_SPEAKING : singing ? COSPEECH_SINGING : 0
  const groove = singing ? GROOVE_SINGING : 0
  const thinking = !situation.thinking
    ? 0
    : speaking || singing
      ? THINKING_BUSY
      : THINKING_IDLE
  return {
    glance: glance * mix(1, STICKER_KEEP_GLANCE, sticker),
    random: random * mix(1, STICKER_KEEP_RANDOM, sticker),
    coSpeech: coSpeech * mix(1, STICKER_KEEP_COSPEECH, sticker),
    groove,
    thinking: thinking * mix(1, STICKER_KEEP_THINKING, sticker),
    speechMouth: speaking ? 1 : 0,
    grooveMouth: singing && !speaking ? 1 : 0,
  }
}

/** Owner/situation flips never step the weights. */
export class PoseOccupancyController {
  private readonly current: PoseOccupancy = { ...ZERO }
  private initialized = false

  sample(
    deltaSeconds: number,
    situation: PoseSituation,
  ): Readonly<PoseOccupancy> {
    const target = occupancyTargets(situation)
    if (!this.initialized) {
      this.initialized = true
      copyOccupancy(this.current, target)
      return this.current
    }
    const dt = clamp(deltaSeconds, 0, 0.05)
    for (const key of KEYS) {
      const from = this.current[key]
      const response =
        target[key] > from ? OCCUPANCY_ATTACK_RATE : OCCUPANCY_RELEASE_RATE
      const rate = 1 - Math.exp(-response * dt)
      this.current[key] = from + (target[key] - from) * rate
    }
    return this.current
  }
}

function copyOccupancy(target: PoseOccupancy, source: PoseOccupancy): void {
  for (const key of KEYS) target[key] = source[key]
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * clamp01(amount)
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(0, Math.min(1, value))
}

function clamp(value: number, minimum: number, maximum: number): number {
  if (!Number.isFinite(value)) return minimum
  return Math.max(minimum, Math.min(maximum, value))
}
