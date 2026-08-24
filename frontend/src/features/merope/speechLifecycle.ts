import type { SpeechArticulation } from './rig/articulation'
import type { MeropeSpeechEventDetail } from './speechEvents'

export interface SpeechLifecycleTarget {
  setSpeechActive: (active: boolean) => void
  setAutoSpeech: (active: boolean) => void
  setSpeechEnergy: (energy: number | null) => void
  setSpeechArticulation: (articulation: SpeechArticulation) => void
}

export interface SpeechLifecycleScheduler {
  now: () => number
  setTimeout: (callback: () => void, delayMs: number) => unknown
  clearTimeout: (timer: unknown) => void
}

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
 * otherwise the existing lightweight auto-prosody controller is used.
 */
export class SpeechLifecycleController {
  private activeMessageId: string | null = null
  private activeUtteranceId: string | null = null
  private startedAt = 0
  private bufferedText = ''
  private authored = false
  private autoActive = false
  private speechActive = false
  private timer: unknown = null

  constructor(
    private readonly target: SpeechLifecycleTarget,
    private readonly scheduler: SpeechLifecycleScheduler = defaultScheduler,
  ) {}

  handle(event: MeropeSpeechEventDetail): void {
    if (event.phase === 'cancel') {
      if (
        this.activeMessageId === event.messageId &&
        (!event.utteranceId || event.utteranceId === this.activeUtteranceId)
      ) {
        this.finishNow()
      }
      return
    }

    if (event.phase === 'start') {
      this.start(event.messageId, event.utteranceId)
      return
    }

    if (!this.matches(event.messageId, event.utteranceId)) {
      if (event.phase === 'end') return
      this.start(event.messageId, event.utteranceId)
    }

    if (event.phase === 'chunk') {
      this.bufferedText = `${this.bufferedText}${event.text}`.slice(
        0,
        MAX_BUFFERED_TEXT,
      )
      this.scheduleWatchdog()
      return
    }

    if (event.phase === 'energy') {
      this.claimAuthoredMouth()
      this.target.setSpeechEnergy(event.energy)
      this.scheduleWatchdog()
      return
    }

    if (event.phase === 'articulation') {
      this.claimAuthoredMouth()
      this.target.setSpeechArticulation(event.articulation)
      this.scheduleWatchdog()
      return
    }

    if (this.authored) {
      this.finishNow()
      return
    }
    const elapsed = Math.max(0, this.scheduler.now() - this.startedAt)
    const remaining = estimateAutoSpeechDurationMs(this.bufferedText) - elapsed
    this.scheduleFinish(Math.max(MIN_END_TAIL_MS, remaining))
  }

  dispose(): void {
    this.finishNow()
  }

  private start(messageId: string, utteranceId: string): void {
    this.finishNow()
    this.activeMessageId = messageId
    this.activeUtteranceId = utteranceId
    this.startedAt = this.scheduler.now()
    this.bufferedText = ''
    this.authored = false
    this.autoActive = true
    this.speechActive = true
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
    this.activeMessageId = null
    this.activeUtteranceId = null
    this.startedAt = 0
    this.bufferedText = ''
    this.authored = false
    this.autoActive = false
    this.speechActive = false
  }

  private clearTimer(): void {
    if (this.timer === null) return
    this.scheduler.clearTimeout(this.timer)
    this.timer = null
  }
}

export function estimateAutoSpeechDurationMs(text: string): number {
  const bounded = text.slice(0, MAX_BUFFERED_TEXT)
  const cjk = Array.from(bounded).filter((unit) =>
    /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u.test(
      unit,
    ),
  ).length
  const latinWords =
    bounded.match(/[\p{Script=Latin}\p{Number}]+/gu)?.length ?? 0
  const punctuation = bounded.match(/[.,!?;:，。！？；：、…—\-]/gu)?.length ?? 0
  const estimated = 420 + cjk * 155 + latinWords * 260 + punctuation * 70
  return Math.round(clamp(estimated, 600, MAX_UTTERANCE_MS))
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
