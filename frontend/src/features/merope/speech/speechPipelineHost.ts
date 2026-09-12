import type { SpeechStatus } from '../../../services/speechApi'
import type { MeropeSpeechSource } from '../speechEvents'
import type { SpeechInterruptMode, SpeechSegment } from './speechSegmenter'
import { getSpeechStatus, textToSpeech } from '../../../services/speechApi'
import { authSubject } from '../../../utils/authSubject'
import { dispatchMeropeSpeech } from '../speechEvents'
import { estimateAutoSpeechDurationMs } from '../speechLifecycle'
import { markTurnTraceOnce, noteTurnTraceCancelToSilence } from '../turnTrace'
import { speakableText } from './speakableText'
import { SpeechSegmenter } from './speechSegmenter'
import { TtsPipeline } from './ttsPipeline'
import { playTtsBuffer } from './ttsPlayer'
import { patchVoicePresence } from './voicePresence'

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

export class SpeechPipelineHost {
  readonly pipeline: TtsPipeline
  private enabled = false
  private wantsSpeech = false
  private ttsReady = false
  private statusEpoch = 0
  private cancelledAt: number | null = null
  private readonly fedMessageIds = new Set<string>()
  private readonly cancelledMessageIds = new Set<string>()

  constructor() {
    this.pipeline = new TtsPipeline({
      synthesize: (segment, signal) => this.synthesize(segment, signal),
      play: (audio, segment, onEnded) => this.play(audio, segment, onEnded),
      fallback: (segment, onEnded) => this.playText(segment, onEnded),
    })
    void this.probe()
  }

  get available(): boolean {
    return this.wantsSpeech && this.ttsReady
  }

  get speechEnabled(): boolean {
    return this.wantsSpeech
  }

  /** Invalidate status probes too: a late old-account response cannot re-enable TTS. */
  resetSubject(): void {
    this.cancel()
    this.applyStatus({ available: false, tts_enabled: false, persona_speech_enabled: false })
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
    const wasEnabled = this.enabled
    this.enabled = flags.speechEnabled && flags.ttsReady
    if (wasEnabled && !this.enabled) this.cancel()
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
    if (!this.enabled) return
    const accepted = segments.filter(
      (segment) => !this.cancelledMessageIds.has(segment.messageId),
    )
    if (accepted.length === 0) return
    for (const segment of accepted)
      rememberMessage(this.fedMessageIds, segment.messageId)
    const mode = accepted[0]!.interrupt
    this.pipeline.enqueue(accepted, mode)
  }

  alreadyFed(messageId: string): boolean {
    return (
      this.fedMessageIds.has(messageId) ||
      this.cancelledMessageIds.has(messageId)
    )
  }

  isBusyWith(messageId: string): boolean {
    return this.pipeline.isBusyWith(messageId)
  }

  speakLine(input: {
    messageId: string
    text: string
    generation?: number
    source?: MeropeSpeechSource
    interrupt?: SpeechInterruptMode
  }): boolean {
    if (this.cancelledMessageIds.has(input.messageId)) return true
    if (!this.enabled) return false
    const text = speakableText(input.text)
    if (!text) return false
    const interrupt = input.interrupt ?? 'queue'
    const splitter = new SpeechSegmenter(
      input.messageId,
      input.generation ?? 0,
      undefined,
      input.source,
    )
    const segments = [
      ...splitter.push(text, interrupt),
      ...splitter.end(interrupt),
    ]
    if (segments.length === 0) return false
    this.feed(segments)
    return true
  }

  cancel(messageId?: string): void {
    if (messageId) {
      rememberMessage(this.cancelledMessageIds, messageId)
      this.fedMessageIds.delete(messageId)
    } else {
      for (const id of this.fedMessageIds)
        rememberMessage(this.cancelledMessageIds, id)
      this.fedMessageIds.clear()
    }
    this.cancelledAt = nowMs()
    const stopped = this.pipeline.cancel(messageId)
    if (!stopped) {
      this.cancelledAt = null
      return
    }
    this.noteSilence()
  }

  private async synthesize(
    segment: SpeechSegment,
    signal: AbortSignal,
  ): Promise<ArrayBuffer | null> {
    try {
      const result = await textToSpeech(
        {
          text: segment.text.slice(0, 150),
          codec: 'mp3',
          sample_rate: 16000,
        },
        undefined,
        signal,
      )
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
    const generation = segment.generation
    const source = segment.source ?? 'reply'
    const utteranceId = `tts-${segment.segmentId}`
    dispatchMeropeSpeech({
      phase: 'start',
      messageId: segment.messageId,
      source,
      utteranceId,
      ...(generation ? { generation } : {}),
    })
    patchVoicePresence({ ttsPlaying: true })
    const handle = playTtsBuffer(audio, segment, {
      onStarted: () => markTurnTraceOnce('first_audio'),
      onProsody: (timeline, timing) => {
        dispatchMeropeSpeech({
          phase: 'prosody',
          text: segment.text,
          messageId: segment.messageId,
          source,
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
          source,
          utteranceId,
          energy,
          ...(generation ? { generation } : {}),
        })
        dispatchMeropeSpeech({
          phase: 'articulation',
          messageId: segment.messageId,
          source,
          utteranceId,
          articulation,
          ...(generation ? { generation } : {}),
        })
      },
      onEnded: () => {
        dispatchMeropeSpeech({
          phase: 'end',
          messageId: segment.messageId,
          source,
          utteranceId,
          ...(generation ? { generation } : {}),
        })
        patchVoicePresence({ ttsPlaying: false })
        onEnded()
        if (!this.pipeline.playing && this.pipeline.queueLength === 0)
          markTurnTraceOnce('speech_ended')
      },
    })
    return {
      stop: () => {
        handle.stop()
        dispatchMeropeSpeech({
          phase: 'cancel',
          messageId: segment.messageId,
          source,
          generation,
          utteranceId,
        })
        patchVoicePresence({ ttsPlaying: false })
        this.noteSilence()
      },
    }
  }

  private playText(
    segment: SpeechSegment,
    onEnded: () => void,
  ): { stop: () => void } {
    const base = {
      messageId: segment.messageId,
      source: segment.source ?? 'reply',
      generation: segment.generation,
      utteranceId: `text-${segment.segmentId}`,
    }
    dispatchMeropeSpeech({ ...base, phase: 'start' })
    dispatchMeropeSpeech({ ...base, phase: 'chunk', text: segment.text })
    dispatchMeropeSpeech({ ...base, phase: 'end' })
    const timer = setTimeout(
      onEnded,
      Math.max(180, estimateAutoSpeechDurationMs(segment.text, segment.locale)),
    )
    return {
      stop: () => {
        clearTimeout(timer)
        dispatchMeropeSpeech({ ...base, phase: 'cancel' })
      },
    }
  }

  private noteSilence(): void {
    if (this.cancelledAt == null) return
    noteTurnTraceCancelToSilence(nowMs() - this.cancelledAt)
    this.cancelledAt = null
    markTurnTraceOnce('speech_ended')
  }
}

let host: SpeechPipelineHost | null = null
authSubject.subscribe(() => host?.resetSubject())

export function getSpeechPipeline(): SpeechPipelineHost {
  if (!host) host = new SpeechPipelineHost()
  return host
}

function nowMs(): number {
  return typeof performance === 'undefined' ? Date.now() : performance.now()
}

/** Late SSE/final-response dedupe is bounded and never persisted. */
function rememberMessage(ids: Set<string>, id: string): void {
  ids.add(id)
  if (ids.size > 256) ids.delete(ids.values().next().value!)
}
