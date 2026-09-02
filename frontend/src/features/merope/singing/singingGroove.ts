import type { BehaviorQuality } from '../motion/behavior'
import type { MusicMode, MusicMotionSignal } from './musicSignal'
import { MinimumJerkMotion } from '../anime25drig/minimumJerk'
import {
  MUSIC_PHRASE_PREPARATION_SECONDS,
  MUSIC_PHRASE_RELEASE_SECONDS,
} from './musicPhrases'

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

export const MIN_SINGING_NOD_INTERVAL_SECONDS = 1.05

export function singingNodBeatStride(bpm: number): 1 | 2 | 4 {
  if (!(bpm > 0) || !Number.isFinite(bpm)) return 1
  if (60 / bpm >= MIN_SINGING_NOD_INTERVAL_SECONDS) return 1
  return 120 / bpm >= MIN_SINGING_NOD_INTERVAL_SECONDS ? 2 : 4
}

interface Motif {
  roll: number
  yaw: number
  torso: number
  offset: number
  nods: number
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
const FIRST: Motif = { roll: 0.8, yaw: 0.3, torso: 0.58, offset: 0, nods: 0.7 }
const TAU = Math.PI * 2

/**
 * Multilevel entrainment: slow weight transfer, softer head following, optional
 * committed accents and persistent motifs. No pose springs here: the player's
 * one C2 response carries actual position/velocity/acceleration across sources.
 * Research principles (coefficients are rig-space art direction):
 * Toiviainen & Carlson 2022 — https://doi.org/10.1525/mp.2022.39.3.249
 * Burger et al. 2014 — https://doi.org/10.3389/fnhum.2014.00903
 * Livingstone & Palmer 2016 — https://doi.org/10.1037/emo0000106
 */
export class SingingGrooveController {
  private readonly output = { ...ZERO }
  private readonly nod = new MinimumJerkMotion()
  private lastTime = Number.NaN
  private observedAt = Number.NaN
  private sampleTime = Number.NaN
  private phase = 0
  private frequency = 0.22
  private amplitude = 0
  private phraseAmount = 0
  private modeAmount = 0
  private lastNodAt = Number.NEGATIVE_INFINITY
  private nodReleaseAt = Number.POSITIVE_INFINITY
  private nodRecovery = 0.6
  private lastNodBeat = -1
  private stride: 1 | 2 | 4 = 2
  private swayBeats: 4 | 8 = 4
  private previous: Motif = { ...FIRST }
  private motif: Motif = { ...FIRST }
  private motifAt = 0
  private nextMotifAt = 0
  private lastPhraseStart = Number.NaN
  private seed = 0x5E71C3
  private trackId: string | null = null
  private armMotion = false

  setArmMotion(enabled: boolean): void {
    this.armMotion = enabled
  }

  setTrack(trackId: string | null): void {
    if (trackId === this.trackId) return
    this.trackId = trackId
    this.seed = 2166136261
    for (const char of trackId ?? '')
      this.seed = Math.imul(this.seed ^ char.charCodeAt(0), 16777619) >>> 0
    this.sampleTime = Number.NaN
    this.observedAt = Number.NaN
    this.lastNodBeat = -1
    this.lastPhraseStart = Number.NaN
    this.nextMotifAt = 0
    // The body phase and the in-flight nod belong to the body, not the track.
  }

  sample(
    timeSeconds: number,
    enabled: boolean,
    signal: Readonly<MusicMotionSignal> | null,
    quality?: Readonly<BehaviorQuality>,
    mode: MusicMode = 'listen',
  ): Readonly<SingingGroovePose> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    const dt = Number.isFinite(this.lastTime)
      ? clamp(now - this.lastTime, 0, 0.1)
      : 0
    this.lastTime = now
    const fresh =
      signal !== null && signal.sampleTimeSeconds !== this.sampleTime
    if (fresh) {
      this.sampleTime = signal.sampleTimeSeconds
      this.observedAt = now
    }
    const age = Number.isFinite(this.observedAt)
      ? now - this.observedAt
      : Infinity
    const freshness = 1 - smooth((age - 0.15) / 0.3)
    const evidence = signal?.beatFrame
    const confidence = enabled ? (evidence?.confidence ?? 0) * freshness : 0
    const locked = confidence >= 0.45 && (evidence?.bpm ?? 0) > 0
    const bpm = locked ? evidence!.bpm : 0
    const mediaTime = signal
      ? signal.sampleTimeSeconds + Math.min(age, 0.15)
      : 0
    const beatPosition =
      evidence && bpm > 0
        ? evidence.beatCount +
          evidence.beatPhase +
          (Math.min(age, 0.15) * bpm) / 60
        : 0
    // Unavailable audio permits quiet listening, but never invents a beat.
    const energy = signal
      ? (signal.audio?.energy ?? (mode === 'sing' ? 0.38 : 0.1)) * freshness
      : 0
    const active = enabled && mode !== 'settle'
    const targetAmplitude = active ? smooth(energy / 0.55) : 0
    this.amplitude = approach(
      this.amplitude,
      targetAmplitude,
      dt,
      targetAmplitude > this.amplitude ? 9 : 2.8,
    )
    this.modeAmount = approach(
      this.modeAmount,
      mode === 'sing' ? 1 : mode === 'hum' ? 0.4 : 0,
      dt,
      7,
    )

    const phraseStart = signal?.phrase?.start
    const phraseChanged =
      phraseStart !== undefined && phraseStart !== this.lastPhraseStart
    if (active && (now >= this.nextMotifAt || phraseChanged)) {
      this.chooseMotif(now, bpm)
      if (phraseStart !== undefined) this.lastPhraseStart = phraseStart
    }
    const blend = smooth((now - this.motifAt) / 0.65)
    const roll = mix(this.previous.roll, this.motif.roll, blend)
    const yaw = mix(this.previous.yaw, this.motif.yaw, blend)
    const torso = mix(this.previous.torso, this.motif.torso, blend)
    const offset = mix(this.previous.offset, this.motif.offset, blend)

    // A full sway spans 4 or 8 pulses, not every neck accent. Soft coupling
    // permits a stable phase preference instead of snapping on each onset.
    if (bpm > 118) this.swayBeats = 8
    else if (bpm > 0 && bpm < 106) this.swayBeats = 4
    const targetFrequency = locked
      ? bpm / (60 * this.swayBeats)
      : 0.18 * clamp((quality?.tempo ?? 0.82) / 0.82, 0.7, 1.25)
    this.frequency = approach(this.frequency, targetFrequency, dt, 2)
    this.phase += dt * this.frequency * (active ? 1 : this.amplitude)
    if (locked && active) {
      const error = wrap(beatPosition / this.swayBeats + 0.12 - this.phase)
      this.phase += error * (1 - Math.exp(-dt * 0.75 * confidence))
    }
    this.phase %= 1

    if (now >= this.nodReleaseAt) {
      this.nod.retarget(this.nodReleaseAt, 0, this.nodRecovery)
      this.nodReleaseAt = Infinity
    }
    this.nod.sample(now)
    if (active && energy > 0.08) {
      this.planNod(
        now,
        bpm,
        beatPosition,
        confidence,
        fresh && !!evidence?.onset,
        signal?.audio?.pulse ?? 0,
        quality,
      )
    }
    const phrase = signal?.phrase
    let phraseTarget = 0
    if (active && phrase) {
      const attack = smooth(
        (mediaTime - phrase.start + MUSIC_PHRASE_PREPARATION_SECONDS) /
          MUSIC_PHRASE_PREPARATION_SECONDS,
      )
      const release =
        1 - smooth((mediaTime - phrase.end) / MUSIC_PHRASE_RELEASE_SECONDS)
      phraseTarget = attack * release * phrase.confidence
    }
    this.phraseAmount = approach(this.phraseAmount, phraseTarget, dt, 12)
    const density = clamp(quality?.density ?? 0.7, 0.2, 1.5)
    const asymmetry = clamp(quality?.asymmetry ?? 0.36, 0, 1.4)
    const extent =
      this.amplitude *
      (0.82 + 0.18 * this.modeAmount) *
      clamp((quality?.extent ?? 1.08) / 1.08, 0.6, 1.2)
    const torsoWave = Math.sin(TAU * this.phase)
    const headDelay = 0.06 + clamp(quality?.fluidity ?? 0.92, 0.2, 1.4) * 0.075
    const headWave = Math.sin(TAU * (this.phase - this.frequency * headDelay))
    const arc = Math.sin(TAU * (this.phase - 0.16))
    this.output.body = extent * (torsoWave * torso + offset * 0.12)
    this.output.angleZ =
      extent * (headWave * roll + offset * (0.3 + asymmetry * 0.2))
    this.output.angleX =
      extent *
      ((arc * yaw * clamp(quality?.directness ?? 0.58, 0.3, 1.2)) / 0.58 +
        offset * 0.14)
    // Modest downward accents. Phrase lift is not pitch-frequency tracking.
    this.output.angleY =
      extent *
      (this.nod.value +
        this.phraseAmount * 0.13 +
        this.modeAmount * 0.025 +
        arc * 0.025 * density)
    this.output.armY = this.armMotion
      ? extent * (Math.abs(torsoWave) * 0.18 + this.phraseAmount * 0.2)
      : 0
    this.output.armPos = this.armMotion
      ? extent * (-torsoWave * 0.25 + offset * 0.1)
      : 0
    this.output.eyeX = 0
    this.output.brow = 0
    return this.output
  }

  private planNod(
    now: number,
    bpm: number,
    position: number,
    confidence: number,
    onset: boolean,
    pulse: number,
    quality?: Readonly<BehaviorQuality>,
  ): void {
    if (now - this.lastNodAt < MIN_SINGING_NOD_INTERVAL_SECONDS) return
    const density = clamp(
      (quality?.density ?? 0.7) * this.motif.nods,
      0.15,
      0.95,
    )
    let lead = 0.16
    if (confidence >= 0.45 && bpm > 0) {
      const proposed = singingNodBeatStride(bpm)
      if (
        proposed > this.stride ||
        (60 * proposed) / bpm > MIN_SINGING_NOD_INTERVAL_SECONDS * 1.12
      ) {
        this.stride = proposed
}
      const beat = Math.ceil(position)
      const until = ((beat - position) * 60) / bpm
      if (
        until > 0.24 ||
        until < 0.055 ||
        beat % this.stride !== 0 ||
        beat === this.lastNodBeat
      ) {
        return
}
      this.lastNodBeat = beat
      if (this.random() > density) return
      // Commit once: subsequent beat corrections cannot retime this accent.
      lead = Math.max(0.09, until - 0.045)
    } else if (!onset || pulse < 0.1 || this.random() > density * 0.7) {
      return
    }
    this.lastNodAt = now
    const power = clamp((quality?.power ?? 0.78) / 0.78, 0.6, 1.25)
    const depth =
      (0.1 + Math.min(1, pulse) * 0.07) * (0.8 + this.random() * 0.3) * power
    this.nod.retarget(now, -depth, lead)
    this.nodReleaseAt = now + lead
    this.nodRecovery =
      (0.45 + this.random() * 0.26) *
      clamp(1.15 - (quality?.rebound ?? 0.3) * 0.3, 0.75, 1.2)
  }

  private chooseMotif(now: number, bpm: number): void {
    const blend = smooth((now - this.motifAt) / 0.65)
    for (const key of Object.keys(this.previous) as (keyof Motif)[])
      this.previous[key] = mix(this.previous[key], this.motif[key], blend)
    const choice = this.random()
    const side = this.random() * 2 - 1
    this.motif =
      choice < 0.38
        ? { roll: 0.82, yaw: 0.24, torso: 0.62, offset: side * 0.15, nods: 0.8 }
        : choice < 0.65
          ? {
              roll: 0.52,
              yaw: 0.45,
              torso: 0.4,
              offset: side * 0.4,
              nods: 0.55,
            }
          : choice < 0.85
            ? {
                roll: 0.36,
                yaw: 0.24,
                torso: 0.76,
                offset: side * 0.2,
                nods: 1,
              }
            : {
                roll: 0.16,
                yaw: 0.12,
                torso: 0.18,
                offset: side * 0.85,
                nods: 0.25,
              }
    this.motifAt = now
    this.nextMotifAt =
      now +
      (bpm > 0
        ? (60 / bpm) * (this.random() < 0.6 ? 8 : 12)
        : 3.5 + this.random() * 3)
  }

  private random(): number {
    this.seed ^= this.seed << 13
    this.seed ^= this.seed >>> 17
    this.seed ^= this.seed << 5
    return (this.seed >>> 0) / 4294967296
  }
}

function approach(
  value: number,
  target: number,
  dt: number,
  rate: number,
): number {
  return target + (value - target) * Math.exp(-dt * rate)
}
function mix(a: number, b: number, t: number): number {
  return a + (b - a) * t
}
function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
function smooth(value: number): number {
  const t = clamp(value, 0, 1)
  return t * t * t * (t * (t * 6 - 15) + 10)
}
function wrap(value: number): number {
  return ((((value + 0.5) % 1) + 1) % 1) - 0.5
}
