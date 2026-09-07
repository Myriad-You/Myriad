import type {
  PerformanceDirective,
  SpeechPhrase,
} from '../../../services/agent/types'
import type { MeropePerformanceEventDetail } from '../performanceEvents'
import type { SpeechArticulation } from '../rig/articulation'
import type { PhraseCoverage } from '../speech/phrasePlan'
import type { SpeechProsodyPlan } from '../speech/prosody'
import type { SpeechLifecycleScheduler } from '../speechLifecycle'
import type { BehaviorPlan, BehaviorSnapshot } from './behavior'
import type { MotionLeaseHandle, RigMotionCoordinator } from './coordinator'
import type { SpeechIntent, SpeechTextChunk } from './intents'
import {
  directorPhraseCoverage,
  mergeSpeechPhrases,
  refineSpeechPhrases,
} from '../speech/phrasePlan'
import { continueTextProsody, predictTextProsody } from '../speech/textProsody'
import { MAX_VISUAL_SPEECH_TEXT_UNITS } from '../speech/textTiming'
import { MEROPE_SPEECH_EVENT, meropeSpeechEventDetail } from '../speechEvents'
import { SpeechLifecycleController } from '../speechLifecycle'
import { compileSpeechBehaviorPlan } from './speechBehaviorPlan'
import { SpeechMotionLease } from './speechLease'

const REST: SpeechArticulation = { energy: 0, viseme: 'rest', amount: 0 }
const MAX_QUEUED_TEXT = 32
const SPEECH_LOOKAHEAD_MS = 4_000
// Longest co-speech recovery: laugh hold (580) + relax (84) + release (360).
const SPEECH_RECOVERY_MS = 1_100

/**
 * One speech producer for a coordinator. Publishes semantic mouth intent
 * and a co-speech lease; never writes a rig.
 */
export class SpeechMotionSource {
  private readonly mouth: SpeechMotionLease
  private speechBehaviorPlan: BehaviorPlan | null = null
  private coSpeech: MotionLeaseHandle | null = null
  private controller: SpeechLifecycleController | null = null
  private textSeq = 0
  private queuedText: SpeechTextChunk[] = []
  private activeUtteranceId: string | null = null
  private speechStartedAtMs = 0
  private behaviorText = ''
  private behaviorLocale: string | undefined
  private externalProsody = false
  private textComplete = false
  private messageKey: string | null = null
  private rawProsody: SpeechProsodyPlan | null = null
  private prosodyText = ''
  private scheduledWindow = ''
  private readonly direction = new Map<
    string,
    { phrases: SpeechPhrase[]; coverage: PhraseCoverage[] }
  >()

  private intent: SpeechIntent = {
    active: false,
    autoSpeech: false,
    energy: null,
    articulation: null,
    prosody: null,
    behaviorPlan: null,
    behaviors: [],
    queuedText: [],
  }

  private listening = false

  constructor(
    private readonly coordinator: RigMotionCoordinator,
    private readonly onChange: (intent: SpeechIntent) => void,
    private readonly activeBehaviors: () => readonly BehaviorSnapshot[] = () => [],
    private readonly scheduler?: SpeechLifecycleScheduler,
  ) {
    this.mouth = new SpeechMotionLease(coordinator)
  }

  current(nowMs: number = this.now()): SpeechIntent {
    this.scheduleProsodyWindow(nowMs)
    return this.intent
  }

  start(): void {
    if (this.listening) return
    this.controller = new SpeechLifecycleController(
      {
        setSpeechActive: (active) => {
          if (!active) this.clearUtteranceBehavior()
          this.intent = { ...this.intent, active }
          this.flush()
        },
        setAutoSpeech: (active) => {
          this.intent = { ...this.intent, autoSpeech: active }
          this.flush()
        },
        setSpeechEnergy: (energy) => {
          this.intent = { ...this.intent, energy, articulation: null }
          this.flush()
        },
        setSpeechArticulation: (articulation) => {
          this.intent = { ...this.intent, articulation, energy: null }
          this.flush()
        },
        setSpeechProsody: (prosody, text) => {
          if (prosody) {
            this.externalProsody = true
            this.prosodyText = text ?? ''
            // handle() binds the accepted event's message before publishing.
            // A segment may open with prosody; never annotate it with the
            // previous segment's direction, even for a single emission.
            this.rawProsody = prosody
            return
          } else {
            this.externalProsody = false
            if (this.activeUtteranceId) {
              this.planFromPredictedText(this.activeUtteranceId)
            } else {
              this.speechBehaviorPlan = null
              this.intent = {
                ...this.intent,
                prosody,
                behaviorPlan: null,
              }
            }
          }
          this.flush()
        },
        enqueueSpeechText: (text, locale) => {
          this.textSeq += 1
          this.queuedText = [
            ...this.queuedText,
            { seq: this.textSeq, text, ...(locale ? { locale } : {}) },
          ].slice(-MAX_QUEUED_TEXT)
          this.intent = { ...this.intent, queuedText: this.queuedText }
          this.flush()
        },
      },
      this.scheduler,
      undefined,
      (busy) => {
        this.mouth.setBusy(busy)
        if (busy) {
          this.claimCoSpeech()
        } else {
          this.clearUtteranceBehavior()
          this.releaseCoSpeech()
        }
        this.flush()
      },
    )
    if (typeof window !== 'undefined') {
      window.addEventListener(MEROPE_SPEECH_EVENT, this.onSpeech)
    }
    this.listening = true
  }

  stop(): void {
    if (!this.listening) return
    if (typeof window !== 'undefined') {
      window.removeEventListener(MEROPE_SPEECH_EVENT, this.onSpeech)
    }
    this.controller?.dispose()
    this.controller = null
    this.mouth.release()
    this.releaseCoSpeech()
    this.listening = false
    this.direction.clear()
    this.queuedText = []
    this.textSeq = 0
    this.speechBehaviorPlan = null
    this.activeUtteranceId = null
    this.speechStartedAtMs = 0
    this.behaviorText = ''
    this.behaviorLocale = undefined
    this.externalProsody = false
    this.intent = {
      active: false,
      autoSpeech: false,
      energy: null,
      articulation: REST,
      prosody: null,
      behaviorPlan: null,
      behaviors: [],
      queuedText: [],
    }
    this.flush()
  }

  handleForTest(
    detail: Parameters<SpeechLifecycleController['handle']>[0],
  ): void {
    this.handle(detail)
  }

  private readonly onSpeech = (event: Event): void => {
    const detail = meropeSpeechEventDetail(
      (event as CustomEvent<unknown>).detail,
    )
    if (detail) this.handle(detail)
  }

  private handle(
    detail: Parameters<SpeechLifecycleController['handle']>[0],
  ): void {
    if (detail.phase === 'cancel')
      this.direction.delete(speechMessageKey(detail))
    const disposition = this.controller?.handle(detail) ?? 'ignored'
    if (disposition !== 'active') return
    this.prepareBehaviorPlan(detail)
    this.flush()
  }

  private prepareBehaviorPlan(
    detail: Parameters<SpeechLifecycleController['handle']>[0],
  ): void {
    if (detail.phase === 'cancel') return
    this.messageKey = speechMessageKey(detail)
    if (
      detail.phase === 'start' ||
      this.activeUtteranceId !== detail.utteranceId
    ) {
      this.activeUtteranceId = detail.utteranceId
      this.speechStartedAtMs = this.now()
      this.behaviorText = ''
      this.textComplete = false
      this.behaviorLocale = detail.locale
      this.externalProsody = false
    }
    if (detail.locale) this.behaviorLocale = detail.locale
    if (detail.phase === 'prosody') {
      this.externalProsody = true
      if (this.rawProsody)
        this.publishProsody(this.rawProsody, this.prosodyText)
      return
    }
    if (detail.phase === 'chunk') {
      this.behaviorText = `${this.behaviorText}${detail.text}`.slice(
        0,
        MAX_VISUAL_SPEECH_TEXT_UNITS,
      )
    }
    if (detail.phase === 'end') this.textComplete = true
    if (this.externalProsody) return
    this.planFromPredictedText(detail.utteranceId)
  }

  /**
   * Predicted prosody is the floor, not a bonus: an utterance without a plan
   * has no co-speech behavior at all, so every path that loses real prosody
   * falls back here rather than leaving the plan null.
   */
  private planFromPredictedText(utteranceId: string): void {
    const predictedProsody = continueTextProsody(
      predictTextProsody({
        utteranceId,
        text: this.behaviorText,
        ...(this.behaviorLocale ? { locale: this.behaviorLocale } : {}),
        startedAtMs: this.speechStartedAtMs,
        streaming: !this.textComplete,
      }),
      this.rawProsody,
      this.now(),
    )
    this.publishProsody(predictedProsody, this.behaviorText)
  }

  applyDirector(
    directive: PerformanceDirective,
    event: MeropePerformanceEventDetail | undefined,
    plan: BehaviorPlan | null,
  ): void {
    if (!event?.messageId) return
    const key = speechMessageKey({ ...event, messageId: event.messageId })
    const old = this.direction.get(key)
    const coverage = [
      ...(old?.coverage ?? []),
      ...directorPhraseCoverage(plan, this.activeBehaviors()),
    ].slice(-12)
    this.direction.set(key, {
      phrases: mergeSpeechPhrases(old?.phrases ?? [], directive.phrases),
      coverage,
    })
    while (this.direction.size > 8)
      this.direction.delete(this.direction.keys().next().value!)
    if (key === this.messageKey && this.rawProsody) {
      this.publishProsody(
        this.rawProsody,
        this.externalProsody ? this.prosodyText : this.behaviorText,
      )
      this.flush()
    }
  }

  private publishProsody(base: SpeechProsodyPlan, text: string): void {
    this.rawProsody = base
    const direction = this.messageKey
      ? this.direction.get(this.messageKey)
      : undefined
    const prosody = refineSpeechPhrases(
      base,
      text,
      direction?.phrases ?? [],
      direction?.coverage ?? [],
      this.intent.prosody,
      this.activeBehaviors(),
      this.now(),
    )
    this.intent = {
      ...this.intent,
      prosody,
    }
    this.scheduledWindow = ''
    this.scheduleProsodyWindow(this.now())
  }

  private scheduleProsodyWindow(nowMs: number): void {
    const prosody = this.intent.prosody
    if (!prosody) return
    const elapsed = nowMs - prosody.startedAtMs
    const first = prosody.accents.findIndex(
      (accent) => accent.offsetMs + SPEECH_RECOVERY_MS >= elapsed,
    )
    const start = first < 0 ? prosody.accents.length : first
    let end = start
    while (
      end < prosody.accents.length &&
      prosody.accents[end]!.offsetMs <= elapsed + SPEECH_LOOKAHEAD_MS
    ) {
      end += 1
    }
    const key = `${start}:${end}`
    if (key === this.scheduledWindow) return
    this.scheduledWindow = key
    // Original accent indices preserve audio IDs as this window advances.
    this.speechBehaviorPlan = compileSpeechBehaviorPlan(prosody, { start, end })
    this.intent = { ...this.intent, behaviorPlan: this.speechBehaviorPlan }
  }

  private claimCoSpeech(): void {
    this.coSpeech =
      this.coordinator.renew(this.coSpeech, ['expression', 'headBody']) ??
      this.coordinator.claim('coSpeech', ['expression', 'headBody'])
  }

  private releaseCoSpeech(): void {
    this.coordinator.release(this.coSpeech)
    this.coSpeech = null
  }

  private clearUtteranceBehavior(): void {
    this.messageKey = null
    this.rawProsody = null
    this.prosodyText = ''
    this.speechBehaviorPlan = null
    this.activeUtteranceId = null
    this.speechStartedAtMs = 0
    this.behaviorText = ''
    this.behaviorLocale = undefined
    this.externalProsody = false
    this.queuedText = []
    this.intent = {
      ...this.intent,
      prosody: null,
      behaviorPlan: null,
      queuedText: [],
    }
  }

  private flush(): void {
    this.onChange(this.intent)
  }

  private now(): number {
    return this.scheduler?.now() ?? currentNow()
  }
}

function speechMessageKey(event: {
  source: string
  messageId: string
  generation?: number
}): string {
  return JSON.stringify([event.source, event.generation ?? 0, event.messageId])
}

function currentNow(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now()
}
