/**
 * Local end-to-end IO trace. Ring + counters only — never the run hub,
 * never volume frames, never mouth samples.
 */

export const TURN_TRACE_SPANS = [
  'input_started',
  'input_ended',
  'asr_started',
  'asr_completed',
  'input_final',
  'request_sent',
  'reaction_ready',
  // Delivery readiness is a phase, not proof that a model director returned.
  // Local sentence delivery can precede the optional model revision.
  'delivery_ready',
  // Backend receipt is not proof of animation. This is stamped only after the
  // renderer accepts a semantic cue from the unified behavior plan.
  'performance_applied',
  'llm_first_token',
  'first_sentence',
  'tts_queued',
  'tts_ready',
  'playback_started',
  'first_audio',
  'speech_ended',
  'turn_completed',
] as const

export type TurnTraceSpan = (typeof TURN_TRACE_SPANS)[number]

export type TurnTraceDropReason =
  | 'stale_generation'
  | 'superseded'
  | 'hidden_face'
  | 'gated_record'
  | 'tts_unavailable'
  | 'synth_failed'
  | 'queue_replaced'
  | 'cancelled'
  | 'lease_conflict'
  | 'foreign_speech_frame'
  | 'behavior_rejected'

export type TurnTraceExtra = Record<string, string | number | boolean>

export interface TurnTraceMark {
  span: string
  t: number
  turnId: string
  extra?: TurnTraceExtra
}

export interface TurnTraceCounters {
  staleGenerationDrops: number
  leaseConflicts: number
  leaseExpiries: number
  droppedFrames: number
  ttsQueuePeak: number
  ttsQueueLength: number
  cancelToSilenceMs: number
  asrMs: number
  ttsSynthMs: number
  llmFirstTokenMs: number
  firstAudioMs: number
  requestToFirstAudioMs: number
  frameCpuMs: number
  audioContexts: number
  voiceListeners: number
  liveLeases: number
}

export interface TurnTraceSnapshot {
  turnId: string
  marks: readonly TurnTraceMark[]
  counters: TurnTraceCounters
  delays: {
    asrMs: number
    llmFirstTokenMs: number
    ttsSynthMs: number
    cancelToSilenceMs: number
    firstAudioMs: number
    requestToFirstAudioMs: number
  }
}

export type TurnTraceListener = (snapshot: TurnTraceSnapshot) => void

const RING = 96

function emptyCounters(): TurnTraceCounters {
  return {
    staleGenerationDrops: 0,
    leaseConflicts: 0,
    leaseExpiries: 0,
    droppedFrames: 0,
    ttsQueuePeak: 0,
    ttsQueueLength: 0,
    cancelToSilenceMs: 0,
    asrMs: 0,
    ttsSynthMs: 0,
    llmFirstTokenMs: 0,
    firstAudioMs: 0,
    requestToFirstAudioMs: 0,
    frameCpuMs: 0,
    audioContexts: 0,
    voiceListeners: 0,
    liveLeases: 0,
  }
}

let turnId = ''
const firsts = new Set<string>()
const marks: TurnTraceMark[] = []
let counters = emptyCounters()
const pending = new Map<
  TurnTraceSpan,
  { t: number; stagedAt: number; extra?: TurnTraceExtra }
>()
const PENDING_TTL_MS = 8_000
const listeners = new Set<TurnTraceListener>()

function now(): number {
  return typeof performance === 'undefined' ? Date.now() : performance.now()
}

function emit(mark: TurnTraceMark): void {
  if (typeof window === 'undefined') return
  if (typeof console.debug !== 'function') return
  console.debug(
    JSON.stringify({
      src: 'merope.trace',
      span: mark.span,
      t: Math.round(mark.t),
      turn: mark.turnId,
      ...(mark.extra ?? {}),
    }),
  )
}

function record(mark: TurnTraceMark): void {
  marks.push(mark)
  if (marks.length > RING) marks.shift()
  emit(mark)
  if (mark.span === 'asr_completed') {
    counters.asrMs =
      delayBetween('asr_started', 'asr_completed') ?? counters.asrMs
  }
  if (mark.span === 'llm_first_token') {
    counters.llmFirstTokenMs =
      delayBetween('request_sent', 'llm_first_token') ??
      counters.llmFirstTokenMs
  }
  if (mark.span === 'first_audio') {
    counters.firstAudioMs =
      delayBetween('playback_started', 'first_audio') ?? counters.firstAudioMs
    counters.requestToFirstAudioMs =
      delayBetween('request_sent', 'first_audio') ??
      counters.requestToFirstAudioMs
  }
  notify()
}

function delayBetween(from: string, to: string): number | null {
  const start = lastMark(from)
  const end = lastMark(to)
  if (!start || !end) return null
  return Math.max(0, Math.round(end.t - start.t))
}

function lastMark(span: string): TurnTraceMark | undefined {
  for (let i = marks.length - 1; i >= 0; i--) {
    const mark = marks[i]
    if (mark && mark.span === span && mark.turnId === turnId) return mark
  }
  return undefined
}

function notify(): void {
  if (listeners.size === 0) return
  const snap = snapshotTurnTrace()
  for (const listener of listeners) listener(snap)
}

export function subscribeTurnTrace(listener: TurnTraceListener): () => void {
  listeners.add(listener)
  listener(snapshotTurnTrace())
  return () => {
    listeners.delete(listener)
  }
}

export function beginTurnTrace(id: string): void {
  turnId = id
  firsts.clear()
  const attached = now()
  for (const span of TURN_TRACE_SPANS) {
    const held = pending.get(span)
    if (!held) continue
    if (attached - held.stagedAt > PENDING_TTL_MS) continue
    firsts.add(`${turnId}:${span}`)
    record({ span, t: held.t, turnId, extra: held.extra })
  }
  pending.clear()
}

/** Stamp a span before the owning turn exists (ASR start, VAD). Latest wins. */
export function stampTurnTrace(
  span: TurnTraceSpan,
  extra?: TurnTraceExtra,
): void {
  if (turnId) {
    markTurnTraceOnce(span, extra)
    return
  }
  const t = now()
  pending.set(span, { t, stagedAt: t, extra })
}

export interface VoiceInputTiming {
  input_started: number
  input_ended: number
  asr_started: number
  asr_completed: number
}

/**
 * Attach this utterance to the NEXT submitted run, never the previous reply
 * that happens to be playing while ASR runs. Stage only when committing text.
 */
export function stageVoiceInputTrace(timing: VoiceInputTiming): void {
  const stagedAt = now()
  for (const span of [
    'input_started',
    'input_ended',
    'asr_started',
    'asr_completed',
  ] as const) {
    pending.set(span, { t: timing[span], stagedAt })
  }
  pending.set('input_final', { t: stagedAt, stagedAt })
}

/** Drop pending input stamps from an abandoned recording. */
export function dropPendingTurnTrace(span?: TurnTraceSpan): void {
  if (span) pending.delete(span)
  else pending.clear()
}

export function markTurnTrace(span: string, extra?: TurnTraceExtra): void {
  record({ span, t: now(), turnId, extra })
}

export function markTurnTraceOnce(
  span: TurnTraceSpan | string,
  extra?: TurnTraceExtra,
): void {
  const key = `${turnId}:${span}`
  if (firsts.has(key)) return
  firsts.add(key)
  markTurnTrace(span, extra)
}

export function noteTurnTraceDrop(reason: TurnTraceDropReason): void {
  if (reason === 'stale_generation') counters.staleGenerationDrops += 1
  if (reason === 'lease_conflict') {
    counters.leaseConflicts += 1
    notify()
    return
  }
  markTurnTrace('drop', { reason })
}

export function noteTurnTraceQueue(length: number): void {
  const value = Math.max(0, Math.trunc(length))
  counters.ttsQueueLength = value
  if (value > counters.ttsQueuePeak) counters.ttsQueuePeak = value
  notify()
}

export function noteTurnTraceDelay(
  kind: 'asr' | 'tts' | 'llm',
  ms: number,
): void {
  const value = Math.max(0, Math.round(ms))
  if (kind === 'asr') counters.asrMs = value
  else if (kind === 'tts') counters.ttsSynthMs = value
  else counters.llmFirstTokenMs = value
  notify()
}

export function noteTurnTraceCancelToSilence(ms: number): void {
  counters.cancelToSilenceMs = Math.max(0, Math.round(ms))
  notify()
}

export function noteTurnTraceLeaseExpiry(count = 1): void {
  counters.leaseExpiries += Math.max(0, Math.trunc(count))
  notify()
}

export function noteTurnTraceFrame(input: {
  dropped: boolean
  cpuMs?: number
}): void {
  if (input.dropped) counters.droppedFrames += 1
  if (input.cpuMs != null && Number.isFinite(input.cpuMs)) {
    counters.frameCpuMs = Math.max(0, input.cpuMs)
  }
  notify()
}

export function noteTurnTraceLeaks(sample: {
  audioContexts: number
  voiceListeners: number
  leases: number
}): void {
  counters.audioContexts = Math.max(0, Math.trunc(sample.audioContexts))
  counters.voiceListeners = Math.max(0, Math.trunc(sample.voiceListeners))
  counters.liveLeases = Math.max(0, Math.trunc(sample.leases))
  notify()
}

export function snapshotTurnTrace(): TurnTraceSnapshot {
  return {
    turnId,
    marks: marks.slice(),
    counters: { ...counters },
    delays: {
      asrMs: counters.asrMs,
      llmFirstTokenMs: counters.llmFirstTokenMs,
      ttsSynthMs: counters.ttsSynthMs,
      cancelToSilenceMs: counters.cancelToSilenceMs,
      firstAudioMs: counters.firstAudioMs,
      requestToFirstAudioMs: counters.requestToFirstAudioMs,
    },
  }
}

export function serializeTurnTrace(): string {
  return `${JSON.stringify(snapshotTurnTrace(), null, 2)}\n`
}

export function resetTurnTraceForTest(): void {
  turnId = ''
  firsts.clear()
  marks.length = 0
  counters = emptyCounters()
  pending.clear()
  listeners.clear()
}
