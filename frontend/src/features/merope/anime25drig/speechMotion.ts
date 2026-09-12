import type { SpeechViseme } from '../rig/articulation'
import type { TextVisemeCue } from './textVisemes'
import {
  isMajorVisualSpeechPause,
  MAX_VISUAL_SPEECH_TEXT_UNITS,
  visualSpeechPauseActivity,
} from '../speech/textTiming'
import {
  SPEECH_ACCENT_BROW_ATTACK,
  SPEECH_ACCENT_BROW_RELEASE,
  SPEECH_ACCENT_HEAD_ATTACK,
  SPEECH_ACCENT_HEAD_DELAY,
  SPEECH_ACCENT_HEAD_RELEASE,
  SPEECH_TEXT_ACCENT_ATTACK,
  SPEECH_TEXT_ACCENT_RELEASE,
} from './speechExpression'
import { compileTextVisemes } from './textVisemes'

export interface AutoSpeechPose {
  mouthOpen: number
  mouthWide: number
  mouthRound: number
  mouthNarrow: number
  mouthSeal: number
  mouthForm: number
  phraseActivity: number
  browAccent: number
  headAccent: number
}

type RandomSource = () => number
type TextVisemeCompiler = typeof compileTextVisemes

const REST_RELEASE = 0.2
const FALLBACK_SYLLABLE_MIN = 0.185
const FALLBACK_SYLLABLE_MAX = 0.275
const TEXT_PHRASE_PACE_MIN = 0.92
const TEXT_PHRASE_PACE_MAX = 1.08
const TEXT_LOCAL_PACE_MIN = 0.94
const TEXT_LOCAL_PACE_MAX = 1.06

const ZERO_SPEECH: AutoSpeechPose = {
  mouthOpen: 0,
  mouthWide: 0,
  mouthRound: 0,
  mouthNarrow: 0,
  mouthSeal: 0,
  mouthForm: 0,
  phraseActivity: 0,
  browAccent: 0,
  headAccent: 0,
}

export function speechPhraseAmplitudeScale(progress: number): number {
  const bounded = unitInterval(progress)
  const onset = mix(0.88, 1, smootherstep(bounded / 0.16))
  const ending = mix(1, 0.82, smootherstep((bounded - 0.72) / 0.28))
  return onset * ending
}

export function speechPhraseIntervalScale(progress: number): number {
  const bounded = unitInterval(progress)
  return mix(1, 1.18, smootherstep((bounded - 0.72) / 0.28))
}

export class AutoSpeechController {
  private readonly output: AutoSpeechPose = { ...ZERO_SPEECH }
  private initialized = false
  private enabled = false
  private speaking = false
  private nextEventAt = Number.POSITIVE_INFINITY
  private phraseStartedAt = 0
  private phraseEndsAt = 0
  private phraseDuration = 0
  private syllablesRemaining = 0
  private emphasisCooldown = 0
  private emphasisStartedAt = Number.NEGATIVE_INFINITY
  private transitionStartedAt = 0
  private transitionDuration = 0.08
  private fromOpen = 0
  private toOpen = 0
  private fromForm = 0
  private toForm = 0
  private fromWide = 0
  private toWide = 0
  private fromRound = 0
  private toRound = 0
  private fromNarrow = 0
  private toNarrow = 0
  private fromPhrase = 0
  private fromBrow = 0
  private fromHead = 0
  private restRelease = false
  private textMode = false
  private textCues: TextVisemeCue[] = []
  private textCueIndex = 0
  private textCueStartedAt = Number.NaN
  private textCueDuration = Number.NaN
  private textPhraseStartedAt = Number.NaN
  private textPhrasePace = 1
  private textLocalPace = 1
  private textPhraseStartPending = true
  private previousViseme: SpeechViseme = 'rest'
  private activeTextAccentIndex = -1
  private nextTextAccentAt = 0
  private textCompilation: Promise<void> = Promise.resolve()
  private textGeneration = 0
  private pendingTextCompilations = 0
  private provisionalTextActive = false

  constructor(
    private readonly random: RandomSource = Math.random,
    private readonly compileVisemes: TextVisemeCompiler = compileTextVisemes,
  ) {}

  clear(timeSeconds = 0): void {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    this.initialized = true
    this.enabled = false
    this.beginRestRelease(now)
  }

  enqueueText(text: string, locale?: string): void {
    const generation = this.textGeneration
    this.textMode = true
    this.pendingTextCompilations += 1
    this.textCompilation = this.textCompilation.then(async () => {
      try {
        const cues = await this.compileVisemes(text, locale)
        if (generation !== this.textGeneration || cues.length === 0) return
        this.appendTextCues(cues)
      } catch {
        return
      } finally {
        if (generation === this.textGeneration) {
          this.pendingTextCompilations = Math.max(
            0,
            this.pendingTextCompilations - 1,
          )
        }
      }
    })
  }

  private appendTextCues(cues: readonly TextVisemeCue[]): void {
    if (this.provisionalTextActive) {
      this.provisionalTextActive = false
      this.speaking = false
      this.nextEventAt = Number.POSITIVE_INFINITY
      this.previousViseme = closestViseme(this.output)
    }
    if (this.textCueIndex > 0) {
      this.activeTextAccentIndex =
        this.activeTextAccentIndex >= this.textCueIndex
          ? this.activeTextAccentIndex - this.textCueIndex
          : -1
      this.textCues = this.textCues.slice(this.textCueIndex)
      this.textCueIndex = 0
    }
    const available = Math.max(
      0,
      MAX_VISUAL_SPEECH_TEXT_UNITS * 2 - this.textCues.length,
    )
    this.textCues.push(...cues.slice(0, available))
  }

  sample(timeSeconds: number, enabled: boolean): Readonly<AutoSpeechPose> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    if (!this.initialized) {
      this.initialized = true
      this.enabled = enabled
      this.nextEventAt = enabled
        ? now + this.randomRange(0.08, 0.16)
        : Number.POSITIVE_INFINITY
    }

    if (enabled !== this.enabled) {
      this.enabled = enabled
      if (!enabled) {
        this.beginRestRelease(now)
        return this.resolveRest(now)
      }
      this.restRelease = false
      this.nextEventAt = now + this.randomRange(0.08, 0.16)
    }

    if (!this.enabled) return this.resolveRest(now)

    if (this.textMode) {
      if (
        !this.textCues[this.textCueIndex] &&
        this.pendingTextCompilations > 0
      ) {
        return this.sampleProvisionalText(now)
      }
      return this.sampleText(now)
    }

    this.resolve(now)
    for (let event = 0; event < 12 && now >= this.nextEventAt; event += 1) {
      const scheduledAt = this.nextEventAt
      this.advance(scheduledAt)
      this.resolve(now)
    }
    return this.output
  }

  private advance(now: number): void {
    if (!this.speaking) {
      this.speaking = true
      this.phraseStartedAt = now
      this.phraseDuration = this.randomRange(1.5, 3.1)
      this.phraseEndsAt = now + this.phraseDuration
      this.syllablesRemaining = this.randomInteger(2, 5)
      this.emphasisCooldown = 0
      this.scheduleSyllable(now)
      return
    }

    if (now >= this.phraseEndsAt) {
      this.speaking = false
      this.beginTransition(now, 0, 0, 0, 0, 0, this.randomRange(0.16, 0.22))
      this.nextEventAt = now + this.randomRange(0.42, 0.92)
      return
    }

    if (this.syllablesRemaining <= 0) {
      this.syllablesRemaining = this.randomInteger(2, 5)
      this.beginTransition(
        now,
        this.randomRange(0.06, 0.12),
        0,
        0,
        0,
        0,
        this.randomRange(0.14, 0.19),
      )
      this.nextEventAt = now + this.randomRange(0.11, 0.24)
      return
    }

    this.scheduleSyllable(now)
  }

  private scheduleSyllable(now: number): void {
    const phraseProgress =
      this.phraseDuration > 0
        ? (now - this.phraseStartedAt) / this.phraseDuration
        : 0
    const interval =
      this.randomRange(FALLBACK_SYLLABLE_MIN, FALLBACK_SYLLABLE_MAX) *
      speechPhraseIntervalScale(phraseProgress)
    const emphasized = this.emphasisCooldown === 0 && this.randomUnit() < 0.14
    this.emphasisCooldown = emphasized
      ? 2
      : Math.max(0, this.emphasisCooldown - 1)
    if (emphasized) this.emphasisStartedAt = now
    let openness = emphasized
      ? this.randomRange(0.62, 0.8)
      : this.randomRange(0.28, 0.61)
    openness *= speechPhraseAmplitudeScale(phraseProgress)
    // Keep enough contrast for articulation while preserving visual continuity.
    openness = clamp(openness, this.toOpen - 0.34, this.toOpen + 0.34)
    openness = clamp(openness, 0.22, 0.8)
    const shape = this.randomUnit()
    const wide = shape < 0.28 ? this.randomRange(0.72, 1) : 0
    const round = shape >= 0.28 && shape < 0.5 ? this.randomRange(0.74, 1) : 0
    const narrow =
      shape >= 0.5 && shape < 0.72 ? this.randomRange(0.68, 0.94) : 0
    this.beginTransition(
      now,
      openness,
      0,
      wide,
      round,
      narrow,
      Math.min(interval * 0.72, this.randomRange(0.08, 0.11)),
    )
    this.syllablesRemaining -= 1
    this.nextEventAt = now + interval
  }

  private beginTransition(
    now: number,
    mouthOpen: number,
    mouthForm: number,
    mouthWide: number,
    mouthRound: number,
    mouthNarrow: number,
    duration: number,
  ): void {
    this.resolve(now)
    this.fromOpen = this.output.mouthOpen
    this.fromForm = this.output.mouthForm
    this.fromWide = this.output.mouthWide
    this.fromRound = this.output.mouthRound
    this.fromNarrow = this.output.mouthNarrow
    this.toOpen = mouthOpen
    this.toForm = mouthForm
    this.toWide = mouthWide
    this.toRound = mouthRound
    this.toNarrow = mouthNarrow
    this.transitionStartedAt = now
    this.transitionDuration = Math.max(0.001, duration)
  }

  private resolve(now: number): Readonly<AutoSpeechPose> {
    const progress = smootherstep(
      (now - this.transitionStartedAt) / this.transitionDuration,
    )
    this.output.mouthOpen = mix(this.fromOpen, this.toOpen, progress)
    this.output.mouthForm = mix(this.fromForm, this.toForm, progress)
    this.output.mouthWide = mix(this.fromWide, this.toWide, progress)
    this.output.mouthRound = mix(this.fromRound, this.toRound, progress)
    this.output.mouthNarrow = mix(this.fromNarrow, this.toNarrow, progress)
    this.output.mouthSeal = 0
    this.output.phraseActivity = this.resolvePhraseActivity(now)
    const emphasisElapsed = now - this.emphasisStartedAt
    this.output.browAccent = attackReleasePulse(
      emphasisElapsed,
      0,
      SPEECH_ACCENT_BROW_ATTACK,
      SPEECH_ACCENT_BROW_RELEASE,
    )
    this.output.headAccent = attackReleasePulse(
      emphasisElapsed,
      SPEECH_ACCENT_HEAD_DELAY,
      SPEECH_ACCENT_HEAD_ATTACK,
      SPEECH_ACCENT_HEAD_RELEASE,
    )
    return this.output
  }

  private sampleText(now: number): Readonly<AutoSpeechPose> {
    if (!Number.isFinite(this.textCueStartedAt)) {
      this.textCueStartedAt = now
      if (
        this.textPhraseStartPending ||
        !Number.isFinite(this.textPhraseStartedAt)
      ) {
        this.textPhraseStartedAt = now
      }
      this.prepareTextCue()
      this.maybeStartTextAccent(now)
    }
    for (let skipped = 0; skipped < 24; skipped += 1) {
      const cue = this.textCues[this.textCueIndex]
      if (
        !cue ||
        !Number.isFinite(this.textCueDuration) ||
        now < this.textCueStartedAt + this.textCueDuration
      ) {
        break
      }
      this.previousViseme = cue.viseme
      this.textCueStartedAt += this.textCueDuration
      this.textCueIndex += 1
      if (cue.viseme === 'rest' && isMajorVisualSpeechPause(cue.duration)) {
        this.textPhraseStartedAt = this.textCueStartedAt
        this.textPhraseStartPending = true
        this.nextTextAccentAt = this.textCueStartedAt
      }
      this.prepareTextCue()
      this.maybeStartTextAccent(this.textCueStartedAt)
    }
    const cue = this.textCues[this.textCueIndex]
    if (!cue) {
      this.textCueStartedAt = Number.NaN
      this.textCueDuration = Number.NaN
      this.textCues = []
      this.textCueIndex = 0
      this.activeTextAccentIndex = -1
      if (this.pendingTextCompilations > 0) {
        return this.sampleProvisionalText(now)
      }
      this.output.mouthOpen = 0
      this.output.mouthWide = 0
      this.output.mouthRound = 0
      this.output.mouthNarrow = 0
      this.output.mouthSeal = 0
      this.output.mouthForm = 0
      this.output.phraseActivity = 0
      this.output.browAccent = 0
      this.output.headAccent = 0
      this.previousViseme = 'rest'
      return this.output
    }
    const next = this.textCues[this.textCueIndex + 1]?.viseme || 'rest'
    const progress = clamp(
      (now - this.textCueStartedAt) / this.textCueDuration,
      0,
      1,
    )
    const onsetFraction = cue.viseme === 'closed' ? 0.12 : 0.24
    const releaseFraction = next === 'round' ? 0.42 : 0.28
    let from = cue.viseme
    let to = cue.viseme
    let blend = 1
    if (progress < onsetFraction) {
      from = this.previousViseme
      blend = smootherstep(progress / onsetFraction)
    } else if (progress > 1 - releaseFraction) {
      to = next
      blend = smootherstep((progress - (1 - releaseFraction)) / releaseFraction)
    }
    this.output.mouthOpen = mix(
      visemeValue(from, 'open'),
      visemeValue(to, 'open'),
      blend,
    )
    this.output.mouthWide = mix(
      visemeValue(from, 'wide'),
      visemeValue(to, 'wide'),
      blend,
    )
    this.output.mouthRound = mix(
      visemeValue(from, 'round'),
      visemeValue(to, 'round'),
      blend,
    )
    this.output.mouthNarrow = mix(
      visemeValue(from, 'narrow'),
      visemeValue(to, 'narrow'),
      blend,
    )
    this.output.mouthSeal = mix(
      visemeValue(from, 'seal'),
      visemeValue(to, 'seal'),
      blend,
    )
    this.output.mouthForm = 0
    const phraseOnset = smootherstep((now - this.textPhraseStartedAt) / 0.14)
    this.output.phraseActivity =
      phraseOnset *
      (cue.viseme === 'rest' ? visualSpeechPauseActivity(cue.duration) : 1)
    const accent =
      this.activeTextAccentIndex === this.textCueIndex
        ? attackReleasePulse(
            now - this.textCueStartedAt,
            0,
            SPEECH_TEXT_ACCENT_ATTACK,
            SPEECH_TEXT_ACCENT_RELEASE,
          )
        : 0
    this.output.browAccent = accent
    this.output.headAccent = accent * smootherstep(progress)
    return this.output
  }

  private prepareTextCue(): void {
    const cue = this.textCues[this.textCueIndex]
    if (!cue) {
      this.textCueDuration = Number.NaN
      return
    }

    const startsPhrase = this.textPhraseStartPending
    if (startsPhrase) {
      this.textPhrasePace = this.randomRange(
        TEXT_PHRASE_PACE_MIN,
        TEXT_PHRASE_PACE_MAX,
      )
      this.textLocalPace = mix(this.textLocalPace, 1, 0.65)
      this.textPhraseStartPending = false
    }

    let pace: number
    if (cue.viseme === 'rest') {
      const pauseSpread = isMajorVisualSpeechPause(cue.duration)
        ? this.randomRange(0.88, 1.22)
        : cue.duration >= 0.18
          ? this.randomRange(0.86, 1.18)
          : this.randomRange(0.78, 1.24)
      pace = Math.sqrt(this.textPhrasePace) * pauseSpread
    } else {
      const target = this.randomRange(TEXT_LOCAL_PACE_MIN, TEXT_LOCAL_PACE_MAX)
      this.textLocalPace += (target - this.textLocalPace) * 0.38
      pace = this.textPhrasePace * this.textLocalPace
      if (startsPhrase) pace *= this.randomRange(1.015, 1.07)
      if (cue.emphasis) pace *= this.randomRange(1.035, 1.095)
      const nextCue = this.textCues[this.textCueIndex + 1]
      if (nextCue?.viseme === 'rest') {
        pace *= isMajorVisualSpeechPause(nextCue.duration)
          ? this.randomRange(1.055, 1.12)
          : this.randomRange(1.02, 1.07)
      }
    }
    this.textCueDuration = Math.max(
      0.025,
      cue.duration * clamp(pace, 0.78, 1.32),
    )
  }

  /** Covers only the causal gap between receiving text and compiling its exact visemes. */
  private sampleProvisionalText(now: number): Readonly<AutoSpeechPose> {
    if (!this.provisionalTextActive) {
      this.provisionalTextActive = true
      this.speaking = true
      this.phraseStartedAt = now
      this.phraseDuration = 3_600
      this.phraseEndsAt = now + this.phraseDuration
      this.emphasisStartedAt = Number.NEGATIVE_INFINITY
      this.scheduleProvisionalSyllable(now)
    }

    this.resolve(now)
    for (let event = 0; event < 12 && now >= this.nextEventAt; event += 1) {
      const scheduledAt = this.nextEventAt
      this.scheduleProvisionalSyllable(scheduledAt)
      this.resolve(now)
    }
    return this.output
  }

  private scheduleProvisionalSyllable(now: number): void {
    const interval = this.randomRange(0.14, 0.2)
    const openness = clamp(
      this.randomRange(0.24, 0.46),
      this.toOpen - 0.26,
      this.toOpen + 0.26,
    )
    const shape = this.randomUnit()
    this.beginTransition(
      now,
      openness,
      0,
      shape < 0.34 ? this.randomRange(0.45, 0.72) : 0,
      shape >= 0.34 && shape < 0.66 ? this.randomRange(0.45, 0.72) : 0,
      shape >= 0.66 ? this.randomRange(0.4, 0.68) : 0,
      Math.min(interval * 0.7, 0.095),
    )
    this.nextEventAt = now + interval
  }

  private maybeStartTextAccent(cueStartedAt: number): void {
    const cue = this.textCues[this.textCueIndex]
    if (!cue?.emphasis || cueStartedAt < this.nextTextAccentAt) return
    this.activeTextAccentIndex = this.textCueIndex
    this.nextTextAccentAt = cueStartedAt + 0.48
  }

  private resolvePhraseActivity(now: number): number {
    if (this.phraseDuration <= 0) return 0
    const sinceStart = now - this.phraseStartedAt
    const untilEnd = this.phraseEndsAt - now
    if (sinceStart < 0 || untilEnd <= 0) return 0
    return smootherstep(sinceStart / 0.16) * smootherstep(untilEnd / 0.22)
  }

  private beginRestRelease(now: number): void {
    this.fromPhrase = this.output.phraseActivity
    this.fromBrow = this.output.browAccent
    this.fromHead = this.output.headAccent
    this.speaking = false
    this.nextEventAt = Number.POSITIVE_INFINITY
    this.phraseStartedAt = now
    this.phraseEndsAt = now
    this.phraseDuration = 0
    this.syllablesRemaining = 0
    this.emphasisCooldown = 0
    this.emphasisStartedAt = Number.NEGATIVE_INFINITY
    this.textMode = false
    this.textCues = []
    this.textCueIndex = 0
    this.textCueStartedAt = Number.NaN
    this.textCueDuration = Number.NaN
    this.textPhraseStartedAt = Number.NaN
    this.textPhrasePace = 1
    this.textLocalPace = 1
    this.textPhraseStartPending = true
    this.previousViseme = 'rest'
    this.activeTextAccentIndex = -1
    this.nextTextAccentAt = 0
    this.pendingTextCompilations = 0
    this.provisionalTextActive = false
    this.textGeneration += 1
    this.textCompilation = Promise.resolve()
    this.restRelease = true
    this.beginTransition(now, 0, 0, 0, 0, 0, REST_RELEASE)
  }

  private resolveRest(now: number): Readonly<AutoSpeechPose> {
    this.resolve(now)
    if (!this.restRelease) return this.output
    const progress = smootherstep(
      (now - this.transitionStartedAt) / this.transitionDuration,
    )
    this.output.phraseActivity = this.fromPhrase * (1 - progress)
    this.output.browAccent = this.fromBrow * (1 - progress)
    this.output.headAccent = this.fromHead * (1 - progress)
    if (progress >= 1) this.restRelease = false
    return this.output
  }

  private randomInteger(minimum: number, maximum: number): number {
    return Math.min(maximum, Math.floor(this.randomRange(minimum, maximum + 1)))
  }

  private randomRange(minimum: number, maximum: number): number {
    return minimum + (maximum - minimum) * this.randomUnit()
  }

  private randomUnit(): number {
    const value = this.random()
    return Number.isFinite(value) ? clamp(value, 0, 1) : 0.5
  }
}

function visemeValue(
  viseme: SpeechViseme,
  channel: 'open' | 'wide' | 'round' | 'narrow' | 'seal',
): number {
  if (channel === 'seal') return viseme === 'closed' ? 1 : 0
  if (channel === 'open') {
    if (viseme === 'open') return 0.78
    if (viseme === 'wide') return 0.54
    if (viseme === 'round') return 0.64
    if (viseme === 'narrow') return 0.34
    return 0
  }
  return viseme === channel ? 1 : 0
}

function closestViseme(pose: Readonly<AutoSpeechPose>): SpeechViseme {
  const candidates: SpeechViseme[] = [
    'rest',
    'closed',
    'open',
    'wide',
    'round',
    'narrow',
  ]
  let closest: SpeechViseme = 'rest'
  let closestDistance = Number.POSITIVE_INFINITY
  for (const candidate of candidates) {
    const distance =
      Math.abs(pose.mouthOpen - visemeValue(candidate, 'open')) +
      Math.abs(pose.mouthWide - visemeValue(candidate, 'wide')) +
      Math.abs(pose.mouthRound - visemeValue(candidate, 'round')) +
      Math.abs(pose.mouthNarrow - visemeValue(candidate, 'narrow')) +
      Math.abs(pose.mouthSeal - visemeValue(candidate, 'seal'))
    if (distance < closestDistance) {
      closest = candidate
      closestDistance = distance
    }
  }
  return closest
}

function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function unitInterval(value: number): number {
  return Number.isFinite(value) ? clamp(value, 0, 1) : 0
}

function attackReleasePulse(
  elapsed: number,
  delay: number,
  attack: number,
  release: number,
): number {
  const shifted = elapsed - delay
  if (!Number.isFinite(shifted) || shifted < 0) return 0
  if (shifted < attack) return smootherstep(shifted / attack)
  if (shifted < attack + release) {
    return 1 - smootherstep((shifted - attack) / release)
  }
  return 0
}
