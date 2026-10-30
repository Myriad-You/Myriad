import type { TouchReaction } from '../interaction/touchReaction'
import type { BehaviorQuality } from '../motion/behavior'
import type { Anime25DMotionUnit } from './behaviorMotion'
import type { CueIntent } from './performanceCueDefinitions'
import { intentExpressionPatch } from './performanceCueDefinitions'

export interface PerformanceExpressionOffset {
  brow: number
  browAngSym: number
  eyeOpen: number
  eyeDizzy: number
  eyeSqueeze: number
  eyeCry: number
  eyeX: number
  eyeY: number
  mouthForm: number
  irisScale: number
  angleY: number
  angleZ: number
  body: number
  armY: number
  armPos: number
  bust?: number
  /** Lower lids pushed up by the cheeks: the smile reaches the eyes. */
  eyeSmile?: number
  /** Eyes opened past their drawing: surprise, sudden interest. */
  eyeWide?: number
  anger?: number
  speechless?: number
  maniac?: number
  silly?: number
  lovestruck?: number
}

export interface PerformanceExpressionTarget {
  brow: number
  browAngSym: number
  eyeOpenL: number
  eyeOpenR: number
  eyeDizzy: number
  eyeSqueeze: number
  eyeCry: number
  eyeX: number
  eyeY: number
  mouthForm: number
  irisScale: number
  angleY: number
  angleZ: number
  body?: number
  armY?: number
  armPos?: number
  eyeSmile?: number
  eyeWide?: number
  anger?: number
  speechless?: number
  maniac?: number
  silly?: number
  lovestruck?: number
}

export interface ScheduledExpressionCue {
  thinking?: boolean
  behaviorId: string
  unitKey: string | null
  start: number
  fadeIn: number
  hold: number
  fadeOut: number
  end: number
  sticker: boolean
  quality: BehaviorQuality
  offset: PerformanceExpressionOffset
  touch?: { form: TouchReaction; amount: number; contact: Anime25DMotionUnit['touch'] } | null
  touchResponseStart?: number
  isTouch?: boolean
}

export const OFFSET_KEYS = [
  'brow',
  'browAngSym',
  'eyeOpen',
  'eyeDizzy',
  'eyeSqueeze',
  'eyeCry',
  'eyeX',
  'eyeY',
  'mouthForm',
  'irisScale',
  'angleY',
  'angleZ',
  'body',
  'armY',
  'armPos',
  'bust',
  'eyeSmile',
  'eyeWide',
  'anger',
  'speechless',
  'maniac',
  'silly',
  'lovestruck',
] as const

export const ZERO_OFFSET: PerformanceExpressionOffset = {
  brow: 0,
  browAngSym: 0,
  eyeOpen: 0,
  eyeDizzy: 0,
  eyeSqueeze: 0,
  eyeCry: 0,
  eyeX: 0,
  eyeY: 0,
  mouthForm: 0,
  irisScale: 0,
  angleY: 0,
  angleZ: 0,
  body: 0,
  armY: 0,
  armPos: 0,
  bust: 0,
  eyeSmile: 0,
  eyeWide: 0,
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 0,
  lovestruck: 0,
}
const EYE_CLOSED_GUARD = 0.12

export function intentExpressionOffset(
  intent: CueIntent,
  intensity: number,
): PerformanceExpressionOffset {
  const output = { ...ZERO_OFFSET }
  Object.assign(output, intentExpressionPatch(intent, intensity))
  return output
}

export function mixEyeOpen(base: number, offset: number): number {
  const boundedBase = clamp(base, 0, 1)
  const boundedOffset = finiteOrZero(offset)
  const influence =
    boundedOffset > 0 ? smootherstep(boundedBase / EYE_CLOSED_GUARD) : 1
  return clamp(boundedBase + boundedOffset * influence, 0, 1)
}

export function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

export function finiteOrZero(value: number): number {
  return Number.isFinite(value) ? value : 0
}

export function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
