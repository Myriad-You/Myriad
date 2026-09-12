import type { BehaviorQuality } from '../motion/behavior'
import type { MusicMode, MusicMotionSignal } from './musicSignal'
import { MinimumJerkMotion } from '../anime25drig/minimumJerk'
import { MIN_BEAT_PERIOD } from './beatClock'
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

/** Without a lock, an onset this soon after the last one is a subdivision. */
const UNLOCKED_NOD_MIN_ONSET_GAP = MIN_BEAT_PERIOD

const NOD_ARRIVAL_SECONDS = 0.42
const NOD_ARRIVAL_FLOOR = 0.32
/** An accent placed without a lock is a guess */
const GUESSED_NOD_DEPTH_SCALE = 0.5
const GUESSED_NOD_ARRIVAL_SCALE = 1.3

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
const FIRST: Motif = { roll: 0.8, yaw: 0.3, torso: 0.76, offset: 0, nods: 0.7 }
const MOTIF_KEYS = Object.keys(FIRST) as (keyof Motif)[]
const TAU = Math.PI * 2

const MANNER_REST = {
  extent: 1.08,
  density: 0.7,
  asymmetry: 0.36,
  directness: 0.58,
  fluidity: 0.92,
}
const MANNER_KEYS = Object.keys(MANNER_REST) as (keyof typeof MANNER_REST)[]
const MODE_RATE = 7

const LOCK_CONFIDENCE = 0.45

const TEMPO_RELEASE_AMPLITUDE = 0.05

export const ENTRY_HELD_BACK_SHARE = 0.25
const ENTRY_ON_TEMPO_WITHIN = 0.05
export const ENTRY_OFF_TEMPO_BEYOND = 0.25

/** Catching up is a transition, not a jump. */
export const MAX_PHASE_CATCHUP = 0.35

export class SingingGrooveController {
  private readonly output = { ...ZERO }
  private readonly nod = new MinimumJerkMotion()
  private lastTime = Number.NaN
  private observedAt = Number.NaN
  private sampleTime = Number.NaN
  private phase = 0
  private frequency = 0.22
  private amplitude = 0
  private amplitudeDrive = 0
  private energyEnvelope = 0
  private phaseCorrection = 0
  private participation = 0
  private phraseAmount = 0
  private modeAmount = 0
  private lastNodAt = Number.NEGATIVE_INFINITY
  private nodReleaseAt = Number.POSITIVE_INFINITY
  private nodRecovery = 0.6
  private lastNodBeat = -1
  private lastOnsetAt = Number.NEGATIVE_INFINITY
  private stride: 1 | 2 | 4 = 2
  private swayBeats: 4 | 8 = 4
  private readonly motifMotion = Object.fromEntries(
    MOTIF_KEYS.map((key) => {
      const motion = new MinimumJerkMotion()
      motion.retarget(-2, FIRST[key], 1)
      motion.sample(0)
      return [key, motion]
    }),
  ) as Record<keyof Motif, MinimumJerkMotion>

  private motif: Motif = { ...FIRST }
  private motifAt = 0
  private nextMotifAt = 0
  private lastPhraseStart = Number.NaN
  private seed = 0x5E71C3
  private trackId: string | null = null
  private armMotion = false
  private lockedFrequency = 0
  private onTempo = 1
  private readonly manner = { ...MANNER_REST }

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
    const locked = confidence >= LOCK_CONFIDENCE && (evidence?.bpm ?? 0) > 0
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
    // A drum transient belongs to the accent planner, not to the radius of
    // an already moving head. Retain musical energy across short beat gaps.
    this.energyEnvelope = approach(
      this.energyEnvelope,
      energy,
      dt,
      energy > this.energyEnvelope ? 8 : 1.2,
    )
    // Enter cautiously, but never contract the whole body when a running
    // beat estimate is corrected. Only a real release resets participation.
    this.participation = active
      ? Math.max(this.participation, this.onTempo)
      : approach(this.participation, 0, dt, 3)
    const targetAmplitude =
      (active ? smooth(this.energyEnvelope / 0.55) : 0) *
      mix(ENTRY_HELD_BACK_SHARE, 1, this.participation)
    this.amplitudeDrive = approach(this.amplitudeDrive, targetAmplitude, dt, 6)
    this.amplitude = approach(this.amplitude, this.amplitudeDrive, dt, 6)
    this.modeAmount = approach(
      this.modeAmount,
      mode === 'sing' ? 1 : mode === 'hum' ? 0.4 : 0,
      dt,
      MODE_RATE,
    )
    for (const key of MANNER_KEYS) {
      this.manner[key] = approach(
        this.manner[key],
        quality?.[key] ?? MANNER_REST[key],
        dt,
        MODE_RATE,
      )
    }

    const phraseStart = signal?.phrase?.start
    const phraseChanged =
      phraseStart !== undefined && phraseStart !== this.lastPhraseStart
    if (
      active &&
      (now >= this.nextMotifAt || (phraseChanged && now - this.motifAt >= 0.8))
    ) {
      this.chooseMotif(now, bpm)
      if (phraseStart !== undefined) this.lastPhraseStart = phraseStart
    }
    const roll = this.motifMotion.roll.sample(now)
    const yaw = this.motifMotion.yaw.sample(now)
    const torso = this.motifMotion.torso.sample(now)
    const offset = this.motifMotion.offset.sample(now)

    // Soft coupling permits a stable phase preference instead of snapping on each onset.
    if (bpm > 118) this.swayBeats = 8
    else if (bpm > 0 && bpm < 106) this.swayBeats = 4
    if (locked) {
      this.lockedFrequency = bpm / (60 * this.swayBeats)
    } else if (confidence <= 0 && this.amplitude < TEMPO_RELEASE_AMPLITUDE) {
      this.lockedFrequency = 0
    }
    const targetFrequency = this.lockedFrequency
      ? this.lockedFrequency
      : 0.18 * clamp((quality?.tempo ?? 0.82) / 0.82, 0.7, 1.25)
    const retiming =
      targetFrequency > 1e-6
        ? clamp(
            Math.abs(targetFrequency - this.frequency) / targetFrequency,
            0,
            1,
          )
        : 0
    this.onTempo =
      1 -
      smooth(
        (retiming - ENTRY_ON_TEMPO_WITHIN) /
          (ENTRY_OFF_TEMPO_BEYOND - ENTRY_ON_TEMPO_WITHIN),
      )
    this.frequency = approach(this.frequency, targetFrequency, dt, 2)
    this.phase += dt * this.frequency * (active ? 1 : this.amplitude)
    let correction = 0
    if (locked && active) {
      const error = wrap(beatPosition / this.swayBeats + 0.12 - this.phase)
      correction = clamp(
        (error * 0.75 * confidence) / Math.max(this.frequency, 1e-6),
        -MAX_PHASE_CATCHUP,
        MAX_PHASE_CATCHUP,
      )
    }
    this.phaseCorrection = approach(this.phaseCorrection, correction, dt, 2)
    this.phase +=
      this.phaseCorrection * this.frequency * dt * (active ? 1 : this.amplitude)
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
    const density = clamp(this.manner.density, 0.2, 1.5)
    const asymmetry = clamp(this.manner.asymmetry, 0, 1.4)
    const extent =
      this.amplitude *
      (0.82 + 0.18 * this.modeAmount) *
      clamp(this.manner.extent / 1.08, 0.6, 1.2)
    const torsoWave = Math.sin(TAU * this.phase)
    const headDelay = 0.06 + clamp(this.manner.fluidity, 0.2, 1.4) * 0.075
    const headWave = Math.sin(TAU * (this.phase - this.frequency * headDelay))
    const arc = Math.sin(TAU * (this.phase - 0.16))
    this.output.body = extent * (torsoWave * torso + offset * 0.12)
    this.output.angleZ =
      extent * (headWave * roll + offset * (0.3 + asymmetry * 0.2))
    this.output.angleX =
      extent *
      ((arc * yaw * clamp(this.manner.directness, 0.3, 1.2)) / 0.58 +
        offset * 0.14)
    this.output.angleY =
      extent *
      (this.nod.value +
        this.phraseAmount * 0.13 +
        this.modeAmount * 0.025 +
        arc * 0.025 * density)
    // Arms follow the torso with a small lag. A squared wave has no cusp at
    // the centre crossing, unlike abs(sin), even with a larger excursion.
    const armWave = Math.sin(TAU * (this.phase - this.frequency * 0.18))
    const armExtent = extent * clamp(torso / 0.8, 0.2, 1.15)
    this.output.armY = this.armMotion
      ? armExtent * (armWave * armWave * 0.3 + this.phraseAmount * 0.2)
      : 0
    this.output.armPos = this.armMotion
      ? armExtent * (-armWave * 0.42 + offset * 0.1)
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
    const sinceLastOnset = onset ? now - this.lastOnsetAt : Infinity
    if (onset) this.lastOnsetAt = now
    if (now - this.lastNodAt < MIN_SINGING_NOD_INTERVAL_SECONDS) return
    const density = clamp(
      (quality?.density ?? 0.7) * this.motif.nods,
      0.15,
      0.95,
    )
    let lead = NOD_ARRIVAL_SECONDS
    let guessed = false
    if (confidence >= 0.45 && bpm > 0) {
      const proposed = singingNodBeatStride(bpm)
      if (
        proposed > this.stride ||
        (60 * proposed) / bpm > MIN_SINGING_NOD_INTERVAL_SECONDS * 1.12
      ) {
        this.stride = proposed
      }
      // Look ahead to the chosen metrical beat, not merely the next beat:
      // at fast tempi a single beat is shorter than a natural preparation.
      const beat = Math.ceil(position / this.stride) * this.stride
      const until = ((beat - position) * 60) / bpm
      if (
        until > 0.5 ||
        until < NOD_ARRIVAL_FLOOR + 0.045 ||
        beat % this.stride !== 0 ||
        beat === this.lastNodBeat
      ) {
        return
      }
      this.lastNodBeat = beat
      if (this.random() > density) return
      // Commit once: subsequent beat corrections cannot retime this accent.
      lead = Math.max(NOD_ARRIVAL_FLOOR, until - 0.045)
    } else if (
      !onset ||
      pulse < 0.1 ||
      sinceLastOnset < UNLOCKED_NOD_MIN_ONSET_GAP ||
      this.random() > density * 0.7
    ) {
      return
    } else {
      guessed = true
    }
    this.lastNodAt = now
    const power = clamp((quality?.power ?? 0.78) / 0.78, 0.6, 1.25)
    if (guessed) lead *= GUESSED_NOD_ARRIVAL_SCALE
    const depth =
      (0.1 + Math.min(1, pulse) * 0.07) *
      (0.8 + this.random() * 0.3) *
      power *
      (guessed ? GUESSED_NOD_DEPTH_SCALE : 1)
    this.nod.retarget(now, -depth, lead)
    this.nodReleaseAt = now + lead
    this.nodRecovery =
      (0.45 + this.random() * 0.26) *
      clamp(1.15 - (quality?.rebound ?? 0.3) * 0.3, 0.75, 1.2)
  }

  private chooseMotif(now: number, bpm: number): void {
    const choice = this.random()
    const side = this.random() * 2 - 1
    this.motif =
      choice < 0.38
        ? { roll: 0.82, yaw: 0.24, torso: 0.8, offset: side * 0.15, nods: 0.8 }
        : choice < 0.65
          ? {
              roll: 0.52,
              yaw: 0.45,
              torso: 0.65,
              offset: side * 0.4,
              nods: 0.55,
            }
          : choice < 0.85
            ? {
                roll: 0.36,
                yaw: 0.24,
                torso: 0.9,
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
    // Carry value, velocity and acceleration through phrase revisions and
    // track changes. Two beats (bounded in wall time) let the body finish
    // a gesture rather than restarting its transition at every lyric line.
    const travel = bpm > 0 ? clamp(120 / bpm, 1, 1.6) : 1.3
    for (const key of MOTIF_KEYS)
      this.motifMotion[key].retarget(now, this.motif[key], travel)
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
