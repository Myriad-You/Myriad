import type { PerformanceBaseline } from '../../../services/agent/types'
import type { TouchReaction } from '../interaction/touchReaction'
import type { BehaviorQuality } from '../motion/behavior'
import type { Anime25DMotionUnit } from './behaviorMotion'
import type { Anime25DDriver } from './driver'
import type { CueIntent } from './performanceCueDefinitions'
import { TOUCH_REACTIONS } from '../interaction/touchReaction'
import { PERFORMANCE_CUE_INTENTS } from '../performanceContract'
import { IDENTITY_DRIVER } from './driver'
import {
  cueIsSticker,
  intentExpressionPatch,
} from './performanceCueDefinitions'
import { MIN_STICKER_FADE_OUT } from './performanceMotion'
import { touchExpressionPatch } from './touchExpression'

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
  anger?: number
  speechless?: number
  maniac?: number
  silly?: number
  lovestruck?: number
}

interface ScheduledExpressionCue {
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

const OFFSET_KEYS = [
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
  'anger',
  'speechless',
  'maniac',
  'silly',
  'lovestruck',
] as const

const ZERO_OFFSET: PerformanceExpressionOffset = {
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
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 0,
  lovestruck: 0,
}

const AMBIENT_SCALE_RATE = 5.2
const LIVE_CUE_AMBIENT_DAMP = 0.3
const EMOTIONAL_EYE_KEYS = ['anger', 'speechless', 'maniac', 'silly', 'lovestruck', 'eyeCry', 'eyeDizzy', 'eyeSqueeze'] as const
const EYE_CLOSED_GUARD = 0.12
const EXTREME_SOFT_LIMIT_START = 0.8
/** Owns only additive facial/head expression offsets selected by Lite. */
export class PerformanceExpressionController {
  private thinkingLevel = 0
  getThinkingLevel(): number { return this.thinkingLevel }
  private readonly output: PerformanceExpressionOffset = { ...ZERO_OFFSET }
  private cues: ScheduledExpressionCue[] = []
  private lastTime = Number.NaN
  private ambientScale = 1
  private ambientScaleTarget = 1
  private activeLevel = 0
  private activeQuality: BehaviorQuality | null = null
  private touchLevel = 0
  private sampledTouch: { behaviorId: string; reaction: TouchReaction } | null = null
  private directedLevel = 0
  private readonly touchOffset: PerformanceExpressionOffset = { ...ZERO_OFFSET }

  playBehaviorUnits(
    units: readonly Anime25DMotionUnit[],
    timeSeconds: number,
    nowMs: number,
  ): boolean {
    const now = finiteTime(timeSeconds)
    const releaseAt = this.releaseClock(now)
    this.pruneExpiredCues(now)
    if (!Number.isFinite(this.lastTime)) this.lastTime = now
    const live = new Map<string, Anime25DMotionUnit>()
    for (const unit of units) {
      if (unit.family === 'performance' || unit.family === 'touch') live.set(unit.behaviorId, unit)
    }
    let changed = false
    const scheduled = new Set<string>()
    for (const cue of this.cues) {
      const unit = live.get(cue.behaviorId)
      if (!unit) {
        releaseScheduledCue(cue, releaseAt)
        changed = true
        continue
      }
      scheduled.add(cue.behaviorId)
      const key = motionUnitKey(unit)
      if (key === cue.unitKey) continue
      const next = scheduledCueFromUnit(unit, now, nowMs)
      if (!next) continue
      const currentOffset = cue.offset
      const responseStart = cue.touch?.form === next.touch?.form
        ? cue.touchResponseStart : releaseAt
      Object.assign(cue, next)
      if (unit.family === 'touch') {
        cue.offset = currentOffset
        cue.touchResponseStart = responseStart
      }
      changed = true
    }
    for (const unit of units) {
      if (unit.family !== 'performance' && unit.family !== 'touch') continue
      if (scheduled.has(unit.behaviorId)) continue
      const cue = scheduledCueFromUnit(unit, now, nowMs)
      if (!cue) continue
      this.cues.push(cue)
      scheduled.add(unit.behaviorId)
      changed = true
    }
    this.cues = this.cues.toSorted((left, right) => left.start - right.start)
    this.pruneExpiredCues(now)
    return changed
  }

  setBearingAttention(attention: number | null): void {
    this.ambientScaleTarget =
      attention === null ? 1 : ambientScaleForAttention(attention)
  }

  /** Releases transient cues without erasing the persistent bearing. */
  stopBehaviors(timeSeconds: number): void {
    this.releaseActiveCues(this.releaseClock(finiteTime(timeSeconds)))
  }

  stop(timeSeconds: number): void {
    this.releaseActiveCues(this.releaseClock(finiteTime(timeSeconds)))
    this.ambientScaleTarget = 1
  }

  /** Never behind the last sample */
  private releaseClock(candidate: number): number {
    return Number.isFinite(this.lastTime)
      ? Math.max(this.lastTime, candidate)
      : candidate
  }

  sample(timeSeconds: number, base: Partial<Anime25DDriver> = {}, speaking = false): Readonly<PerformanceExpressionOffset> {
    const now = finiteTime(timeSeconds)
    if (speaking) {
      const releaseAt = this.releaseClock(now)
      for (const cue of this.cues) {
        if (cue.thinking) {
          releaseScheduledCue(cue, releaseAt)
        }
      }
    }
    const dt = Number.isFinite(this.lastTime)
      ? clamp(now - this.lastTime, 0, 0.05)
      : 0
    this.lastTime = now
    // Only attention is eased here.
    this.ambientScale +=
      (this.ambientScaleTarget - this.ambientScale) *
      (1 - Math.exp(-AMBIENT_SCALE_RATE * dt))
    for (const key of OFFSET_KEYS) { this.output[key] = 0; this.touchOffset[key] = 0 }

    this.pruneExpiredCues(now)
    this.activeLevel = 0
    this.thinkingLevel = 0
    this.touchLevel = 0
    this.sampledTouch = null
    this.directedLevel = 0
    this.activeQuality = null
    for (const cue of this.cues) {
      if (now < cue.start || now >= cue.end) continue
      if (cue.touch) {
        const target = touchExpressionPatch(cue.touch.form, cue.touch.amount, cue.touch.contact, now - (cue.touchResponseStart ?? cue.start))
        for (const key of OFFSET_KEYS) {
          const blend = -Math.expm1(-(key === 'eyeX' || key === 'eyeY' ? 18 : 9) * dt)
          cue.offset[key] = (cue.offset[key] ?? 0)
            + ((target[key] ?? 0) - (cue.offset[key] ?? 0)) * blend
        }
      }
      const envelope = cueEnvelope(cue, now)
      if (cue.thinking) this.thinkingLevel = Math.max(this.thinkingLevel, envelope)
      if (cue.touch && cue.behaviorId && envelope >= 0.5
        && now - (cue.touchResponseStart ?? cue.start) >= 0.12) {
        this.sampledTouch = { behaviorId: cue.behaviorId, reaction: cue.touch.form }
      }
      if (cue.isTouch) this.touchLevel = Math.max(this.touchLevel, cue.unitKey !== null ? 1 : envelope)
      if (envelope <= 0) continue
      if (!cue.isTouch) this.directedLevel = Math.max(this.directedLevel, envelope)
      if (envelope > this.activeLevel) {
        this.activeLevel = envelope
        this.activeQuality = cue.quality
      }
      const output = cue.isTouch ? this.touchOffset : this.output
      for (const key of OFFSET_KEYS) {
        output[key] = (output[key] ?? 0) + (cue.offset[key] ?? 0) * envelope
      }
    }
    // Preserve authored brows and eye artwork
    if (this.touchLevel <= 0) return this.output
    let special = 0
    for (const key of EMOTIONAL_EYE_KEYS) special = Math.max(special, Math.abs((base[key] ?? 0) + (this.output[key] ?? 0)))
    const protection = clamp(Math.max(special,
      Math.abs((base.browAngSym ?? 0) + this.output.browAngSym) / 0.6), 0, 1)
    for (const key of OFFSET_KEYS) {
      let addition = this.touchOffset[key] ?? 0
      if (key === 'brow' || key === 'browAngSym' || key === 'eyeOpen') {
        addition *= 1 - 0.75 * protection
        if (key !== 'eyeOpen') {
          const underlying = (base[key] ?? 0) + (this.output[key] ?? 0)
          if (addition * underlying < 0) {
            addition = Math.sign(addition) * Math.min(Math.abs(addition),
              Math.abs(underlying) * 0.25 + Math.abs(addition) * (1 - protection))
          }
        }
      }
      this.output[key] = (this.output[key] ?? 0) + addition
    }
    return this.output
  }

  getTouchShare(): number { return this.touchLevel * (1 - this.directedLevel) }

  getSampledTouch(): { behaviorId: string; reaction: TouchReaction } | null {
    return this.directedLevel < 0.5 ? this.sampledTouch : null
  }

  getAmbientMotionScale(): number {
    return this.ambientScale * (1 - LIVE_CUE_AMBIENT_DAMP * this.activeLevel)
  }

  getActiveLevel(): number {
    return this.activeLevel
  }

  getActiveQuality(): Readonly<BehaviorQuality> | null {
    return this.activeQuality
  }

  getScheduledCueCount(): number {
    return this.cues.length
  }

  private releaseActiveCues(now: number): void {
    let write = 0
    for (const cue of this.cues) {
      if (cue.end <= now || cue.start > now) continue
      releaseScheduledCue(cue, now)
      if (cue.end <= now) continue
      this.cues[write] = cue
      write += 1
    }
    this.cues.length = write
  }

  private pruneExpiredCues(now: number): void {
    let write = 0
    for (let read = 0; read < this.cues.length; read += 1) {
      const cue = this.cues[read]
      if (!cue || cue.end <= now) continue
      this.cues[write] = cue
      write += 1
    }
    this.cues.length = write
  }
}

export function baselineExpressionOffset(
  baseline: PerformanceBaseline,
): PerformanceExpressionOffset {
  const output = { ...ZERO_OFFSET }
  writeBaselineOffset(output, baseline)
  return output
}

export function bearingDriverPatch(
  baseline: PerformanceBaseline | null,
): Partial<Anime25DDriver> {
  const offset = baseline ? baselineExpressionOffset(baseline) : ZERO_OFFSET
  return {
    brow: offset.brow,
    browAngSym: offset.browAngSym,
    eyeOpenL: IDENTITY_DRIVER.eyeOpenL + offset.eyeOpen,
    eyeOpenR: IDENTITY_DRIVER.eyeOpenR + offset.eyeOpen,
    mouthForm: offset.mouthForm,
    irisScale: IDENTITY_DRIVER.irisScale + offset.irisScale,
    body: offset.body,
    armY: offset.armY,
    armPos: offset.armPos,
  }
}

export function intentExpressionOffset(
  intent: CueIntent,
  intensity: number,
): PerformanceExpressionOffset {
  const output = { ...ZERO_OFFSET }
  Object.assign(output, intentExpressionPatch(intent, intensity))
  return output
}

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

function motionUnitKey(unit: Anime25DMotionUnit): string {
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

function scheduledCueFromUnit(
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

export function applyPerformanceExpressionOffset(
  target: PerformanceExpressionTarget,
  offset: Readonly<PerformanceExpressionOffset>,
  gaze = 1,
): void {
  target.brow = mixBoundedExpressionChannel(target.brow, offset.brow, -1, 1, 0)
  target.eyeX = mixBoundedExpressionChannel(
    target.eyeX,
    offset.eyeX * gaze,
    -1,
    1,
    0,
  )
  target.eyeY = mixBoundedExpressionChannel(
    target.eyeY,
    offset.eyeY * gaze,
    -1,
    1,
    0,
  )
  target.angleY = mixBoundedExpressionChannel(
    target.angleY,
    offset.angleY,
    -1,
    1,
    0,
  )
  target.angleZ = mixBoundedExpressionChannel(
    target.angleZ,
    offset.angleZ,
    -1,
    1,
    0,
  )
  if (typeof target.body === 'number') {
    target.body = mixBoundedExpressionChannel(
      target.body,
      offset.body,
      -1,
      1,
      0,
    )
  }
  if (typeof target.armY === 'number') {
    target.armY = mixBoundedExpressionChannel(
      target.armY,
      offset.armY,
      -1,
      1,
      0,
    )
  }
  if (typeof target.armPos === 'number') {
    target.armPos = mixBoundedExpressionChannel(
      target.armPos,
      offset.armPos,
      -1,
      1,
      0,
    )
  }
  applyPerformanceExpressionExtras(target, offset)
}

export function applyPerformanceExpressionExtras(
  target: PerformanceExpressionTarget,
  offset: Readonly<PerformanceExpressionOffset>,
  amount = 1,
): void {
  const weight = clamp(amount, 0, 1)
  target.browAngSym = mixBoundedExpressionChannel(
    target.browAngSym,
    offset.browAngSym * weight,
    -1,
    1,
    0,
  )
  target.eyeOpenL = mixEyeOpen(target.eyeOpenL, offset.eyeOpen * weight)
  target.eyeOpenR = mixEyeOpen(target.eyeOpenR, offset.eyeOpen * weight)
  target.eyeDizzy = mixBoundedExpressionChannel(
    target.eyeDizzy,
    offset.eyeDizzy * weight,
    0,
    1,
    0,
  )
  target.eyeSqueeze = mixBoundedExpressionChannel(
    target.eyeSqueeze,
    offset.eyeSqueeze * weight,
    0,
    1,
    0,
  )
  target.eyeCry = mixBoundedExpressionChannel(
    target.eyeCry,
    offset.eyeCry * weight,
    0,
    1,
    0,
  )
  target.mouthForm = mixBoundedExpressionChannel(
    target.mouthForm,
    offset.mouthForm * weight,
    -1,
    1,
    0,
  )
  target.irisScale = mixBoundedExpressionChannel(
    target.irisScale,
    offset.irisScale * weight,
    0.5,
    1.3,
    1,
  )
  target.anger = mixBoundedExpressionChannel(
    target.anger ?? 0,
    (offset.anger ?? 0) * weight,
    0,
    1,
    0,
  )
  target.speechless = mixBoundedExpressionChannel(
    target.speechless ?? 0,
    (offset.speechless ?? 0) * weight,
    0,
    1,
    0,
  )
  target.maniac = mixBoundedExpressionChannel(
    target.maniac ?? 0,
    (offset.maniac ?? 0) * weight,
    0,
    1,
    0,
  )
  target.silly = mixBoundedExpressionChannel(
    target.silly ?? 0,
    (offset.silly ?? 0) * weight,
    0,
    1,
    0,
  )
  target.lovestruck = mixBoundedExpressionChannel(
    target.lovestruck ?? 0,
    (offset.lovestruck ?? 0) * weight,
    0,
    1,
    0,
  )
}

export function mixEyeOpen(base: number, offset: number): number {
  const boundedBase = clamp(base, 0, 1)
  const boundedOffset = finiteOrZero(offset)
  const influence =
    boundedOffset > 0 ? smootherstep(boundedBase / EYE_CLOSED_GUARD) : 1
  return clamp(boundedBase + boundedOffset * influence, 0, 1)
}

/** Preserves expression headroom instead of flattening other sources at limits. */
export function mixBoundedExpressionChannel(
  base: number,
  offset: number,
  minimum: number,
  maximum: number,
  neutral: number,
): number {
  const boundedBase = clamp(finiteOrZero(base), minimum, maximum)
  const boundedOffset = finiteOrZero(offset)
  if (boundedOffset === 0) return boundedBase
  const direction = boundedOffset > 0 ? 1 : -1
  const boundary = direction > 0 ? maximum : minimum
  const outwardSpan = Math.abs(boundary - neutral)
  if (outwardSpan <= 0) return boundedBase

  let result = boundedBase
  let remaining = Math.abs(boundedOffset)
  const distanceToNeutral = direction * (neutral - result)
  if (distanceToNeutral > 0) {
    const inward = Math.min(remaining, distanceToNeutral)
    result += direction * inward
    remaining -= inward
  }
  if (remaining <= 0) return clamp(result, minimum, maximum)

  const softStart = neutral + direction * outwardSpan * EXTREME_SOFT_LIMIT_START
  const distanceToSoftStart = direction * (softStart - result)
  if (distanceToSoftStart > 0) {
    const linear = Math.min(remaining, distanceToSoftStart)
    result += direction * linear
    remaining -= linear
  }
  if (remaining <= 0) return clamp(result, minimum, maximum)

  const headroom = Math.max(0, direction * (boundary - result))
  if (headroom <= 0) return clamp(result, minimum, maximum)
  const compressed = (remaining * headroom) / (remaining + headroom)
  return clamp(result + direction * compressed, minimum, maximum)
}

function writeBaselineOffset(
  output: PerformanceExpressionOffset,
  baseline: PerformanceBaseline,
): void {
  writeZero(output)
  const patches: Record<
    PerformanceBaseline['expression'],
    Partial<PerformanceExpressionOffset>
  > = {
    withdrawn: {
      brow: 0.18,
      browAngSym: -0.92,
      eyeOpen: -0.4,
      mouthForm: -0.82,
      irisScale: -0.075,
    },
    subdued: {
      brow: 0.12,
      browAngSym: -0.66,
      eyeOpen: -0.26,
      mouthForm: -0.55,
      irisScale: -0.04,
    },
    // Eye openness cannot exceed the driver's 1.0
    tense: {
      brow: -0.56,
      browAngSym: 0.82,
      eyeOpen: -0.08,
      mouthForm: -0.48,
      irisScale: -0.1,
    },
    steady: {},
    warm: { brow: 0.12, mouthForm: 0.15, irisScale: 0.015 },
  }
  Object.assign(output, patches[baseline.expression])
  const posture: Record<
    PerformanceBaseline['posture'],
    Partial<PerformanceExpressionOffset>
  > = {
    closed: { body: -0.16, armY: -0.14, armPos: -0.18 },
    neutral: {},
    open: { body: 0.12, armY: 0.16, armPos: 0.2 },
  }
  Object.assign(output, posture[baseline.posture])
  const poseEnergy = 0.72 + clamp(baseline.motionEnergy, 0.2, 1.4) * 0.36
  output.body *= poseEnergy
  output.armY *= poseEnergy
  output.armPos *= poseEnergy
}

function ambientScaleForAttention(attention: number): number {
  return 1 - 0.3 * clamp(attention, 0, 1)
}

const MIN_CUE_RELEASE = 0.06

function releaseScheduledCue(cue: ScheduledExpressionCue, at: number): void {
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

function cueEnvelope(cue: ScheduledExpressionCue, now: number): number {
  const elapsed = now - cue.start
  if (elapsed < 0) return 0
  if (cue.fadeIn > 0 && elapsed < cue.fadeIn) {
    return smootherstep(elapsed / cue.fadeIn)
  }
  if (elapsed < cue.fadeIn + cue.hold) return 1
  if (cue.fadeOut <= 0) return 0
  return 1 - smootherstep((elapsed - cue.fadeIn - cue.hold) / cue.fadeOut)
}

function writeZero(output: PerformanceExpressionOffset): void {
  for (const key of OFFSET_KEYS) output[key] = 0
}

function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

function finiteTime(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function finiteOrZero(value: number): number {
  return Number.isFinite(value) ? value : 0
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
