import type { SpeechInterruptMode, SpeechSegment } from './speechSegmenter'
import {
  markTurnTraceOnce,
  noteTurnTraceDelay,
  noteTurnTraceDrop,
  noteTurnTraceQueue,
} from '../turnTrace'

export interface TtsAudioHandle {
  stop: () => void
}

export interface TtsPipelineHost {
  synthesize: (
    segment: SpeechSegment,
    signal: AbortSignal,
  ) => Promise<ArrayBuffer | null>
  play: (
    audio: ArrayBuffer,
    segment: SpeechSegment,
    onEnded: () => void,
  ) => TtsAudioHandle
  onCancel?: (messageId: string) => void
  fallback?: (segment: SpeechSegment, onEnded: () => void) => TtsAudioHandle
}

const MAX_SYNTH = 2

interface QueuedSegment {
  playId: number
  segment: SpeechSegment
}

interface ReadySlot {
  segment: SpeechSegment
  audio: ArrayBuffer | null
}

interface Synthesis {
  segment: SpeechSegment
  controller: AbortController
}

/** Segment.sequence is per-utterance and must not be the play cursor. */
export class TtsPipeline {
  private nextPlayId = 0
  private nextPlay = 1
  private readonly synthesis = new Map<number, Synthesis>()
  private pending: QueuedSegment[] = []
  private readonly ready = new Map<number, ReadySlot>()
  private handle: TtsAudioHandle | null = null
  private playingMessageId: string | null = null
  private playSeq = 0

  constructor(private readonly host: TtsPipelineHost) {}

  get playing(): boolean {
    return this.handle != null
  }

  isBusyWith(messageId: string): boolean {
    if (this.playingMessageId === messageId) return true
    if (this.pending.some((item) => item.segment.messageId === messageId)) {
      return true
    }
    for (const slot of this.ready.values()) {
      if (slot.segment.messageId === messageId) return true
    }
    for (const task of this.synthesis.values()) {
      if (task.segment.messageId === messageId) return true
    }
    return false
  }

  get queueLength(): number {
    return (
      this.pending.length +
      this.ready.size +
      this.synthesis.size +
      (this.handle ? 1 : 0)
    )
  }

  /** Ordered future segments only */
  upcomingText(messageId: string, generation: number): string {
    return [
      ...this.pending,
      ...Iterator.from(this.ready).map(([playId, slot]) => ({
        playId,
        segment: slot.segment,
      })),
      ...Iterator.from(this.synthesis).map(([playId, task]) => ({
        playId,
        segment: task.segment,
      })),
    ]
      .filter(
        ({ segment }) =>
          segment.messageId === messageId && segment.generation === generation,
      )
      .toSorted((a, b) => a.playId - b.playId)
      .slice(0, 6)
      .map(({ segment }) => segment.text)
      .join('\n')
  }

  enqueue(
    segments: readonly SpeechSegment[],
    mode: SpeechInterruptMode = 'queue',
  ): void {
    if (segments.length === 0) return
    const messageId = segments[0]!.messageId
    if (mode === 'interrupt') this.cancel()
    else if (mode === 'replace') this.replaceMessage(messageId)
    for (const segment of segments) {
      this.nextPlayId += 1
      this.pending.push({ playId: this.nextPlayId, segment })
    }
    markTurnTraceOnce('tts_queued', { n: segments.length })
    this.noteQueue()
    this.pumpSynth()
  }

  cancel(messageId?: string): boolean {
    if (messageId) return this.cancelMessage(messageId)
    return this.cancelAll()
  }

  private cancelMessage(messageId: string): boolean {
    const stopped = this.playingMessageId === messageId
    const dropped = this.dropMessage(messageId)
    if (stopped) {
      this.stopPlayback()
      this.playingMessageId = null
      this.host.onCancel?.(messageId)
    }
    if (dropped || stopped) noteTurnTraceDrop('cancelled')
    this.noteQueue()
    this.tryPlay()
    this.pumpSynth()
    return stopped
  }

  private cancelAll(): boolean {
    const stopped = this.handle != null
    const hadWork =
      stopped ||
      this.pending.length > 0 ||
      this.ready.size > 0 ||
      this.synthesis.size > 0
    this.stopPlayback()
    this.pending.length = 0
    this.ready.clear()
    const abandoned = Iterator.from(this.synthesis.values()).toArray()
    this.synthesis.clear()
    this.nextPlayId = 0
    this.nextPlay = 1
    for (const task of abandoned) task.controller.abort()
    const id = this.playingMessageId
    this.playingMessageId = null
    this.noteQueue()
    if (hadWork) noteTurnTraceDrop('cancelled')
    if (id) this.host.onCancel?.(id)
    return stopped
  }

  private replaceMessage(messageId: string): void {
    const dropped = this.dropMessage(messageId)
    const stopped = this.playingMessageId === messageId
    if (stopped) {
      this.stopPlayback()
      this.playingMessageId = null
    }
    if (dropped || stopped) noteTurnTraceDrop('queue_replaced')
    this.noteQueue()
    this.tryPlay()
  }

  private dropMessage(messageId: string): boolean {
    let dropped = false
    for (let i = this.pending.length - 1; i >= 0; i--) {
      if (this.pending[i]?.segment.messageId === messageId) {
        this.pending = this.pending.toSpliced(i, 1)
        dropped = true
      }
    }
    for (const [playId, slot] of Iterator.from(this.ready).toArray()) {
      if (slot.segment.messageId === messageId) {
        this.ready.delete(playId)
        dropped = true
      }
    }
    for (const [playId, task] of this.synthesis) {
      if (task.segment.messageId === messageId) {
        this.synthesis.delete(playId)
        task.controller.abort()
        dropped = true
      }
    }
    return dropped
  }

  private pumpSynth(): void {
    while (this.synthesis.size < MAX_SYNTH && this.pending.length > 0) {
      const item = this.pending.shift()!
      const task: Synthesis = {
        segment: item.segment,
        controller: new AbortController(),
      }
      const started = nowMs()
      this.synthesis.set(item.playId, task)
      this.noteQueue()
      let result: Promise<ArrayBuffer | null>
      try {
        result = this.host.synthesize(item.segment, task.controller.signal)
      } catch {
        result = Promise.resolve(null)
      }
      void result
        .catch(() => null)
        .then((audio) => {
          if (this.synthesis.get(item.playId) !== task) return
          this.synthesis.delete(item.playId)
          if (audio) {
            noteTurnTraceDelay('tts', nowMs() - started)
            markTurnTraceOnce('tts_ready')
          } else {
            noteTurnTraceDrop('synth_failed')
          }
          this.ready.set(item.playId, { segment: item.segment, audio })
          this.tryPlay()
          this.pumpSynth()
        })
    }
  }

  private tryPlay(): void {
    if (this.handle) return
    this.skipMissingPlays()
    const slot = this.ready.get(this.nextPlay)
    if (!slot) return
    this.ready.delete(this.nextPlay)
    this.nextPlay += 1
    if (!slot.audio && !this.host.fallback) {
      this.tryPlay()
      return
    }
    this.playingMessageId = slot.segment.messageId
    const playSeq = ++this.playSeq
    let ended = false
    markTurnTraceOnce('playback_started')
    this.handle = { stop: () => undefined }
    const onEnded = () => {
      if (playSeq !== this.playSeq) return
      ended = true
      this.handle = null
      this.playingMessageId = null
      this.noteQueue()
      this.tryPlay()
    }
    const handle = slot.audio
      ? this.host.play(slot.audio, slot.segment, onEnded)
      : this.host.fallback!(slot.segment, onEnded)
    if (!ended && playSeq === this.playSeq) this.handle = handle
  }

  private stopPlayback(): void {
    this.playSeq += 1
    this.handle?.stop()
    this.handle = null
    this.noteQueue()
  }

  private skipMissingPlays(): void {
    while (
      this.nextPlay <= this.nextPlayId &&
      !this.ready.has(this.nextPlay) &&
      !this.synthesis.has(this.nextPlay) &&
      !this.pending.some((item) => item.playId === this.nextPlay)
    ) {
      this.nextPlay += 1
    }
  }

  private noteQueue(): void {
    noteTurnTraceQueue(this.queueLength)
  }
}

function nowMs(): number {
  return typeof performance === 'undefined' ? Date.now() : performance.now()
}
