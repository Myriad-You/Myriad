import type { TouchReaction } from '../interaction/touchReaction'
import type { BehaviorQuality } from '../motion/behavior'
import type { Anime25DMotionUnit } from './behaviorMotion'
import type { CueIntent } from './performanceCueDefinitions'
import type { PerformanceExpressionOffset, ScheduledExpressionCue } from './performanceExpressionShared'
import { PERFORMANCE_CUE_INTENTS } from '../events/performanceContract'
import { TOUCH_REACTIONS } from '../interaction/touchReaction'
import { cueIsSticker } from './performanceCueDefinitions'
import { clamp, finiteOrZero, intentExpressionOffset, OFFSET_KEYS, smootherstep } from './performanceExpressionShared'
import { MIN_STICKER_FADE_OUT } from './performanceMotion'
import { touchExpressionPatch } from './touchExpression'

export function performanceUnitAmount(
  intensity: number,
  quality: Readonly<BehaviorQuality>,
): number {
  return clamp(
    intensity * (0.62 + quality.extent * 0.25 + quality.power * 0.13),
    0.2,
    1.4,
  )
}

export function motionUnitKey(unit: Anime25DMotionUnit): string {
  return [
    unit.form,
    unit.timing.startMs,
    unit.timing.strokePeakMs,
    unit.timing.relaxMs,
    unit.timing.endMs,
    unit.intensity,
    unit.quality.extent,
    unit.quality.power,
    unit.touch?.x, unit.touch?.y, unit.touch?.strokeX, unit.touch?.strokeY,
    unit.touch?.caress,
  ].join('|')
}

export function scheduledCueFromUnit(
  unit: Anime25DMotionUnit,
  playerNow: number,
  wallNowMs: number,
): ScheduledExpressionCue | null {
  const intent = unit.form as CueIntent
  const touch = unit.family === 'touch'
  if (touch ? !TOUCH_REACTIONS.includes(unit.form as TouchReaction) : !PERFORMANCE_CUE_INTENTS.includes(intent)) return null
  const local = (atMs: number): number =>
    playerNow + (finiteOrZero(atMs) - wallNowMs) / 1_000
  const start = local(unit.timing.startMs)
  const peak = local(unit.timing.strokePeakMs)
  const relax = unit.timing.relaxMs === null ? null : local(unit.timing.relaxMs)
  const end = unit.timing.endMs === null ? null : local(unit.timing.endMs)
  const tail = relax ?? end
  return {
    behaviorId: unit.behaviorId,
    thinking: !touch && intent === 'think',
    unitKey: motionUnitKey(unit),
    touchResponseStart: touch ? start : undefined,
    isTouch: touch,
    start,
    fadeIn: Math.max(0, peak - start),
    hold: tail === null ? Number.POSITIVE_INFINITY : Math.max(0, tail - peak),
    fadeOut: tail === null || end === null ? 0 : Math.max(0, end - tail),
    end: end ?? Number.POSITIVE_INFINITY,
    sticker: !touch && cueIsSticker(intent),
    quality: unit.quality,
    touch: touch ? { form: unit.form as TouchReaction, amount: performanceUnitAmount(unit.intensity, unit.quality), contact: unit.touch } : null,
    offset: touch ? {
      ...intentExpressionOffset('listen', 0),
      ...touchExpressionPatch(unit.form as TouchReaction, performanceUnitAmount(unit.intensity, unit.quality), unit.touch, Math.max(0, playerNow - start)),
    } : intentExpressionOffset(
      intent,
      performanceUnitAmount(unit.intensity, unit.quality),
    ),
  }
}

const MIN_CUE_RELEASE = 0.06

export function releaseScheduledCue(cue: ScheduledExpressionCue, at: number): void {
  if (cue.unitKey === null) return
  cue.unitKey = null
  const touch = cue.touch !== null && cue.touch !== undefined
  cue.touch = null
  if (at <= cue.start || at >= cue.end) {
    cue.end = Math.min(cue.end, at)
    return
  }
  const fadeOut = Math.min(
    cue.end - at,
    Math.max(cue.fadeOut, touch ? 0.42 : cue.sticker ? MIN_STICKER_FADE_OUT : MIN_CUE_RELEASE),
  )
  scaleOffset(cue.offset, cueEnvelope(cue, at))
  cue.start = at
  cue.fadeIn = 0
  cue.hold = 0
  cue.fadeOut = fadeOut
  cue.end = at + fadeOut
}

function scaleOffset(
  offset: PerformanceExpressionOffset,
  amount: number,
): void {
  for (const key of OFFSET_KEYS) {
    offset[key] = (offset[key] ?? 0) * amount
  }
}

export function cueEnvelope(cue: ScheduledExpressionCue, now: number): number {
  const elapsed = now - cue.start
  if (elapsed < 0) return 0
  if (cue.fadeIn > 0 && elapsed < cue.fadeIn) {
    return smootherstep(elapsed / cue.fadeIn)
  }
  if (elapsed < cue.fadeIn + cue.hold) return 1
  if (cue.fadeOut <= 0) return 0
  return 1 - smootherstep((elapsed - cue.fadeIn - cue.hold) / cue.fadeOut)
}
