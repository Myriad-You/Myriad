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
  synthesize: (segment: SpeechSegment) => Promise<ArrayBuffer | null>
  play: (
    audio: ArrayBuffer,
    segment: SpeechSegment,
    onEnded: () => void,
  ) => TtsAudioHandle
  onCancel?: (messageId: string) => void
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

/**
 * Synthesize up to two segments at once; play in enqueue order.
 * Segment.sequence is per-utterance and must not be the play cursor.
 */
export class TtsPipeline {
  private epoch = 0
  private nextPlayId = 0
  private nextPlay = 1
  private inflight = 0
  private readonly inflightIds = new Set<number>()
  private readonly inflightByPlay = new Map<number, string>()
  private readonly pending: QueuedSegment[] = []
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
    for (const id of this.inflightByPlay.values()) {
      if (id === messageId) return true
    }
    return false
  }

  get queueLength(): number {
    return (
      this.pending.length +
      this.ready.size +
      this.inflight +
      (this.handle ? 1 : 0)
    )
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
      this.inflight > 0
    this.epoch += 1
    this.stopPlayback()
    this.pending.length = 0
    this.ready.clear()
    this.inflightIds.clear()
    this.inflightByPlay.clear()
    this.nextPlayId = 0
    this.nextPlay = 1
    this.inflight = 0
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
        this.pending.splice(i, 1)
        dropped = true
      }
    }
    for (const [playId, slot] of [...this.ready]) {
      if (slot.segment.messageId === messageId) {
        this.ready.delete(playId)
        dropped = true
      }
    }
    for (const [playId, id] of [...this.inflightByPlay]) {
      if (id === messageId) {
        this.inflightByPlay.delete(playId)
        this.inflightIds.delete(playId)
        dropped = true
      }
    }
    return dropped
  }

  private pumpSynth(): void {
    while (this.inflight < MAX_SYNTH && this.pending.length > 0) {
      const item = this.pending.shift()!
      const epoch = this.epoch
      const started = nowMs()
      this.inflight += 1
      this.inflightIds.add(item.playId)
      this.inflightByPlay.set(item.playId, item.segment.messageId)
      this.noteQueue()
      void this.host
        .synthesize(item.segment)
        .catch(() => null)
        .then((audio) => {
          this.inflight = Math.max(0, this.inflight - 1)
          this.inflightIds.delete(item.playId)
          const wanted = this.inflightByPlay.delete(item.playId)
          if (epoch !== this.epoch) {
            this.pumpSynth()
            return
          }
          if (!wanted) {
            this.pumpSynth()
            return
          }
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
    if (!slot.audio) {
      this.tryPlay()
      return
    }
    this.playingMessageId = slot.segment.messageId
    const playSeq = this.playSeq
    markTurnTraceOnce('playback_started')
    this.handle = this.host.play(slot.audio, slot.segment, () => {
      if (playSeq !== this.playSeq) return
      this.handle = null
      this.noteQueue()
      this.tryPlay()
    })
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
      !this.inflightIds.has(this.nextPlay) &&
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
