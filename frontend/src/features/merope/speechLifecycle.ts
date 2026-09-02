import type { SpeechArticulation } from './rig/articulation'
import type { SpeechProsodyPlan } from './speech/prosody'
import type { MeropeSpeechEventDetail } from './speechEvents'
import { isLiveMotionGeneration } from './motion/liveGeneration'
import { estimateVisualSpeechDurationMs } from './speech/textTiming'
import { noteTurnTraceDrop } from './turnTrace'

export interface SpeechLifecycleTarget {
  setSpeechActive: (active: boolean) => void
  setAutoSpeech: (active: boolean) => void
  setSpeechEnergy: (energy: number | null) => void
  setSpeechArticulation: (articulation: SpeechArticulation) => void
  setSpeechProsody?: (prosody: SpeechProsodyPlan | null) => void
  enqueueSpeechText: (text: string, locale?: string) => void
}

export interface SpeechLifecycleScheduler {
  now: () => number
  setTimeout: (callback: () => void, delayMs: number) => unknown
  clearTimeout: (timer: unknown) => void
}

/** Mutable occupancy flag; the mouth lease follows this through onBusyChange. */
export interface SpeechOccupancy {
  current: boolean
}

/** Whether the motion layer may derive co-speech behavior from this event. */
export type SpeechLifecycleDisposition = 'active' | 'finished' | 'ignored'

const MIN_END_TAIL_MS = 180
const MAX_UTTERANCE_MS = 12_000
const MAX_BUFFERED_TEXT = 2_000

const defaultScheduler: SpeechLifecycleScheduler = {
  now: () => performance.now(),
  setTimeout: (callback, delayMs) => globalThis.setTimeout(callback, delayMs),
  clearTimeout: (timer) => globalThis.clearTimeout(timer as number),
}

/**
 * Bridges reply lifecycle events to one rig without adding work to its frame loop.
 * Audio energy or phoneme articulation owns the mouth as soon as it arrives;
 * otherwise streamed text drives the bounded visual-viseme controller.
 */
export class SpeechLifecycleController {
  private activeMessageId: string | null = null
  private activeUtteranceId: string | null = null
  private startedAt = 0
  private bufferedText = ''
  private activeLocale: string | undefined
  private authored = false
  private autoActive = false
  private speechActive = false
  private notifiedBusy = false
  private timer: unknown = null
  private readonly scheduler: SpeechLifecycleScheduler
  private readonly occupancy?: SpeechOccupancy

  constructor(
    private readonly target: SpeechLifecycleTarget,
    scheduler: SpeechLifecycleScheduler = defaultScheduler,
    occupancy?: SpeechOccupancy,
    private readonly onBusyChange?: (busy: boolean) => void,
  ) {
    this.scheduler = scheduler ?? defaultScheduler
    this.occupancy = occupancy
  }

  handle(event: MeropeSpeechEventDetail): SpeechLifecycleDisposition {
    if (event.phase === 'cancel') {
      if (
        this.activeMessageId === event.messageId &&
        (!event.utteranceId || event.utteranceId === this.activeUtteranceId)
      ) {
        this.finishNow()
        return 'finished'
      }
      return 'ignored'
    }

    if (!isLiveMotionGeneration(event.generation)) {
      if (event.phase === 'start') noteTurnTraceDrop('stale_generation')
      return 'ignored'
    }

    if (event.phase === 'start') {
      this.start(event.messageId, event.utteranceId, event.locale)
      return 'active'
    }

    if (!this.matches(event.messageId, event.utteranceId)) {
      if (event.phase === 'end') return 'ignored'
      // A sampled frame may open an idle mouth but must not evict a live
      // utterance. The live-conversation analyser and a streamed reply are
      // different producers; being loud does not make one the owner.
      if (this.speechActive && event.phase !== 'chunk') {
        noteTurnTraceDrop('foreign_speech_frame')
        return 'ignored'
      }
      this.start(event.messageId, event.utteranceId, event.locale)
    }

    if (event.phase === 'chunk') {
      if (event.locale) this.activeLocale = event.locale
      this.target.enqueueSpeechText(event.text, event.locale)
      this.bufferedText = `${this.bufferedText}${event.text}`.slice(
        0,
        MAX_BUFFERED_TEXT,
      )
      this.scheduleWatchdog()
      return 'active'
    }

    if (event.phase === 'energy') {
      this.claimAuthoredMouth()
      this.target.setSpeechEnergy(event.energy)
      this.scheduleWatchdog()
      return 'active'
    }

    if (event.phase === 'articulation') {
      this.claimAuthoredMouth()
      this.target.setSpeechArticulation(event.articulation)
      this.scheduleWatchdog()
      return 'active'
    }

    if (event.phase === 'prosody') {
      this.target.setSpeechProsody?.(event.prosody)
      this.scheduleWatchdog()
      return 'active'
    }

    if (this.authored) {
      this.finishNow()
      return 'finished'
    }
    const elapsed = Math.max(0, this.scheduler.now() - this.startedAt)
    const remaining =
      estimateAutoSpeechDurationMs(this.bufferedText, this.activeLocale) -
      elapsed
    this.scheduleFinish(Math.max(MIN_END_TAIL_MS, remaining))
    return 'active'
  }

  dispose(): void {
    this.finishNow()
  }

  private start(messageId: string, utteranceId: string, locale?: string): void {
    this.finishNow()
    this.activeMessageId = messageId
    this.activeUtteranceId = utteranceId
    this.startedAt = this.scheduler.now()
    this.bufferedText = ''
    this.activeLocale = locale
    this.authored = false
    this.autoActive = true
    this.speechActive = true
    this.setOccupancy(true)
    this.target.setSpeechActive(true)
    this.target.setAutoSpeech(true)
    this.scheduleWatchdog()
  }

  private claimAuthoredMouth(): void {
    if (this.authored) return
    this.authored = true
    if (this.autoActive) {
      this.autoActive = false
      this.target.setAutoSpeech(false)
    }
  }

  private matches(messageId: string, utteranceId: string): boolean {
    return (
      this.activeMessageId === messageId &&
      this.activeUtteranceId === utteranceId
    )
  }

  private scheduleWatchdog(): void {
    this.scheduleFinish(MAX_UTTERANCE_MS)
  }

  private scheduleFinish(delayMs: number): void {
    this.clearTimer()
    const boundedDelay = clamp(delayMs, MIN_END_TAIL_MS, MAX_UTTERANCE_MS)
    this.timer = this.scheduler.setTimeout(() => this.finishNow(), boundedDelay)
  }

  private finishNow(): void {
    const hadUtterance = this.activeMessageId !== null || this.speechActive
    this.clearTimer()
    if (this.authored) {
      this.target.setSpeechArticulation({
        energy: 0,
        viseme: 'rest',
        amount: 0,
      })
    } else if (this.autoActive) {
      this.target.setAutoSpeech(false)
    }
    if (this.speechActive) this.target.setSpeechActive(false)
    if (hadUtterance) this.target.setSpeechProsody?.(null)
    this.setOccupancy(false)
    this.activeMessageId = null
    this.activeUtteranceId = null
    this.startedAt = 0
    this.bufferedText = ''
    this.activeLocale = undefined
    this.authored = false
    this.autoActive = false
    this.speechActive = false
  }

  private clearTimer(): void {
    if (this.timer === null) return
    this.scheduler.clearTimeout(this.timer)
    this.timer = null
  }

  private setOccupancy(busy: boolean): void {
    if (this.occupancy) this.occupancy.current = busy
    if (this.notifiedBusy === busy) return
    this.notifiedBusy = busy
    this.onBusyChange?.(busy)
  }
}

export function estimateAutoSpeechDurationMs(
  text: string,
  locale?: string,
): number {
  return estimateVisualSpeechDurationMs(
    text.slice(0, MAX_BUFFERED_TEXT),
    locale,
  )
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
