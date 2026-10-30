/** Outside this an "interval" is a fill or a dropout, not a tempo. */
export const MIN_BEAT_PERIOD = 0.3
export const MAX_BEAT_PERIOD = 1

/** Two onsets closer than this are one hit, not a tempo. */
const MIN_ONSET_GAP = 0.12
const EVIDENCE_GRACE = 1.6
const EVIDENCE_EXPIRY = 3.2
const FLUX_THRESHOLD = 0.045
const INTERVAL_MEMORY = 8
const PHASE_PULL = 0.28

export interface BeatFrame {
  beatPhase: number
  beatCount: number
  bpm: number
  confidence: number
  beatCrossed: boolean
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

  private forgetTempo(): void {
    this.intervals.length = 0
    this.period = 0
    this.lastOnsetAt = Number.NaN
  }
}

function median(values: readonly number[]): number {
  if (!values.length) return 0
  const sorted = values.toSorted((left, right) => left - right)
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
