import type { SpeechArticulation } from '../rig/articulation'
import type { BehaviorPlan } from './behavior'
import type { MotionLeaseHandle, RigMotionCoordinator } from './coordinator'
import type { SpeechIntent, SpeechTextChunk } from './intents'
import { predictTextProsody } from '../speech/textProsody'
import { MEROPE_SPEECH_EVENT, meropeSpeechEventDetail } from '../speechEvents'
import { SpeechLifecycleController } from '../speechLifecycle'
import { compileSpeechBehaviorPlan } from './speechBehaviorPlan'
import { SpeechMotionLease } from './speechLease'

const REST: SpeechArticulation = { energy: 0, viseme: 'rest', amount: 0 }
const MAX_QUEUED_TEXT = 32

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
  ) {
    this.mouth = new SpeechMotionLease(coordinator)
  }

  current(_nowMs: number = currentNow()): SpeechIntent {
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
        setSpeechProsody: (prosody) => {
          if (prosody) {
            this.externalProsody = true
            this.speechBehaviorPlan = compileSpeechBehaviorPlan(prosody)
            this.intent = {
              ...this.intent,
              prosody,
              behaviorPlan: this.speechBehaviorPlan,
            }
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
      undefined,
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
    const disposition = this.controller?.handle(detail) ?? 'ignored'
    if (disposition !== 'active') return
    this.prepareBehaviorPlan(detail)
    this.flush()
  }

  private prepareBehaviorPlan(
    detail: Parameters<SpeechLifecycleController['handle']>[0],
  ): void {
    if (detail.phase === 'cancel') return
    if (
      detail.phase === 'start' ||
      this.activeUtteranceId !== detail.utteranceId
    ) {
      this.activeUtteranceId = detail.utteranceId
      this.speechStartedAtMs = currentNow()
      this.behaviorText = ''
      this.behaviorLocale = detail.locale
      this.externalProsody = false
    }
    if (detail.locale) this.behaviorLocale = detail.locale
    if (detail.phase === 'prosody') {
      this.externalProsody = true
      return
    }
    if (detail.phase === 'chunk') {
      this.behaviorText = `${this.behaviorText}${detail.text}`.slice(0, 2_000)
    }
    if (this.externalProsody) return
    this.planFromPredictedText(detail.utteranceId)
  }

  /**
   * Predicted prosody is the floor, not a bonus: an utterance without a plan
   * has no co-speech behavior at all, so every path that loses real prosody
   * falls back here rather than leaving the plan null.
   */
  private planFromPredictedText(utteranceId: string): void {
    const predictedProsody = predictTextProsody({
      utteranceId,
      text: this.behaviorText,
      ...(this.behaviorLocale ? { locale: this.behaviorLocale } : {}),
      startedAtMs: this.speechStartedAtMs,
    })
    this.speechBehaviorPlan = compileSpeechBehaviorPlan(predictedProsody)
    this.intent = {
      ...this.intent,
      prosody: predictedProsody,
      behaviorPlan: this.speechBehaviorPlan,
    }
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
}

function currentNow(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now()
}
