import type { BehaviorQuality } from '../motion/behavior'
import type { BeatFrame } from './beatClock'
import { beatAccentLead, beatAnticipation, BeatClock } from './beatClock'
import { singingVocalEnergy } from './singingClock'

export interface SingingSpectrumDrive {
  bass: number
  beat: number
  vocal: number
  /** Media clock sample used to keep beat prediction aligned with audio. */
  sampleTimeSeconds?: number
  /** Production beat evidence; workbench callers may omit it. */
  beatFrame?: Readonly<BeatFrame>
}

export interface SingingGroovePose {
  angleX: number
  angleY: number
  angleZ: number
  body: number
  armY: number
  armPos: number
  eyeX: number
  brow: number
}

interface Spring1 {
  value: number
  velocity: number
}

const ZERO: SingingGroovePose = {
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  body: 0,
  armY: 0,
  armPos: 0,
  eyeX: 0,
  brow: 0,
}

/** A head nod needs at least this long to land and visibly recover. */
export const MIN_SINGING_NOD_INTERVAL_SECONDS = 0.82

/**
 * Read fast music in half-time (or at the top of the bar) so the body follows
 * the pulse without trying to articulate every beat with its neck.
 */
export function singingNodBeatStride(bpm: number): 1 | 2 | 4 {
  if (!Number.isFinite(bpm) || bpm <= 0) return 1
  const beatPeriod = 60 / bpm
  if (beatPeriod >= MIN_SINGING_NOD_INTERVAL_SECONDS) return 1
  if (beatPeriod * 2 >= MIN_SINGING_NOD_INTERVAL_SECONDS) return 2
  return 4
}

export function singingSpectrumDrive(
  bands: readonly number[],
): SingingSpectrumDrive {
  const bass = unit(bands[0])
  const low = unit(bands[1])
  return {
    bass,
    beat: unit(bass * 0.62 + low * 0.38),
    vocal: singingVocalEnergy(bands),
  }
}

/**
 * Weight cruises side to side. Nod size comes from the live mix: vocals lift,
 * kick/bass dip, and a punch on rising beats. Turns ease instead of bouncing.
 */
export class SingingGrooveController {
  private readonly output: SingingGroovePose = { ...ZERO }
  private lastTime = Number.NaN
  private energy = 0.4
  private vocalFollow = 0
  private beatFollow = 0
  private nodPulse = 0
  private lastFollowerNodAt = Number.NEGATIVE_INFINITY
  private followerNodWindowUntil = Number.NEGATIVE_INFINITY
  private leanTarget = 0
  private leanSpeed = 0
  private leanDir = 0
  private cruise = 0.16
  private spanNow = 0.26
  private spanGoal = 0.26
  private turnAt = 0.82
  private cruiseClock = 0
  private nextCruiseAt = 0.6
  private readonly neckX: Spring1 = { value: 0, velocity: 0 }
  private readonly neckZ: Spring1 = { value: 0, velocity: 0 }
  private readonly neckY: Spring1 = { value: 0, velocity: 0 }
  private readonly torso: Spring1 = { value: 0, velocity: 0 }
  private readonly arm: Spring1 = { value: 0, velocity: 0 }
  private readonly beat = new BeatClock()
  private beatFrame = this.beat.sample(0, 0, false)
  private readonly externalBeatFrame: BeatFrame = { ...this.beatFrame }
  private externalBeatSampleAt = Number.NaN
  private externalBeatObservedAt = Number.NaN
  private armMotion = false
  private weyl = 0.41

  setArmMotion(enabled: boolean): void {
    this.armMotion = enabled
  }

  setTrack(trackId: string | null): void {
    this.beat.setTrack(trackId)
    this.externalBeatSampleAt = Number.NaN
    this.externalBeatObservedAt = Number.NaN
  }

  sample(
    timeSeconds: number,
    enabled: boolean,
    drive: SingingSpectrumDrive | null,
    quality?: Readonly<BehaviorQuality>,
  ): Readonly<SingingGroovePose> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    const dt = Number.isFinite(this.lastTime)
      ? clamp(now - this.lastTime, 0, 0.08)
      : 1 / 60
    this.lastTime = now
    const vocal = unit(drive?.vocal)
    const beat = unit(drive?.beat)
    this.energy +=
      ((enabled ? Math.max(vocal, beat * 0.4, 0.32) : 0) - this.energy) *
      (1 - Math.exp(-1.6 * dt))
    this.beatFrame = this.resolveBeatFrame(now, enabled, drive)
    this.followSpectrum(now, dt, enabled, vocal, beat)

    const pace =
      relativeQuality(quality?.tempo, 1, 0.72) *
      relativeQuality(quality?.density, 0.8, 0.12)
    if (enabled) this.driftLean(dt * pace)
    else this.settleLean(dt)

    const pitch = enabled ? this.nodPitch() : 0
    const directness = relativeQuality(quality?.directness, 0.72, 0.22)
    const fluidity = relativeQuality(quality?.fluidity, 0.8, 0.18)
    const rebound = relativeQuality(quality?.rebound, 0.35, -0.2)
    const response = clamp(directness / fluidity, 0.72, 1.35)
    const damping = clamp(fluidity * rebound, 0.78, 1.24)
    const asymmetry = relativeQuality(quality?.asymmetry, 0.2, 0.2)
    const density = relativeQuality(quality?.density, 0.8, 0.16)
    stepSpring(
      this.neckZ,
      this.leanTarget * asymmetry,
      dt,
      1.45 * response,
      1.04 * damping,
    )
    stepSpring(
      this.neckX,
      (this.leanTarget * 0.42) / directness,
      dt,
      1.5 * response,
      1.04 * damping,
    )
    stepSpring(this.neckY, pitch, dt, 2.25 * response, 1.16 * damping)
    stepSpring(
      this.torso,
      this.neckZ.value * 0.55 * density,
      dt,
      0.95 * response,
      1.08 * damping,
    )
    const barWave = Math.sin(this.beatFrame.barPhase * Math.PI * 2)
    const armTarget =
      enabled && this.armMotion
        ? (-this.leanTarget * 0.42 + barWave * this.energy * 0.045) * density
        : 0
    stepSpring(this.arm, armTarget, dt, 0.88 * response, 1.04 * damping)

    this.output.angleX = this.neckX.value
    this.output.angleY = this.neckY.value
    this.output.angleZ = this.neckZ.value
    // The renderer gives singing body rotation more range than ordinary idle
    // motion, but the old 0.22 transfer still collapsed a full lean to only a
    // few hundredths. Preserve the slower torso spring and transmit enough of
    // it for the upper body to visibly follow the head.
    this.output.body = this.torso.value * 0.55
    this.output.armY = this.arm.value * 0.72
    this.output.armPos = -this.arm.value * 0.48 + this.torso.value * 0.14
    // A rhythmic controller realizes body entrainment only. Eye and brow
    // reactions are sparse semantic behaviors selected above this layer.
    this.output.eyeX = 0
    this.output.brow = 0
    return this.output
  }

  private resolveBeatFrame(
    now: number,
    enabled: boolean,
    drive: SingingSpectrumDrive | null,
  ): Readonly<BeatFrame> {
    const supplied = drive?.beatFrame
    const mediaTime = drive?.sampleTimeSeconds
    if (!supplied || mediaTime === undefined || !Number.isFinite(mediaTime)) {
      return this.beat.sample(now, unit(drive?.bass), enabled)
    }
    const fresh = mediaTime !== this.externalBeatSampleAt
    if (fresh) {
      Object.assign(this.externalBeatFrame, supplied)
      this.externalBeatSampleAt = mediaTime
      this.externalBeatObservedAt = now
    }
    if (!enabled || supplied.bpm <= 0 || supplied.confidence <= 0) {
      this.externalBeatFrame.downbeat = fresh && supplied.downbeat
      this.externalBeatFrame.onset = fresh && supplied.onset
      return this.externalBeatFrame
    }
    const period = 60 / supplied.bpm
    const elapsed = Number.isFinite(this.externalBeatObservedAt)
      ? Math.max(0, now - this.externalBeatObservedAt)
      : 0
    const total = supplied.beatPhase + elapsed / period
    const crossed = Math.floor(total)
    this.externalBeatFrame.beatPhase = total - crossed
    this.externalBeatFrame.beatCount = supplied.beatCount + crossed
    this.externalBeatFrame.barPhase =
      ((this.externalBeatFrame.beatCount % 4) +
        this.externalBeatFrame.beatPhase) /
      4
    this.externalBeatFrame.downbeat = fresh && supplied.downbeat
    this.externalBeatFrame.onset = fresh && supplied.onset
    return this.externalBeatFrame
  }

  private followSpectrum(
    now: number,
    dt: number,
    enabled: boolean,
    vocal: number,
    beat: number,
  ): void {
    const vocalTarget = enabled ? vocal : 0
    const beatTarget = enabled ? beat : 0
    this.vocalFollow +=
      (vocalTarget - this.vocalFollow) * (1 - Math.exp(-1.35 * dt))
    const rise = enabled ? Math.max(0, beatTarget - this.beatFollow) : 0
    const beatRate = beatTarget > this.beatFollow ? 9 : 3.2
    this.beatFollow +=
      (beatTarget - this.beatFollow) * (1 - Math.exp(-beatRate * dt))
    const cadenceLocked =
      this.beatFrame.confidence >= 0.4 && this.beatFrame.bpm > 0
    const cadenceBeat = isSingingNodBeat(
      this.beatFrame.beatCount,
      this.beatFrame.bpm,
    )
    const startsNod =
      rise > 0.01 &&
      (!cadenceLocked || cadenceBeat) &&
      now - this.lastFollowerNodAt >= MIN_SINGING_NOD_INTERVAL_SECONDS
    if (startsNod) {
      this.lastFollowerNodAt = now
      // A spectrum attack spans several frames. Admit that short attack as
      // one gesture so cadence limiting removes extra nods, not their depth.
      this.followerNodWindowUntil = now + 0.16
    }
    if (!enabled) this.followerNodWindowUntil = Number.NEGATIVE_INFINITY
    if (now <= this.followerNodWindowUntil) this.nodPulse += rise * 14
    // A cadence-limited nod needs time to reach the slower neck spring. The
    // old fast release decayed before the head could follow, so reducing nod
    // frequency also erased almost all of its downward stroke.
    this.nodPulse +=
      (0 - this.nodPulse) * (1 - Math.exp(-(enabled ? 3.5 : 8) * dt))
    this.nodPulse = clamp(this.nodPulse, 0, 1)
  }

  /**
   * Dip into the beat and come back up.
   *
   * `nodPulse` is an envelope follower, so it can only fire after the hit that
   * caused it — the head always arrived late. When the beat clock finds a
   * tempo, a phase-timed accent joins it, leaving early enough to land on the
   * downbeat. They combine by max rather than crossfade: the two peak at
   * different moments, so averaging them would flatten the very accent this is
   * meant to sharpen. With no tempo the timed term is zero and the groove is
   * exactly the envelope follower it always was.
   */
  private nodPitch(): number {
    const spanForNod = Math.max(this.spanNow, 0.16)
    const edge = clamp(Math.abs(this.leanTarget) / spanForNod, 0, 1)
    const lift = mix(0.16, 0.32, this.vocalFollow)
    const grooveDip = mix(0.01, 0.04, this.beatFollow)
    const locked = this.beatFrame.confidence
    const lead = beatAccentLead(this.beatFrame.bpm)
    const accentBeat =
      this.beatFrame.beatCount + (this.beatFrame.beatPhase >= 1 - lead ? 1 : 0)
    const timed = isSingingNodBeat(accentBeat, this.beatFrame.bpm)
      ? beatAnticipation(this.beatFrame.beatPhase, lead)
      : 0
    // The follower carries the depth, the timed accent carries the timing, and
    // they combine by max so neither is traded for the other: suppressing the
    // follower flattens the dip, and averaging them flattens both, since the
    // two peak at different moments by construction.
    //
    // The neck is still a spring, so the pose trails the drive; the accent
    // begins earlier than the hit that used to cause it, but this does not on
    // its own put the head exactly on the beat.
    const follower = smootherstep(this.nodPulse)
    const drive = Math.max(follower, timed * locked * 0.82)
    // Keep the downbeat legible without making the whole head dive. The lift
    // remains unchanged, so this trims only the downward half of the nod.
    const hitDip = drive * mix(0.25, 0.38, this.beatFollow)
    const edgeDip = edge * mix(0.03, 0.06, this.energy)
    return clamp(lift - grooveDip - hitDip - edgeDip, -0.31, 0.32)
  }

  private driftLean(dt: number): void {
    if (this.leanDir === 0) {
      this.leanDir = this.unit() < 0.5 ? -1 : 1
      this.pickCruise()
      this.turnAt = this.mixRange(0.4, 0.96)
    }

    this.cruiseClock += dt
    if (this.cruiseClock >= this.nextCruiseAt) {
      this.cruiseClock = 0
      this.nextCruiseAt = this.mixRange(0.25, 1.1)
      this.pickCruise()
    }

    this.spanNow += (this.spanGoal - this.spanNow) * (1 - Math.exp(-0.7 * dt))
    if (Math.abs(this.spanNow - this.spanGoal) < 0.004) {
      this.spanGoal = mix(0.22, 0.3, this.energy) * mix(0.94, 1.08, this.unit())
    }

    const span = Math.max(this.spanNow, 0.16)
    const edge = Math.abs(this.leanTarget) / span
    const outward = Math.sign(this.leanTarget) === this.leanDir
    if (outward && (edge > this.turnAt || Math.abs(this.leanTarget) >= span)) {
      this.turnAround()
    }

    const slow = outward ? mix(1, 0.7, clamp((edge - 0.55) / 0.35, 0, 1)) : 1
    const desired = this.leanDir * this.cruise * slow
    this.leanSpeed += (desired - this.leanSpeed) * (1 - Math.exp(-12 * dt))
    this.leanTarget = clamp(this.leanTarget + this.leanSpeed * dt, -span, span)
  }

  private settleLean(dt: number): void {
    this.leanSpeed += (0 - this.leanSpeed) * (1 - Math.exp(-2.1 * dt))
    this.leanTarget += this.leanSpeed * dt
    this.leanTarget += (0 - this.leanTarget) * (1 - Math.exp(-1.15 * dt))
  }

  private turnAround(): void {
    this.leanDir = -this.leanDir
    this.turnAt = this.mixRange(0.4, 0.96)
    this.pickCruise()
    this.cruiseClock = 0
    this.nextCruiseAt = this.mixRange(0.25, 1.1)
  }

  private pickCruise(): void {
    this.cruise = mix(0.14, 0.24, this.energy) * mix(0.82, 1.22, this.unit())
  }

  private mixRange(minimum: number, maximum: number): number {
    return mix(minimum, maximum, this.unit())
  }

  private unit(): number {
    this.weyl = (this.weyl + 0.6180339887) % 1
    return this.weyl
  }
}

function isSingingNodBeat(beatCount: number, bpm: number): boolean {
  const stride = singingNodBeatStride(bpm)
  const beat = Math.trunc(Number.isFinite(beatCount) ? beatCount : 0)
  return ((beat % stride) + stride) % stride === 0
}

function stepSpring(
  state: Spring1,
  target: number,
  dt: number,
  frequencyHz: number,
  dampingRatio: number,
): void {
  if (dt <= 0) return
  let remain = dt
  const omega = Math.PI * 2 * frequencyHz
  while (remain > 0) {
    const step = Math.min(1 / 90, remain)
    const accel =
      -(omega * omega) * (state.value - target) -
      2 * dampingRatio * omega * state.velocity
    state.velocity += accel * step
    state.value += state.velocity * step
    remain -= step
  }
}

function unit(value: number | undefined): number {
  if (value == null || !Number.isFinite(value)) return 0
  return Math.max(0, Math.min(1, value))
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * unit(amount)
}

function smootherstep(value: number): number {
  const amount = unit(value)
  return amount * amount * amount * (amount * (amount * 6 - 15) + 10)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function relativeQuality(
  value: number | undefined,
  neutral: number,
  influence: number,
): number {
  const resolved =
    typeof value === 'number' && Number.isFinite(value) ? value : neutral
  return clamp(1 + (resolved - neutral) * influence, 0.65, 1.4)
}
