import type { BehaviorKind, BehaviorQuality } from '../motion/behavior'
import type { MusicMode } from '../singing/musicSignal'
import {
  MAX_RECOVERY_MS,
  MIN_RECOVERY_MS,
  NOMINAL_RECOVERY_MS,
} from '../motion/behaviorScheduler'
import { isMusicMode } from '../singing/musicSignal'
import { SPEECH_GESTURES } from '../speech/phraseGestures'
import { SpeechFormTransition } from './speechFormTransition'

export interface Anime25DMotionUnit {
  behaviorId: string
  family: 'co-speech' | 'music' | 'performance'
  form: string
  kind: BehaviorKind
  timing: {
    startMs: number
    readyMs: number
    strokeStartMs: number
    strokePeakMs: number
    strokeEndMs: number
    relaxMs: number | null
    endMs: number | null
  }
  intensity: number
  quality: BehaviorQuality
}

export interface Anime25DBehaviorMotionSample {
  coSpeechGesture: Readonly<CoSpeechGestureMix>
  coSpeech: number
  coSpeechPower: number
  coSpeechQuality: Readonly<BehaviorQuality>
  music: number
  musicPower: number
  musicQuality: Readonly<BehaviorQuality>
  musicMode: MusicMode
}

interface MutableBehaviorMotionSample {
  coSpeechGesture: CoSpeechGestureMix
  coSpeech: number
  coSpeechPower: number
  coSpeechQuality: BehaviorQuality
  music: number
  musicPower: number
  musicQuality: BehaviorQuality
  musicMode: MusicMode
}

/** Relative shares; the common co-speech gate applies the envelope once. */
export interface CoSpeechGestureMix {
  hesitate: number
  tease: number
  'check-in': number
  question: number
  contrast: number
  laugh: number
  laughPulse: number
}

interface UnitRelease {
  /** Envelope level the unit was last drawn at. */
  from: number
  startedAt: number
  duration: number
}

interface LocalMotionUnit extends Omit<Anime25DMotionUnit, 'timing'> {
  timing: {
    start: number
    ready: number
    strokeStart: number
    strokePeak: number
    strokeEnd: number
    relax: number | null
    end: number | null
  }
  /** Last sampled envelope, so a retreat can start from what was drawn. */
  envelope: number
  /** Set once the unit leaves the plan; a second restatement never restarts it. */
  release: UnitRelease | null
  speechForm: SpeechFormTransition | null
}

const DEFAULT_QUALITY: BehaviorQuality = {
  extent: 1,
  tempo: 1,
  power: 1,
  fluidity: 0.8,
  directness: 0.72,
  rebound: 0.35,
  asymmetry: 0.2,
  density: 0.8,
}

/**
 * Samples renderer-neutral motion units on the player's monotonic clock.
 *
 * Only the families that modulate an existing pose generator land here.
 * Performance units carry their own pose and go to the expression controller,
 * so an unmatched family is skipped rather than folded into music.
 */
export class Anime25DBehaviorMotionController {
  private units: LocalMotionUnit[] = []
  /**
   * The player writes on its own clock and reads one `predictedControlTime`
   * ahead of it. A retreat that starts on the write clock is therefore already
   * a whole lead into itself on its first frame — 42% of a short one — which
   * is a snap toward rest, not a retreat. The drawn value belongs to the read
   * clock, so the retreat away from it starts there too.
   */
  private lastSampledAt = Number.NaN
  private readonly output: MutableBehaviorMotionSample = {
    coSpeechGesture: {
      question: 0,
      contrast: 0,
      laugh: 0,
      laughPulse: 0,
      hesitate: 0,
      tease: 0,
      'check-in': 0,
    },
    coSpeech: 0,
    coSpeechPower: 0,
    coSpeechQuality: { ...DEFAULT_QUALITY },
    music: 0,
    musicPower: 0,
    musicQuality: { ...DEFAULT_QUALITY },
    musicMode: 'listen',
  }

  replace(
    units: readonly Anime25DMotionUnit[],
    nowMs: number,
    playerTimeSeconds: number,
  ): void {
    const localOrigin = finite(playerTimeSeconds)
    const wallNow = finite(nowMs)
    const previous = new Map(this.units.map((unit) => [unit.behaviorId, unit]))
    const next: LocalMotionUnit[] = units
      .filter((unit) => unit.family === 'co-speech' || unit.family === 'music')
      .map((unit) => ({
        ...unit,
        timing: {
          start: localTime(unit.timing.startMs, wallNow, localOrigin),
          ready: localTime(unit.timing.readyMs, wallNow, localOrigin),
          strokeStart: localTime(
            unit.timing.strokeStartMs,
            wallNow,
            localOrigin,
          ),
          strokePeak: localTime(unit.timing.strokePeakMs, wallNow, localOrigin),
          strokeEnd: localTime(unit.timing.strokeEndMs, wallNow, localOrigin),
          relax:
            unit.timing.relaxMs === null
              ? null
              : localTime(unit.timing.relaxMs, wallNow, localOrigin),
          end:
            unit.timing.endMs === null
              ? null
              : localTime(unit.timing.endMs, wallNow, localOrigin),
        },
        envelope: 0,
        release: null,
        speechForm: null,
      }))
    for (const unit of next) {
      const old = previous.get(unit.behaviorId)
      // A second event can cancel before the next sample. Retain the level
      // actually drawn, even though this publish has not been sampled yet.
      if (old?.family === unit.family) unit.envelope = old.envelope
      if (unit.family !== 'co-speech') continue
      if (old?.speechForm && !old.release && old.envelope > 0) {
        unit.speechForm = old.speechForm
        unit.speechForm.revise(
          unit.form,
          this.releaseOrigin(localOrigin),
          unit.timing.strokePeak,
        )
      } else {
        unit.speechForm = new SpeechFormTransition(unit.form)
      }
    }
    const restated = new Set(next.map((unit) => unit.behaviorId))
    for (const unit of this.units) {
      if (restated.has(unit.behaviorId)) continue
      if (unit.release) {
        // Already retreating. Restating the plan is one release, not
        // permission to start the retreat over from the top.
        next.push(unit)
      } else if (unit.envelope > 0) {
        next.push(releasingUnit(unit, this.releaseOrigin(localOrigin)))
      }
    }
    this.units = next
  }

  /**
   * Retires every live unit through the same retreat a plan revision uses.
   *
   * This is the stop command, not teardown: the expression controller it is
   * called beside releases its cues rather than erasing them, and a body whose
   * head snapped straight while its face eased out was the visible half of
   * that disagreement.
   */
  clear(playerTimeSeconds: number): void {
    const now = finite(playerTimeSeconds)
    const releasing: LocalMotionUnit[] = []
    for (const unit of this.units) {
      const origin = this.releaseOrigin(now)
      if (unit.release) releasing.push(unit)
      else if (unit.envelope > 0) releasing.push(releasingUnit(unit, origin))
    }
    this.units = releasing
  }

  private releaseOrigin(fallback: number): number {
    return Number.isFinite(this.lastSampledAt) ? this.lastSampledAt : fallback
  }

  sample(timeSeconds: number): Readonly<Anime25DBehaviorMotionSample> {
    const now = finite(timeSeconds)
    this.lastSampledAt = now
    this.output.coSpeech = 0
    const gesture = this.output.coSpeechGesture
    gesture.question = gesture.contrast = gesture.laugh = gesture.laughPulse = 0
    gesture.hesitate = gesture.tease = gesture['check-in'] = 0
    this.output.coSpeechPower = 0
    this.output.music = 0
    this.output.musicPower = 0
    this.output.musicMode = 'listen'
    copyQuality(this.output.coSpeechQuality, DEFAULT_QUALITY)
    copyQuality(this.output.musicQuality, DEFAULT_QUALITY)
    let write = 0
    for (const unit of this.units) {
      if (unit.release) {
        if (now >= unit.release.startedAt + unit.release.duration) {
          unit.envelope = 0
          continue
        }
      } else if (unit.timing.end !== null && now >= unit.timing.end) {
        unit.envelope = 0
        continue
      }
      this.units[write] = unit
      write += 1
      const envelope = unit.release
        ? unit.release.from *
          (1 -
            smoothProgress(
              unit.release.startedAt,
              unit.release.startedAt + unit.release.duration,
              now,
            ))
        : unitEnvelope(unit.timing, now, unit.quality)
      unit.envelope = envelope
      if (envelope <= 0) continue
      const density = scaleAroundDefault(unit.quality.density, 0.8, 0.18)
      const extent =
        envelope *
        clamp(unit.intensity, 0.2, 1.4) *
        clamp(unit.quality.extent, 0.35, 1.4) *
        density
      const power = envelope * clamp(unit.quality.power, 0.35, 1.4)
      if (unit.family === 'co-speech') {
        const formShares = unit.speechForm?.sample(now)
        if (formShares) {
          for (const form of SPEECH_GESTURES)
            gesture[form] += extent * formShares[form]
          if (formShares.laugh > 0) {
            // A short chuckle follows this behavior's resolved clock. Never
            // restart an oscillator on a new frame or a plan restatement.
            gesture.laughPulse +=
              extent *
              formShares.laugh *
              Math.sin((now - unit.timing.strokePeak) * Math.PI * 4)
          }
        }
        if (extent >= this.output.coSpeech) {
          this.output.coSpeech = extent
          copyQuality(this.output.coSpeechQuality, unit.quality)
        }
        this.output.coSpeechPower = Math.max(this.output.coSpeechPower, power)
      } else if (unit.family === 'music' && isMusicMode(unit.form)) {
        if (extent >= this.output.music) {
          this.output.music = extent
          this.output.musicMode = unit.form
          copyQuality(this.output.musicQuality, unit.quality)
        }
        this.output.musicPower = Math.max(this.output.musicPower, power)
      }
    }
    this.units.length = write
    const denominator = Math.max(
      this.output.coSpeech,
      gesture.question +
        gesture.contrast +
        gesture.laugh +
        gesture.hesitate +
        gesture.tease +
        gesture['check-in'],
    )
    if (denominator > 0) {
      gesture.question /= denominator
      gesture.contrast /= denominator
      gesture.laugh /= denominator
      gesture.laughPulse /= denominator
      gesture.hesitate /= denominator
      gesture.tease /= denominator
      gesture['check-in'] /= denominator
    }
    return this.output
  }
}

export function completeBehaviorQuality(
  quality: Partial<BehaviorQuality> | undefined,
): BehaviorQuality {
  return {
    extent: clamp(finiteOr(quality?.extent, 1), 0.2, 1.6),
    tempo: clamp(finiteOr(quality?.tempo, 1), 0.45, 1.7),
    power: clamp(finiteOr(quality?.power, 1), 0.2, 1.6),
    fluidity: clamp(finiteOr(quality?.fluidity, 0.8), 0.2, 1.4),
    directness: clamp(finiteOr(quality?.directness, 0.72), 0.2, 1.4),
    rebound: clamp(finiteOr(quality?.rebound, 0.35), 0, 1.4),
    asymmetry: clamp(finiteOr(quality?.asymmetry, 0.2), 0, 1.4),
    density: clamp(finiteOr(quality?.density, 0.8), 0.2, 1.5),
  }
}

/**
 * A unit that left the plan retreats from the level it was last drawn at.
 *
 * Without this the extent stepped straight to zero on the frame the plan
 * changed — the body dropped a half-finished gesture while the face, which
 * has always released its cues from their current value, eased out of the
 * same beat. The retreat is shaped like the scheduler's own: further out and
 * slower delivery take longer to put away, inside the same bounds.
 */
function releasingUnit(
  unit: LocalMotionUnit,
  startedAt: number,
): LocalMotionUnit {
  unit.speechForm?.freeze(startedAt)
  const level = clamp(unit.envelope, 0, 1)
  const tempo = clamp(unit.quality.tempo, 0.45, 1.7)
  const durationMs = clamp(
    (NOMINAL_RECOVERY_MS *
      (0.45 + 0.55 * level) *
      clamp(unit.quality.extent, 0.2, 1.6)) /
      tempo,
    MIN_RECOVERY_MS,
    MAX_RECOVERY_MS,
  )
  return {
    ...unit,
    envelope: level,
    release: { from: level, startedAt, duration: durationMs / 1_000 },
  }
}

function unitEnvelope(
  timing: LocalMotionUnit['timing'],
  now: number,
  quality: Readonly<BehaviorQuality>,
): number {
  if (now < timing.start) return 0
  if (timing.end !== null && now >= timing.end) return 0
  if (now < timing.ready) {
    return mix(
      0,
      0.34,
      qualityProgress(timing.start, timing.ready, now, quality),
    )
  }
  if (now < timing.strokeStart) {
    return mix(
      0.34,
      0.62,
      qualityProgress(timing.ready, timing.strokeStart, now, quality),
    )
  }
  if (now < timing.strokePeak) {
    return mix(
      0.62,
      1,
      qualityProgress(timing.strokeStart, timing.strokePeak, now, quality),
    )
  }
  if (now < timing.strokeEnd) {
    const progress = qualityProgress(
      timing.strokePeak,
      timing.strokeEnd,
      now,
      quality,
    )
    return (
      mix(1, 0.84, progress) +
      Math.sin(progress * Math.PI) *
        deltaAroundDefault(quality.rebound, 0.35, 0.08)
    )
  }
  if (timing.relax === null) return 0.84
  if (now < timing.relax) {
    const progress = smoothProgress(timing.strokeEnd, timing.relax, now)
    const rebound = deltaAroundDefault(quality.rebound, 0.35, 0.07)
    return 0.84 + Math.sin(progress * Math.PI * 2) * rebound * (1 - progress)
  }
  if (timing.end === null) return 0.84
  return mix(0.84, 0, qualityProgress(timing.relax, timing.end, now, quality))
}

function localTime(atMs: number, nowMs: number, localNow: number): number {
  return localNow + (finite(atMs) - nowMs) / 1_000
}

function smoothProgress(start: number, end: number, now: number): number {
  if (end <= start) return now >= end ? 1 : 0
  const t = clamp((now - start) / (end - start), 0, 1)
  return t * t * (3 - 2 * t)
}

function qualityProgress(
  start: number,
  end: number,
  now: number,
  quality: Readonly<BehaviorQuality>,
): number {
  if (end <= start) return now >= end ? 1 : 0
  const raw = clamp((now - start) / (end - start), 0, 1)
  const tempo = clamp(quality.tempo, 0.45, 1.7)
  const paced = raw ** (1 / tempo)
  const smooth = paced * paced * (3 - 2 * paced)
  const fluidity = clamp((quality.fluidity - 0.2) / 1.2, 0, 1)
  const directness = clamp((quality.directness - 0.2) / 1.2, 0, 1)
  return mix(
    paced,
    smooth,
    clamp(0.2 + fluidity * 0.65 - directness * 0.18, 0, 1),
  )
}

function scaleAroundDefault(
  value: number,
  neutral: number,
  range: number,
): number {
  return 1 + clamp(value - neutral, -1, 1) * range
}

function deltaAroundDefault(
  value: number,
  neutral: number,
  range: number,
): number {
  return clamp(value - neutral, -1, 1) * range
}

function copyQuality(
  target: BehaviorQuality,
  source: Readonly<BehaviorQuality>,
): void {
  target.extent = source.extent
  target.tempo = source.tempo
  target.power = source.power
  target.fluidity = source.fluidity
  target.directness = source.directness
  target.rebound = source.rebound
  target.asymmetry = source.asymmetry
  target.density = source.density
}

function mix(start: number, end: number, amount: number): number {
  return start + (end - start) * amount
}

function finite(value: number): number {
  return Number.isFinite(value) ? value : 0
}

function finiteOr(value: number | undefined, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
