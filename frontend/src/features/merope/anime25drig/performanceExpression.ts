import type { PerformanceBaseline } from '../../../services/agent/types'
import type { BehaviorQuality } from '../motion/behavior'
import type { Anime25DMotionUnit } from './behaviorMotion'
import type { Anime25DDriver } from './driver'
import type { CueIntent } from './performanceCueDefinitions'
import { PERFORMANCE_CUE_INTENTS } from '../performanceContract'
import { IDENTITY_DRIVER } from './driver'
import {
  cueIsSticker,
  intentExpressionPatch,
} from './performanceCueDefinitions'
import { MIN_STICKER_FADE_OUT } from './performanceMotion'

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
  /** Additive secondary-motion strength around the driver's 2.5 rest value. */
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
  behaviorId: string
  /**
   * Signature of the motion unit this was built from, so a restatement that
   * changed nothing is free and one that moved the beat is picked up. Cleared
   * on release, so a behavior that leaves the plan and returns is rebuilt.
   */
  unitKey: string | null
  start: number
  fadeIn: number
  hold: number
  fadeOut: number
  end: number
  sticker: boolean
  /** Manner this beat was authored with, read by the pose response filter. */
  quality: BehaviorQuality
  offset: PerformanceExpressionOffset
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
/**
 * Matches what a focused bearing and a large random action each take out of
 * the idle layer, so no source is quietly privileged over the others. The cue
 * envelope is already smootherstep-shaped and continuous, so this needs no
 * easing of its own.
 */
const LIVE_CUE_AMBIENT_DAMP = 0.3
const EYE_CLOSED_GUARD = 0.12
const EXTREME_SOFT_LIMIT_START = 0.8
/**
 * Owns only additive facial/head expression offsets selected by Lite.
 * Base poses, workbench controls, lip sync, gaze, blinking, and body motion
 * remain separate owners and are never written back by this controller.
 */
export class PerformanceExpressionController {
  private readonly output: PerformanceExpressionOffset = { ...ZERO_OFFSET }
  private readonly cues: ScheduledExpressionCue[] = []
  private lastTime = Number.NaN
  private ambientScale = 1
  private ambientScaleTarget = 1
  private activeLevel = 0
  private activeQuality: BehaviorQuality | null = null

  /**
   * Restates the live performance units on the player clock.
   *
   * Nothing is re-derived here. The plan already resolved when each behavior
   * starts, peaks, releases and ends, so its pegs map straight onto the
   * envelope — the previous path packed those points back into a cue's three
   * durations and rebuilt them, which quietly dropped the stroke plateau.
   *
   * Taking the whole live set makes this idempotent: a restated plan keeps the
   * poses it already scheduled, and a behavior that dropped out of the plan
   * releases instead of playing on to its authored end. That is why the caller
   * needs no record of what it has already played.
   */
  playBehaviorUnits(
    units: readonly Anime25DMotionUnit[],
    timeSeconds: number,
    nowMs: number,
  ): boolean {
    const now = finiteTime(timeSeconds)
    // Scheduling maps wall time onto the write clock, which is correct: the
    // read clock's lead is what makes a cue land on time. A release is the
    // other direction — it continues from the value already on screen, which
    // belongs to the read clock. Starting it on the write clock put a whole
    // lead into the fade before its first frame, and a short `fadeOut` lost
    // most of itself there: 79% in one frame, which reads as snapping to rest.
    const releaseAt = this.releaseClock(now)
    this.pruneExpiredCues(now)
    if (!Number.isFinite(this.lastTime)) this.lastTime = now
    const live = new Map<string, Anime25DMotionUnit>()
    for (const unit of units) {
      if (unit.family === 'performance') live.set(unit.behaviorId, unit)
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
      // The scheduler keeps a beat's identity across plan revisions, so the
      // same id can come back retimed or stronger. Restating it must reach the
      // pose too, or the body would follow the refinement while the face went
      // on playing the beat the floor authored.
      const key = motionUnitKey(unit)
      if (key === cue.unitKey) continue
      const next = scheduledCueFromUnit(unit, now, nowMs)
      if (!next) continue
      Object.assign(cue, next)
      changed = true
    }
    for (const unit of units) {
      if (unit.family !== 'performance') continue
      if (scheduled.has(unit.behaviorId)) continue
      const cue = scheduledCueFromUnit(unit, now, nowMs)
      if (!cue) continue
      this.cues.push(cue)
      scheduled.add(unit.behaviorId)
      changed = true
    }
    this.cues.sort((left, right) => left.start - right.start)
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

  /**
   * The moment the pose currently on screen belongs to.
   *
   * Never behind the last sample: rewinding it would also hand the next frame
   * a second copy of the lead as elapsed time.
   */
  private releaseClock(candidate: number): number {
    return Number.isFinite(this.lastTime)
      ? Math.max(this.lastTime, candidate)
      : candidate
  }

  sample(timeSeconds: number): Readonly<PerformanceExpressionOffset> {
    const now = finiteTime(timeSeconds)
    const dt = Number.isFinite(this.lastTime)
      ? clamp(now - this.lastTime, 0, 0.05)
      : 0
    this.lastTime = now
    // Only attention is eased here. The persistent bearing is a base pose the
    // player installs through `bearingDriverPatch`; this controller owns the
    // transient behaviors that ride on top of it, so it starts every frame at
    // rest rather than holding a second copy of the baseline.
    this.ambientScale +=
      (this.ambientScaleTarget - this.ambientScale) *
      (1 - Math.exp(-AMBIENT_SCALE_RATE * dt))
    for (const key of OFFSET_KEYS) this.output[key] = 0

    this.pruneExpiredCues(now)
    this.activeLevel = 0
    this.activeQuality = null
    for (const cue of this.cues) {
      if (now < cue.start || now >= cue.end) continue
      const envelope = cueEnvelope(cue, now)
      if (envelope <= 0) continue
      if (envelope > this.activeLevel) {
        this.activeLevel = envelope
        this.activeQuality = cue.quality
      }
      for (const key of OFFSET_KEYS) {
        this.output[key] =
          (this.output[key] ?? 0) + (cue.offset[key] ?? 0) * envelope
      }
    }
    return this.output
  }

  /**
   * How much of the idle layer survives underneath the director.
   *
   * Three sources can cover the rig's own drift, and the pose gate multiplies
   * all three: a sticker holds the face, a large random action damps the drift
   * beneath it, and this. Until the live term was added this one answered only
   * to the persistent bearing, so the director's own beats were the single
   * covering motion that left the idle layer running at full amplitude.
   */
  getAmbientMotionScale(): number {
    return this.ambientScale * (1 - LIVE_CUE_AMBIENT_DAMP * this.activeLevel)
  }

  /** Envelope of the loudest live beat, as its share of the composed pose. */
  getActiveLevel(): number {
    return this.activeLevel
  }

  /** Manner of that beat, so the response filter can move at its speed. */
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

/** Installs persistent bearing onto the authored base pose. */
export function bearingDriverPatch(
  baseline: PerformanceBaseline | null,
): Partial<Anime25DDriver> {
  const offset = baseline ? baselineExpressionOffset(baseline) : ZERO_OFFSET
  // PerformanceExpressionOffset is an additive space. Spell out the bearing's
  // actual base-pose ownership here so non-zero-neutral driver fields are not
  // mistaken for absolutes, and transient cue-only fields never leak into it.
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

/**
 * Amplitudes are calibrated against screen pixels, not driver decimals.
 *
 * `layerDeformation` moves a brow point by `brow * 9 * faceScale`, and the
 * homepage rig renders about 300px wide — roughly a third of the source atlas.
 * The old 0.02 `greet` brow was therefore ~0.06 displayed pixels: correct in
 * the driver and invisible on screen, while `randomAction` was ambling around
 * in the 0.12–0.36 band the whole time. The character's own idle fidgeting
 * read louder than anything the director said. These land the ordinary beats
 * in that same band so a semantic cue is at least as legible as a fidget;
 * stickers still carry the strong, rare reads.
 */
export function intentExpressionOffset(
  intent: CueIntent,
  intensity: number,
): PerformanceExpressionOffset {
  const output = { ...ZERO_OFFSET }
  Object.assign(output, intentExpressionPatch(intent, intensity))
  return output
}

/**
 * How large a performance unit reads.
 *
 * Amplitude is the behavior's own intensity shaped by the quality the planner
 * resolved. It belongs here rather than in the realizer: it is a question
 * about the pose, and only the thing that draws the pose should answer it.
 */
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

/** Everything `scheduledCueFromUnit` reads, and nothing else. */
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
  ].join('|')
}

function scheduledCueFromUnit(
  unit: Anime25DMotionUnit,
  playerNow: number,
  wallNowMs: number,
): ScheduledExpressionCue | null {
  const intent = unit.form as CueIntent
  if (!PERFORMANCE_CUE_INTENTS.includes(intent)) return null
  const local = (atMs: number): number =>
    playerNow + (finiteOrZero(atMs) - wallNowMs) / 1_000
  const start = local(unit.timing.startMs)
  const peak = local(unit.timing.strokePeakMs)
  const relax = unit.timing.relaxMs === null ? null : local(unit.timing.relaxMs)
  const end = unit.timing.endMs === null ? null : local(unit.timing.endMs)
  // The stroke plateau is part of the hold. Measuring the hold from strokeEnd
  // instead of strokePeak is what cut every performance behavior short.
  const tail = relax ?? end
  return {
    behaviorId: unit.behaviorId,
    unitKey: motionUnitKey(unit),
    start,
    fadeIn: Math.max(0, peak - start),
    hold: tail === null ? Number.POSITIVE_INFINITY : Math.max(0, tail - peak),
    fadeOut: tail === null || end === null ? 0 : Math.max(0, end - tail),
    // An absent end means the behavior holds until something replaces it.
    end: end ?? Number.POSITIVE_INFINITY,
    sticker: cueIsSticker(intent),
    quality: unit.quality,
    offset: intentExpressionOffset(
      intent,
      performanceUnitAmount(unit.intensity, unit.quality),
    ),
  }
}

/** Adds expression-owned channels while preserving manual left/right asymmetry. */
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

/** Applies semantic expression fields that are not pose-compositor channels. */
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

/** Keeps authored closed eyes closed while allowing partially open eyes to act. */
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
    // At widget scale, bangs can hide most of the brow. Carry low valence in
    // both the eyelids and mouth corners as well; brow rotation distinguishes
    // sadness from tiredness. No replacement eyes, tears or sobbing are held.
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
    // Irritation stays more alert than sadness: knitted, lowered brows, focused
    // eyes and a firmer mouth. Eye openness cannot exceed the driver's 1.0;
    // a positive offset above neutral was clamped away before rendering.
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
  // A missing unit is restated on every plan update; it is one release, not
  // permission to restart the tail (or revive a half-faded cue at full power).
  if (cue.unitKey === null) return
  cue.unitKey = null
  if (at <= cue.start || at >= cue.end) {
    cue.end = Math.min(cue.end, at)
    return
  }
  const fadeOut = Math.min(
    cue.end - at,
    Math.max(cue.fadeOut, cue.sticker ? MIN_STICKER_FADE_OUT : MIN_CUE_RELEASE),
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
