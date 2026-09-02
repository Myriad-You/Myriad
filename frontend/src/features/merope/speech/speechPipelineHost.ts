import type { SpeechStatus } from '../../../services/speechApi'
import type { SpeechInterruptMode, SpeechSegment } from './speechSegmenter'
import { getSpeechStatus, textToSpeech } from '../../../services/speechApi'
import { liveMotionGeneration } from '../motion/liveGeneration'
import { dispatchMeropeSpeech } from '../speechEvents'
import { markTurnTraceOnce, noteTurnTraceCancelToSilence } from '../turnTrace'
import { speakableText } from './speakableText'
import { SpeechSegmenter } from './speechSegmenter'
import { TtsPipeline } from './ttsPipeline'
import { playTtsBuffer } from './ttsPlayer'
import { patchVoicePresence } from './voicePresence'

/** Map /api/speech/status onto the persona pipeline. Missing flag = off. */
export function personaSpeechFlags(status: {
  available: boolean
  tts_enabled: boolean
  persona_speech_enabled?: boolean
}): { speechEnabled: boolean; ttsReady: boolean } {
  return {
    speechEnabled: Boolean(status.persona_speech_enabled),
    ttsReady: Boolean(status.available && status.tts_enabled),
  }
}

function audioFromBase64(base64: string): ArrayBuffer {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes.buffer
}

/**
 * One live-face TTS outlet. Synthesis may run ahead; playback stays ordered.
 */
export class SpeechPipelineHost {
  readonly pipeline: TtsPipeline
  private enabled = false
  private wantsSpeech = false
  private ttsReady = false
  private statusEpoch = 0
  private cancelledAt: number | null = null
  private readonly fedMessageIds = new Set<string>()

  constructor() {
    this.pipeline = new TtsPipeline({
      synthesize: (segment) => this.synthesize(segment),
      play: (audio, segment, onEnded) => this.play(audio, segment, onEnded),
      onCancel: (messageId) => this.emitCancel(messageId),
    })
    void this.probe()
  }

  get available(): boolean {
    return this.wantsSpeech && this.ttsReady
  }

  /** Owner opted the persona into speaking. Independent of TTS being ready. */
  get speechEnabled(): boolean {
    return this.wantsSpeech
  }

  applyStatus(
    status: Pick<
      SpeechStatus,
      'available' | 'tts_enabled' | 'persona_speech_enabled'
    >,
  ): void {
    this.statusEpoch += 1
    const flags = personaSpeechFlags(status)
    this.wantsSpeech = flags.speechEnabled
    this.ttsReady = flags.ttsReady
    this.enabled = flags.speechEnabled && flags.ttsReady
  }

  async probe(): Promise<boolean> {
    const epoch = this.statusEpoch
    try {
      const status = await getSpeechStatus()
      if (this.statusEpoch !== epoch) return this.enabled
      this.applyStatus(status)
    } catch {
      if (this.statusEpoch !== epoch) return this.enabled
      this.applyStatus({
        available: false,
        tts_enabled: false,
        persona_speech_enabled: false,
      })
    }
    return this.enabled
  }

  feed(segments: readonly SpeechSegment[]): void {
    if (!this.enabled || segments.length === 0) return
    for (const segment of segments) this.fedMessageIds.add(segment.messageId)
    const mode = segments[0]!.interrupt
    this.pipeline.enqueue(segments, mode)
  }

  alreadyFed(messageId: string): boolean {
    return this.fedMessageIds.has(messageId)
  }

  isBusyWith(messageId: string): boolean {
    return this.pipeline.isBusyWith(messageId)
  }

  /**
   * One finished line through the same TTS queue as streamed Chat.
   * Returns false when TTS is off so callers may fall back to text visemes.
   */
  speakLine(input: {
    messageId: string
    text: string
    generation?: number
    interrupt?: SpeechInterruptMode
  }): boolean {
    if (!this.enabled) return false
    const text = speakableText(input.text)
    if (!text) return false
    const interrupt = input.interrupt ?? 'queue'
    const splitter = new SpeechSegmenter(input.messageId, input.generation ?? 0)
    const segments = [
      ...splitter.push(text, interrupt),
      ...splitter.end(interrupt),
    ]
    if (segments.length === 0) return false
    this.feed(segments)
    return true
  }

  cancel(messageId?: string): void {
    if (messageId) this.fedMessageIds.delete(messageId)
    else this.fedMessageIds.clear()
    const stopped = this.pipeline.cancel(messageId)
    if (!stopped) return
    this.cancelledAt = nowMs()
    patchVoicePresence({ ttsPlaying: false })
    this.noteSilence()
  }

  private async synthesize(
    segment: SpeechSegment,
  ): Promise<ArrayBuffer | null> {
    try {
      const result = await textToSpeech({
        text: segment.text.slice(0, 150),
        codec: 'mp3',
        sample_rate: 16000,
      })
      if (!result.success || !result.audio) return null
      return audioFromBase64(result.audio)
    } catch {
      return null
    }
  }

  private play(
    audio: ArrayBuffer,
    segment: SpeechSegment,
    onEnded: () => void,
  ): { stop: () => void } {
    const generation = liveMotionGeneration()
    const utteranceId = `tts-${segment.segmentId}`
    dispatchMeropeSpeech({
      phase: 'start',
      messageId: segment.messageId,
      source: 'reply',
      utteranceId,
      ...(generation ? { generation } : {}),
    })
    patchVoicePresence({ ttsPlaying: true })
    const handle = playTtsBuffer(audio, segment, {
      onStarted: () => markTurnTraceOnce('first_audio'),
      onProsody: (timeline, timing) => {
        dispatchMeropeSpeech({
          phase: 'prosody',
          messageId: segment.messageId,
          source: 'reply',
          utteranceId,
          prosody: {
            ...timeline,
            utteranceId,
            startedAtMs: timing.startedAtMs,
          },
          ...(generation ? { generation } : {}),
        })
      },
      onEnergy: (energy, articulation) => {
        dispatchMeropeSpeech({
          phase: 'energy',
          messageId: segment.messageId,
          source: 'reply',
          utteranceId,
          energy,
          ...(generation ? { generation } : {}),
        })
        dispatchMeropeSpeech({
          phase: 'articulation',
          messageId: segment.messageId,
          source: 'reply',
          utteranceId,
          articulation,
          ...(generation ? { generation } : {}),
        })
      },
      onEnded: () => {
        onEnded()
        if (this.pipeline.playing || this.pipeline.queueLength > 0) return
        dispatchMeropeSpeech({
          phase: 'end',
          messageId: segment.messageId,
          source: 'reply',
          utteranceId,
          ...(generation ? { generation } : {}),
        })
        patchVoicePresence({ ttsPlaying: false })
        markTurnTraceOnce('speech_ended')
      },
    })
    return {
      stop: () => {
        handle.stop()
        patchVoicePresence({ ttsPlaying: false })
        this.noteSilence()
      },
    }
  }

  private noteSilence(): void {
    if (this.cancelledAt == null) return
    noteTurnTraceCancelToSilence(nowMs() - this.cancelledAt)
    this.cancelledAt = null
    markTurnTraceOnce('speech_ended')
  }

  private emitCancel(messageId: string): void {
    dispatchMeropeSpeech({
      phase: 'cancel',
      messageId,
      source: 'reply',
    })
  }
}

let host: SpeechPipelineHost | null = null

export function getSpeechPipeline(): SpeechPipelineHost {
  if (!host) host = new SpeechPipelineHost()
  return host
}

function nowMs(): number {
  return typeof performance === 'undefined' ? Date.now() : performance.now()
}
