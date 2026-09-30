import type { PerformanceBaseline } from '../../../services/agent/types'
import type { TouchReaction } from '../interaction/touchReaction'
import type { BehaviorQuality } from '../motion/behavior'
import type { Anime25DMotionUnit } from './behaviorMotion'
import type { Anime25DDriver } from './driver'
import type { PerformanceExpressionOffset, ScheduledExpressionCue } from './performanceExpressionShared'
import { IDENTITY_DRIVER } from './driver'
import { cueEnvelope, motionUnitKey, releaseScheduledCue, scheduledCueFromUnit } from './performanceCueSchedule'
import { writeBaselineOffset } from './performanceExpressionApply'
import { clamp, OFFSET_KEYS, ZERO_OFFSET } from './performanceExpressionShared'
import { touchExpressionPatch } from './touchExpression'

export { performanceUnitAmount } from './performanceCueSchedule'
export { applyPerformanceExpressionExtras, applyPerformanceExpressionOffset, mixBoundedExpressionChannel } from './performanceExpressionApply'
export { intentExpressionOffset, mixEyeOpen, type PerformanceExpressionOffset, type PerformanceExpressionTarget } from './performanceExpressionShared'

const AMBIENT_SCALE_RATE = 5.2
const LIVE_CUE_AMBIENT_DAMP = 0.3
const EMOTIONAL_EYE_KEYS = ['anger', 'speechless', 'maniac', 'silly', 'lovestruck', 'eyeCry', 'eyeDizzy', 'eyeSqueeze'] as const
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

function ambientScaleForAttention(attention: number): number {
  return 1 - 0.3 * clamp(attention, 0, 1)
}

function finiteTime(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0
}
