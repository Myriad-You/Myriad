/**
 * Causal tempo and pulse phase from low-band energy. Confidence-gated timing
 * lets the body prepare before a likely pulse instead of always reacting late.
 * This is not a neural beat/downbeat/meter estimator. When the pulse becomes
 * uncertain, confidence falls and callers stop scheduling predicted accents.
 */

/** 60–200 BPM. Outside this an "interval" is a fill or a dropout, not a tempo. */
export const MIN_BEAT_PERIOD = 0.3
export const MAX_BEAT_PERIOD = 1

/** Two onsets closer than this are one hit, not a tempo. */
const MIN_ONSET_GAP = 0.12
/**
 * A tempo is a claim about music that is playing now, so it has to expire.
 *
 * `singing` stays true across a track switch — the rig holds the pose for up
 * to 12s so it does not snap to rest between songs — and the old signal is
 * cleared, not replaced. Keyed only on that flag the clock kept a 120bpm lock
 * at full confidence through the silence and carried it into the next song.
 * The longest legitimate gap here is one beat at 60bpm, so evidence older than
 * this is a dropout, not a tempo.
 */
const EVIDENCE_GRACE = 1.6
/** Fully forgotten this long after the last onset. */
const EVIDENCE_EXPIRY = 3.2
const FLUX_THRESHOLD = 0.045
/** Intervals kept for the period estimate. */
const INTERVAL_MEMORY = 8
/** How hard an onset drags the phase back. Full correction chases noise. */
const PHASE_PULL = 0.28

export interface BeatFrame {
  /** 0 at an estimated pulse, approaching 1 just before the next. */
  beatPhase: number
  /** Whole beats since tracking began. */
  beatCount: number
  bpm: number
  /** 0 when the tempo estimate is not to be trusted. */
  confidence: number
  /** True on the frame a beat boundary is crossed. */
  beatCrossed: boolean
  /** True on the frame an onset was detected, tempo or not. */
  onset: boolean
}

const IDLE: BeatFrame = {
  beatPhase: 0,
  beatCount: 0,
  bpm: 0,
  confidence: 0,
  beatCrossed: false,
  onset: false,
}

export class BeatClock {
  private trackId: string | null = null
  private readonly intervals: number[] = []
  private lastTime = Number.NaN
  private lastBass = 0
  private lastOnsetAt = Number.NaN
  private anchor = Number.NaN
  private period = 0
  private beatCount = 0
  private phase = 0
  private readonly frame: BeatFrame = { ...IDLE }

  /** A new media item cannot inherit tempo or phase from the previous one. */
  setTrack(trackId: string | null): void {
    if (trackId === this.trackId) return
    this.trackId = trackId
    this.reset()
  }

  reset(): void {
    this.intervals.length = 0
    this.lastTime = Number.NaN
    this.lastBass = 0
    this.lastOnsetAt = Number.NaN
    this.anchor = Number.NaN
    this.period = 0
    this.beatCount = 0
    this.phase = 0
    Object.assign(this.frame, IDLE)
  }

  sample(
    timeSeconds: number,
    bass: number,
    enabled: boolean,
  ): Readonly<BeatFrame> {
    if (!enabled) {
      this.reset()
      return this.frame
    }
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    const level = unit(bass)
    let previous = this.lastTime
    if (Number.isFinite(previous) && (now < previous || now - previous > 0.5)) {
      this.reset()
      previous = Number.NaN
    }
    this.lastTime = now
    const dt = Number.isFinite(previous) ? Math.max(0, now - previous) : 0

    if (this.evidenceAge(now) >= EVIDENCE_EXPIRY) this.forgetTempo()
    const beatCrossed = this.advancePhase(dt)
    // Advance the prediction to `now` before correcting it with evidence at
    // `now`. Doing this in the opposite order applied the same dt after an
    // onset reset and placed the predicted beat almost one period late.
    const onset = this.detectOnset(now, level)

    this.frame.beatPhase = this.phase
    this.frame.beatCount = this.beatCount
    this.frame.bpm = this.period > 0 ? 60 / this.period : 0
    this.frame.confidence = this.confidence(now)
    this.frame.beatCrossed = beatCrossed
    this.frame.onset = onset
    return this.frame
  }

  private detectOnset(now: number, level: number): boolean {
    const flux = level - this.lastBass
    this.lastBass = level
    if (flux < FLUX_THRESHOLD) return false
    if (
      Number.isFinite(this.lastOnsetAt) &&
      now - this.lastOnsetAt < MIN_ONSET_GAP
    ) {
      return false
    }
    if (Number.isFinite(this.lastOnsetAt)) {
      this.observeInterval(now - this.lastOnsetAt)
    }
    this.lastOnsetAt = now
    this.pullPhaseTo(now)
    return true
  }

  /**
   * Half- and double-time readings are the same tempo heard differently, so
   * they are folded in rather than thrown away — an eighth-note hi-hat should
   * not halve the estimate.
   */
  private observeInterval(raw: number): void {
    let interval = raw
    if (this.period > 0) {
      for (const factor of [2, 0.5]) {
        const folded = interval * factor
        if (
          Math.abs(folded - this.period) < Math.abs(interval - this.period) &&
          folded >= MIN_BEAT_PERIOD &&
          folded <= MAX_BEAT_PERIOD
        ) {
          interval = folded
        }
      }
    }
    if (interval < MIN_BEAT_PERIOD || interval > MAX_BEAT_PERIOD) return
    this.intervals.push(interval)
    if (this.intervals.length > INTERVAL_MEMORY) this.intervals.shift()
    this.period = median(this.intervals)
  }

  /** An onset is evidence of where the beat is, weighted by how sure we are. */
  private pullPhaseTo(now: number): void {
    if (this.period <= 0) {
      this.anchor = now
      this.phase = 0
      return
    }
    if (!Number.isFinite(this.anchor)) {
      this.anchor = now
      this.phase = 0
      return
    }
    // Nearest boundary, so a late onset pulls back rather than forward a beat.
    const error = this.phase > 0.5 ? this.phase - 1 : this.phase
    this.phase -= error * PHASE_PULL * Math.max(0.25, this.confidence(now))
    if (this.phase < 0) this.phase += 1
  }

  private advancePhase(dt: number): boolean {
    if (this.period <= 0 || dt <= 0) return false
    this.phase += dt / this.period
    if (this.phase < 1) return false
    const crossed = Math.floor(this.phase)
    this.phase -= crossed
    this.beatCount += crossed
    return true
  }

  /**
   * Spread of the remembered intervals, faded out as the evidence goes stale.
   *
   * Fading rather than dropping means a quiet bar or a breakdown loosens the
   * lock instead of cutting the groove dead, and a track switch has decayed to
   * nothing long before the next song starts.
   */
  private confidence(now: number): number {
    if (this.intervals.length < 3 || this.period <= 0) return 0
    let spread = 0
    for (const interval of this.intervals) {
      spread += Math.abs(interval - this.period)
    }
    spread /= this.intervals.length
    const fit = unit(1 - (spread / this.period) * 4)
    return fit * this.freshness(now)
  }

  private freshness(now: number): number {
    const age = this.evidenceAge(now)
    if (age <= EVIDENCE_GRACE) return 1
    if (age >= EVIDENCE_EXPIRY) return 0
    return unit(1 - (age - EVIDENCE_GRACE) / (EVIDENCE_EXPIRY - EVIDENCE_GRACE))
  }

  private evidenceAge(now: number): number {
    return Number.isFinite(this.lastOnsetAt) ? now - this.lastOnsetAt : 0
  }

  /** Keeps the running phase so the pose does not jump; drops only the claim. */
  private forgetTempo(): void {
    this.intervals.length = 0
    this.period = 0
    this.lastOnsetAt = Number.NaN
  }
}

function median(values: readonly number[]): number {
  if (!values.length) return 0
  const sorted = [...values].sort((left, right) => left - right)
  const middle = sorted.length >> 1
  return sorted.length % 2
    ? sorted[middle]!
    : (sorted[middle - 1]! + sorted[middle]!) / 2
}

function unit(value: number | undefined): number {
  return clamp(Number.isFinite(value) ? (value as number) : 0, 0, 1)
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
