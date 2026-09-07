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

/**
 * Without a lock, an onset this soon after the last one is a subdivision.
 *
 * Accents were taken from whichever onset happened to pass a coin flip, which
 * is fine when the hits are already beat-spaced and wrong when they are not.
 * Measured across 34 excerpts of a real library: tracks with three or more
 * onsets a second nodded 23 times a minute against 4.6 for everything else,
 * three quarters of those while the beat was not locked — a head jab a couple
 * of seconds on hits that carried no accent. Nothing faster than the tempo
 * range the clock will even consider is a beat.
 */
const UNLOCKED_NOD_MIN_ONSET_GAP = MIN_BEAT_PERIOD

/**
 * How long the head takes to arrive at an accent.
 *
 * Minimum-jerk motion carries jerk proportional to depth over duration cubed,
 * so a 0.15 dip in 0.16s peaks near 2200 — a jab, not a nod. Measured through
 * the whole player on a dense percussion track, the accent was 86% of all the
 * pitch jerk there was (21 against 3 with it silenced, and 2 on sparse music).
 * The recovery has always been 0.45-0.71s; the arrival is what was sharp.
 */
const NOD_ARRIVAL_SECONDS = 0.23
const NOD_ARRIVAL_FLOOR = 0.13
/**
 * An accent placed without a lock is a guess; it moves less and arrives later.
 *
 * Measured through the whole player over 33 excerpts of a real library, with
 * every random source seeded so the runs are comparable. Pitch jerk, which is
 * where the head accent lands: 11.4 on dense percussion and 10.9 on sparse
 * music to start with. Refusing to guess at all got those to 5.6 and 2.5, but
 * stopped the head nodding entirely on the sparse music that never locks — 0
 * accents a minute. Guessing softly reaches 7.0 and 2.9 and keeps the rate
 * exactly where it was.
 */
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
const FIRST: Motif = { roll: 0.8, yaw: 0.3, torso: 0.58, offset: 0, nods: 0.7 }
const TAU = Math.PI * 2

/** Manner with no plan installed, and the rate it and the mode move at. */
const MANNER_REST = {
  extent: 1.08,
  density: 0.7,
  asymmetry: 0.36,
  directness: 0.58,
  fluidity: 0.92,
}
const MANNER_KEYS = Object.keys(MANNER_REST) as (keyof typeof MANNER_REST)[]
const MODE_RATE = 7

/**
 * Confidence at which the tempo estimate may set the sway speed.
 *
 * The estimate is a median over the last eight onset gaps, and on music with
 * dense low-band onsets it churns: measured across 46 excerpts of a real
 * library, this threshold was crossed 20-40 times a minute on a quarter of
 * them. Losing the lock is not the same as losing the music, so the speed we
 * last agreed on is kept rather than handed back to the generic fallback and
 * taken again — the swing was up to 1.98x, twice a second.
 */
const LOCK_CONFIDENCE = 0.45

/**
 * The body finishes winding down at the speed it was moving at.
 *
 * Confidence goes to zero the moment the music is taken away, and releasing
 * the tempo there made the sway slow to the generic idle drift while it was
 * still shrinking — running out of power rather than coming to a stop. Hold
 * it until there is no sway left to time. Measured over 42 excerpts of a real
 * library that had a sway to wind down, this took 84% off how far the tempo
 * travels while the body can still be seen moving to it.
 */
const TEMPO_RELEASE_AMPLITUDE = 0.05

/**
 * How much of the sway is held back while its speed is still moving.
 *
 * Entering music, the amplitude reaches nine tenths in a quarter second while
 * the speed takes more than twice that to settle, so the body swings at full
 * size and then changes tempo underneath itself. Joining in at reduced size
 * until the timing is agreed reads as picking the beat up rather than guessing
 * at it. Keyed on the speed having converged, not on the beat being locked:
 * music the estimator never locks onto settles on the idle fallback instead,
 * so the sway opens up there too.
 *
 * Adopting the tempo faster was tried first and measured worse on real music
 * (+5.2% over 45 excerpts): the estimate churns, so arriving at it sooner just
 * arrives at a wrong value sooner. Holding the size back instead took 28% off
 * how far the tempo travels while the body is visibly swaying to it, improved
 * 20 excerpts and worsened none, and left the settled sway alone — the median
 * amplitude after six seconds is unchanged, the worst single loss 9%.
 */
export const ENTRY_HELD_BACK_SHARE = 0.25
const ENTRY_ON_TEMPO_WITHIN = 0.05
export const ENTRY_OFF_TEMPO_BEYOND = 0.25

/**
 * How far the beat may drag the sway off its own speed while catching up.
 *
 * `phase` is the one driver the whole body reads — torso, head, arms and the
 * gaze arc are all sines of it — so a correction applied to it moves every
 * limb at once. Adding the error straight in bounded nothing: measured across
 * 46 excerpts of a real library, the phase velocity swung between 0.18 and
 * 0.51 cycles per second against a nominal 0.21, so the body sped up and slowed
 * down by two to three times while nothing about the music had changed.
 *
 * Catching up is a transition, not a jump. The body may run this much faster
 * or slower than its own tempo to get back on the beat, and no more.
 */
export const MAX_PHASE_CATCHUP = 0.35

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
  private lastOnsetAt = Number.NEGATIVE_INFINITY
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
  /** Sway rate of the last agreed tempo, kept while evidence is still present. */
  private lockedFrequency = 0
  /** 0 while the sway speed is still moving to its target, 1 once settled. */
  private onTempo = 1
  /**
   * The manner fields the pose reads on every frame.
   *
   * `modeAmount` next to them has always been eased, but the quality vector
   * behind it was applied raw, so a participation change stepped the yaw,
   * roll and torso amplitudes within one frame instead of moving to them.
   * `power` and `rebound` are absent on purpose: a nod reads those once when
   * it commits, and a step in a decision is not a step in a pose.
   */
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
    // Commit to the sway only as far as the timing is agreed. `onTempo` is
    // read from the previous frame: this frame's speed resolves further down.
    const targetAmplitude =
      (active ? smooth(energy / 0.55) : 0) *
      mix(ENTRY_HELD_BACK_SHARE, 1, this.onTempo)
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
    if (locked) { this.lockedFrequency = bpm / (60 * this.swayBeats)
}
    // Evidence gone, not merely uncertain — but keep the tempo until the sway
    // it was timing has actually stopped.
    else if (confidence <= 0 && this.amplitude < TEMPO_RELEASE_AMPLITUDE) {
      this.lockedFrequency = 0
    }
    const targetFrequency = this.lockedFrequency
      ? this.lockedFrequency
      : 0.18 * clamp((quality?.tempo ?? 0.82) / 0.82, 0.7, 1.25)
    const retiming =
      targetFrequency > 1e-6
        ? clamp(Math.abs(targetFrequency - this.frequency) / targetFrequency, 0, 1)
        : 0
    this.onTempo =
      1 -
      smooth(
        (retiming - ENTRY_ON_TEMPO_WITHIN) /
          (ENTRY_OFF_TEMPO_BEYOND - ENTRY_ON_TEMPO_WITHIN),
      )
    this.frequency = approach(this.frequency, targetFrequency, dt, 2)
    this.phase += dt * this.frequency * (active ? 1 : this.amplitude)
    if (locked && active) {
      const error = wrap(beatPosition / this.swayBeats + 0.12 - this.phase)
      const pull = error * (1 - Math.exp(-dt * 0.75 * confidence))
      const limit = this.frequency * dt * MAX_PHASE_CATCHUP
      this.phase += clamp(pull, -limit, limit)
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
    // Before the cooldown check: an onset that lands during it still tells us
    // how densely the hits are coming, and the first one after it must not
    // inherit the whole cooldown as its gap.
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
      lead = Math.max(NOD_ARRIVAL_FLOOR, until - 0.045)
    } else if (
      !onset ||
      pulse < 0.1 ||
      sinceLastOnset < UNLOCKED_NOD_MIN_ONSET_GAP ||
      this.random() > density * 0.7
    ) {
      return
    } else {
      // No lock: this accent is a guess about where the beat is, so make it
      // one. Same gesture, softer and slower — measured through the whole
      // player over 33 excerpts, taking the guessed accents out entirely got
      // the pitch jerk down furthest but stopped the head nodding at all on
      // the sparse music that never locks.
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
