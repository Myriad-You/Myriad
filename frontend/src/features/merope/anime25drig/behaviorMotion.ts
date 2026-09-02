import type { BehaviorKind, BehaviorQuality } from '../motion/behavior'

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
  coSpeech: number
  coSpeechPower: number
  coSpeechQuality: Readonly<BehaviorQuality>
  music: number
  musicPower: number
  musicQuality: Readonly<BehaviorQuality>
}

interface MutableBehaviorMotionSample {
  coSpeech: number
  coSpeechPower: number
  coSpeechQuality: BehaviorQuality
  music: number
  musicPower: number
  musicQuality: BehaviorQuality
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
  private readonly output: MutableBehaviorMotionSample = {
    coSpeech: 0,
    coSpeechPower: 0,
    coSpeechQuality: { ...DEFAULT_QUALITY },
    music: 0,
    musicPower: 0,
    musicQuality: { ...DEFAULT_QUALITY },
  }

  replace(
    units: readonly Anime25DMotionUnit[],
    nowMs: number,
    playerTimeSeconds: number,
  ): void {
    const localOrigin = finite(playerTimeSeconds)
    const wallNow = finite(nowMs)
    this.units = units
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
      }))
  }

  clear(): void {
    this.units = []
  }

  sample(timeSeconds: number): Readonly<Anime25DBehaviorMotionSample> {
    const now = finite(timeSeconds)
    this.output.coSpeech = 0
    this.output.coSpeechPower = 0
    this.output.music = 0
    this.output.musicPower = 0
    copyQuality(this.output.coSpeechQuality, DEFAULT_QUALITY)
    copyQuality(this.output.musicQuality, DEFAULT_QUALITY)
    let write = 0
    for (const unit of this.units) {
      if (unit.timing.end !== null && now >= unit.timing.end) continue
      this.units[write] = unit
      write += 1
      const envelope = unitEnvelope(unit.timing, now, unit.quality)
      if (envelope <= 0) continue
      const density = scaleAroundDefault(unit.quality.density, 0.8, 0.18)
      const extent =
        envelope *
        clamp(unit.intensity, 0.2, 1.4) *
        clamp(unit.quality.extent, 0.35, 1.4) *
        density
      const power = envelope * clamp(unit.quality.power, 0.35, 1.4)
      if (unit.family === 'co-speech') {
        if (extent >= this.output.coSpeech) {
          this.output.coSpeech = extent
          copyQuality(this.output.coSpeechQuality, unit.quality)
        }
        this.output.coSpeechPower = Math.max(this.output.coSpeechPower, power)
      } else if (unit.family === 'music') {
        if (extent >= this.output.music) {
          this.output.music = extent
          copyQuality(this.output.musicQuality, unit.quality)
        }
        this.output.musicPower = Math.max(this.output.musicPower, power)
      }
    }
    this.units.length = write
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
