import type { TextVisemeCue } from '../anime25drig/textVisemes'
import type { SpeechArticulation } from '../rig/articulation'
import { compileTextVisemes } from '../anime25drig/textVisemes'
import {
  alignVisemeTimeline,
  VISEME_SILENCE_ENERGY,
  visemeAmount,
} from './visemeTimeline'

const MAX_WORDS = 512
const MAX_MESSAGE_BYTES = 64_000
const MAX_WORD_DURATION_MS = 30_000
const DEFAULT_WORD_DURATION_MS = 180
const AUDIO_TO_DISPLAY_LEAD_MS = 45
const MAX_PTS_EXTRAPOLATION_MS = 120
const STALE_PTS_MS = 300

export interface RtcTranscriptWord {
  word: string
  start_ms: number
  duration_ms: number
}

interface AbsoluteVisemeSpan {
  startsAtMs: number
  endsAtMs: number
  viseme: SpeechArticulation['viseme']
  emphasis: boolean
}

interface TranscriptFrame {
  turnId: number
  sequence: number | null
  interrupted: boolean
  locale: string
  words: RtcTranscriptWord[]
}

interface TurnWords {
  sequence: number | null
  locale: string
  words: Map<number, RtcTranscriptWord>
}

type TextVisemeCompiler = (
  text: string,
  locale?: string,
) => Promise<TextVisemeCue[]>

/** Provider word timestamps own the clock; Myriad's existing compiler owns shape. */
export async function compileRtcVisemeTimeline(
  words: readonly RtcTranscriptWord[],
  locale?: string,
  compiler: TextVisemeCompiler = compileTextVisemes,
): Promise<AbsoluteVisemeSpan[]> {
  const spans: AbsoluteVisemeSpan[] = []
  for (let index = 0; index < words.length; index += 1) {
    const word = words[index]!
    const nextStart = words[index + 1]?.start_ms
    const available =
      nextStart === undefined
        ? MAX_WORD_DURATION_MS
        : Math.max(0, nextStart - word.start_ms)
    const durationMs = Math.min(
      word.duration_ms > 0
        ? word.duration_ms
        : nextStart === undefined
          ? DEFAULT_WORD_DURATION_MS
          : available,
      available,
      MAX_WORD_DURATION_MS,
    )
    const cues = await compiler(word.word, locale)
    let startsAtMs = word.start_ms
    for (const span of alignVisemeTimeline(cues, durationMs / 1_000)) {
      const endsAtMs = word.start_ms + span.endsAt * 1_000
      spans.push({
        startsAtMs,
        endsAtMs,
        viseme: span.viseme,
        emphasis: span.emphasis,
      })
      startsAtMs = endsAtMs
    }
  }
  return spans
}

/**
 * A bounded transport-only timing cache, not another transcript/Chat store.
 * RTM can arrive before the authenticated run notice. Buffer those words, but
 * never drive a mouth until that exact provider turn is adopted by Agent Chat.
 */
export class RtcSpeechAlignment {
  private providerTurnId: number | null = null
  private cancelledThrough = -1
  private locale: string | undefined
  private revision = 0
  private requestedSignature = ''
  private spans: AbsoluteVisemeSpan[] = []
  private ptsMs: number | null = null
  private ptsObservedAtMs = 0
  private readonly turns = new Map<number, TurnWords>()
  private readonly compiled = new Map<string, Promise<TextVisemeCue[]>>()

  constructor(
    private readonly agentUid: string,
    private readonly channel: string,
    private readonly compiler: TextVisemeCompiler = compileTextVisemes,
  ) {}

  select(providerTurnId: number, locale?: string): Promise<boolean> {
    this.providerTurnId = providerTurnId
    this.locale = locale
    this.revision += 1
    this.requestedSignature = ''
    this.spans = []
    this.compiled.clear()
    for (const turn of this.turns.keys()) {
      if (turn < providerTurnId) this.turns.delete(turn)
    }
    return this.rebuild()
  }

  cancel(): void {
    if (this.providerTurnId !== null) {
      this.cancelledThrough = Math.max(
        this.cancelledThrough,
        this.providerTurnId,
      )
      this.turns.delete(this.providerTurnId)
    }
    this.providerTurnId = null
    this.revision += 1
    this.requestedSignature = ''
    this.spans = []
    this.compiled.clear()
  }

  clear(): void {
    this.cancel()
    this.turns.clear()
    this.ptsMs = null
    this.ptsObservedAtMs = 0
  }

  noteAudioPts(ptsMs: number, observedAtMs = monotonicNow()): void {
    // PTS can be Unix milliseconds. Do not cap it to a session duration.
    if (!timestamp(ptsMs) || (this.ptsMs !== null && ptsMs <= this.ptsMs))
      return
    this.ptsMs = ptsMs
    this.ptsObservedAtMs = observedAtMs
  }

  async update(event: unknown): Promise<boolean> {
    const frame = parseTranscript(event, this.channel, this.agentUid)
    if (
      !frame ||
      frame.turnId <= this.cancelledThrough ||
      (this.providerTurnId !== null && frame.turnId < this.providerTurnId)
    ) {
      return false
    }
    if (frame.interrupted) {
      this.turns.delete(frame.turnId)
      this.cancelledThrough = Math.max(this.cancelledThrough, frame.turnId)
      if (frame.turnId === this.providerTurnId) this.cancel()
      return false
    }
    const turn = this.turns.get(frame.turnId) ?? {
      sequence: null,
      locale: frame.locale,
      words: new Map<number, RtcTranscriptWord>(),
    }
    if (
      frame.sequence !== null &&
      turn.sequence !== null &&
      frame.sequence < turn.sequence
    ) {
      return false
    }
    turn.sequence = frame.sequence ?? turn.sequence
    turn.locale = frame.locale || turn.locale
    for (const word of frame.words) turn.words.set(word.start_ms, word)
    const oldestPts = this.ptsMs === null ? 0 : this.ptsMs - 1_000
    const retained = [...turn.words.values()]
      .filter(
        (word) =>
          word.start_ms +
            Math.max(word.duration_ms, DEFAULT_WORD_DURATION_MS) >=
          oldestPts,
      )
      .sort((a, b) => a.start_ms - b.start_ms)
      .slice(-MAX_WORDS)
    turn.words = new Map(retained.map((word) => [word.start_ms, word]))
    this.turns.set(frame.turnId, turn)
    // Only a few in-flight turns can overtake their server notices.
    const ids = [...this.turns.keys()].sort((a, b) => a - b)
    while (ids.length > 3) this.turns.delete(ids.shift()!)
    return frame.turnId === this.providerTurnId ? this.rebuild() : false
  }

  private async rebuild(): Promise<boolean> {
    const turnId = this.providerTurnId
    if (turnId === null || turnId <= this.cancelledThrough) return false
    const turn = this.turns.get(turnId)
    if (!turn || !turn.words.size) return false
    const words = [...turn.words.values()]
    const locale = turn.locale || this.locale
    const signature = JSON.stringify([locale, words])
    if (signature === this.requestedSignature) return false
    this.requestedSignature = signature
    const revision = this.revision
    try {
      const spans = await compileRtcVisemeTimeline(
        words,
        locale,
        (text, lang) => {
          if (revision !== this.revision)
            throw new Error('Speech turn replaced')
          const key = `${lang}:${text}`
          let result = this.compiled.get(key)
          if (!result) {
            result = this.compiler(text, lang)
            if (this.compiled.size >= MAX_WORDS)
              this.compiled.delete(this.compiled.keys().next().value!)
            this.compiled.set(key, result)
          }
          return result
        },
      )
      if (revision !== this.revision || signature !== this.requestedSignature)
        return false
      this.spans = spans
      return spans.length > 0
    } catch {
      if (revision === this.revision && signature === this.requestedSignature) {
        this.requestedSignature = ''
        this.compiled.clear()
      }
      return false
    }
  }

  sample(
    energy: number,
    observedAtMs = monotonicNow(),
  ): SpeechArticulation | null {
    if (
      this.ptsMs === null ||
      !this.spans.length ||
      this.providerTurnId === null ||
      this.providerTurnId <= this.cancelledThrough
    ) {
      return null
}
    const ageMs = Math.max(0, observedAtMs - this.ptsObservedAtMs)
    if (ageMs > STALE_PTS_MS) return null
    const ptsMs =
      this.ptsMs +
      Math.min(MAX_PTS_EXTRAPOLATION_MS, ageMs) +
      AUDIO_TO_DISPLAY_LEAD_MS
    if (
      ptsMs < this.spans[0]!.startsAtMs ||
      ptsMs >= this.spans.at(-1)!.endsAtMs
    ) {
      return null
    }
    const span = this.spans.find(
      (item) => ptsMs >= item.startsAtMs && ptsMs < item.endsAtMs,
    )
    const normalizedEnergy = Number.isFinite(energy)
      ? Math.max(0, Math.min(1, energy))
      : 0
    if (
      !span ||
      span.viseme === 'rest' ||
      normalizedEnergy < VISEME_SILENCE_ENERGY
    ) {
      return { energy: normalizedEnergy, viseme: 'rest', amount: 0 }
    }
    return {
      energy: normalizedEnergy,
      viseme: span.viseme,
      amount: visemeAmount(normalizedEnergy, span.emphasis),
    }
  }
}

function parseTranscript(
  event: unknown,
  channel: string,
  agentUid: string,
): TranscriptFrame | null {
  if (
    !isRecord(event) ||
    event.channelName !== channel ||
    event.publisher !== agentUid
  ) {
    return null
  }
  const payload = event.message
  let value: unknown
  try {
    if (typeof payload === 'string' && payload.length <= MAX_MESSAGE_BYTES) {
      value = JSON.parse(payload)
    } else if (
      payload instanceof Uint8Array &&
      payload.byteLength <= MAX_MESSAGE_BYTES
    ) {
      value = JSON.parse(new TextDecoder().decode(payload))
    } else {
      return null
    }
  } catch {
    return null
  }
  if (
    !isRecord(value) ||
    !Number.isSafeInteger(value.turn_id) ||
    Number(value.turn_id) < 0 ||
    (value.object !== 'assistant.transcription' &&
      value.object !== 'message.interrupt')
  ) {
    return null
  }
  const sequence =
    typeof value.turn_seq_id === 'number' &&
    Number.isSafeInteger(value.turn_seq_id)
      ? value.turn_seq_id
      : null
  const locale =
    typeof value.language === 'string' &&
    /^[A-Z]{2,3}(?:-[A-Z0-9]{2,8})?$/i.test(value.language)
      ? value.language
      : ''
  const words: RtcTranscriptWord[] = []
  for (const item of Array.isArray(value.words)
    ? value.words.slice(-MAX_WORDS)
    : []) {
    if (
      !isRecord(item) ||
      typeof item.word !== 'string' ||
      !item.word ||
      !timestamp(item.start_ms)
    ) {
      continue
    }
    // The official protocol can omit duration; the next word then bounds it.
    const duration = item.duration_ms === undefined ? 0 : item.duration_ms
    if (
      typeof duration !== 'number' ||
      !Number.isFinite(duration) ||
      duration < 0 ||
      duration > MAX_WORD_DURATION_MS
    ) {
      continue
    }
    words.push({
      word: item.word.slice(0, 160),
      start_ms: item.start_ms,
      duration_ms: duration,
    })
  }
  return {
    turnId: Number(value.turn_id),
    sequence,
    locale,
    words,
    interrupted:
      value.object === 'message.interrupt' || value.turn_status === 2,
  }
}

function timestamp(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function monotonicNow(): number {
  return typeof performance === 'undefined' ? Date.now() : performance.now()
}
