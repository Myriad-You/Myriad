import type { SpeechArticulation, SpeechViseme } from '../rig/articulation'
import type { SpeechProsodyTimeline } from './prosody'
import type { SpeechSegment } from './speechSegmenter'
import type { VisemeSpan } from './visemeTimeline'
import { speechProsodyTimeline } from './prosody'
import { alignTextProsody } from './textProsody'
import { compileTextVisemes } from './textVisemes'
import {
  alignVisemeTimelineToAudio,
  audioVoicing,
  textTimeOnAudio,
  VISEME_SILENCE_ENERGY,
  visemeAmount,
  visemeAt,
} from './visemeTimeline'

export interface TtsPlayHooks {
  onEnergy: (energy: number, articulation: SpeechArticulation) => void
  onProsody?: (
    timeline: SpeechProsodyTimeline,
    timing: TtsProsodyTiming,
  ) => void
  /**
   * It is about to be heard: decoded, not held, resumed. Called in the same
   * task as its first mouth frame and its start, before both.
   */
  onBegin?: () => void
  onStarted?: () => void
  onEnded: () => void
}

export interface TtsProsodyTiming {
  startedAtMs: number
}

export interface TtsPlaybackDependencies {
  compileVisemes?: typeof compileTextVisemes
  now?: () => number
}

export interface TtsPlayHandle {
  stop: () => void
  /** How far into the audio it has played, 0 to 1; 0 before it starts. */
  progress?: () => number
}

const FFT = 256
/** One display frame plus the mouth driver's response, without leading speech visibly. */
export const SPEECH_MOUTH_PREDICTION_SECONDS = 0.045
const ENERGY_WINDOW_SECONDS = 0.018
/** Past this an output delay is a wrong reading, not a headset. */
const MAX_OUTPUT_LATENCY_SECONDS = 1
/** Smaller changes in the output delay are jitter, not a new device. */
const OUTPUT_LATENCY_SHIFT_MS = 10

/**
 * How far what is heard trails the audio clock: the output device's delay, a
 * Bluetooth headset's most of all. 0 when the browser does not say, which
 * leaves the mouth on the audio clock as before.
 */
export function speechOutputLatencySeconds(
  ctx: Partial<Pick<AudioContext, 'outputLatency' | 'baseLatency'>>,
): number {
  const output = ctx.outputLatency
  if (typeof output !== 'number' || !Number.isFinite(output) || output <= 0)
    return 0
  const base = ctx.baseLatency
  const processing =
    typeof base === 'number' && Number.isFinite(base) && base > 0 ? base : 0
  return Math.min(MAX_OUTPUT_LATENCY_SECONDS, output + processing)
}

export function playTtsBuffer(
  audio: ArrayBuffer,
  segment: SpeechSegment,
  hooks: TtsPlayHooks,
  context?: AudioContext,
  dependencies: TtsPlaybackDependencies = {},
): TtsPlayHandle {
  let stopped = false
  if (!context && typeof AudioContext === 'undefined') {
    queueMicrotask(() => {
      if (stopped) return
      stopped = true
      hooks.onEnded()
    })
    return {
      stop: () => {
        stopped = true
      },
    }
  }
  const ctx = context ?? speechAudioContext()
  let source: AudioBufferSourceNode | null = null
  let analyser: AnalyserNode | null = null
  let raf = 0
  let unheld = (): void => {}
  let played = (): number => 0

  const stop = (): void => {
    if (stopped) return
    stopped = true
    unheld()
    if (raf) cancelAnimationFrame(raf)
    try {
      source?.stop()
    } catch {
    }
    source?.disconnect()
    analyser?.disconnect()
    source = null
    analyser = null
  }

  const compileVisemes = dependencies.compileVisemes ?? compileTextVisemes
  const wallNow = dependencies.now ?? monotonicNow
  const compiled = compileVisemes(segment.text, segment.locale).catch(() => [])
  let spans: VisemeSpan[] = []
  void ctx
    .decodeAudioData(audio.slice(0))
    .then(async (decoded) => {
      if (stopped) return
      // Do not spend its phrase preparation or hold a predicted mouth pose while resume is pending
      if (ctx === sharedContext) await whenSpeechAudioReleased()
      if (stopped) return
      if (ctx.state === 'suspended') await ctx.resume()
      if (stopped) return
      analyser = ctx.createAnalyser()
      analyser.fftSize = FFT
      source = ctx.createBufferSource()
      source.buffer = decoded
      source.connect(analyser)
      analyser.connect(ctx.destination)
      const bins: Uint8Array<ArrayBuffer> = new Uint8Array(
        new ArrayBuffer(analyser.fftSize),
      )
      hooks.onBegin?.()
      const startedAt = ctx.currentTime
      // Heard later than the audio clock by the output delay, read afresh
      // each time: a headset connected mid-line moves it.
      const heardAt = (): number =>
        ctx.currentTime - startedAt - speechOutputLatencySeconds(ctx)
      // A held context stands still, so a hold is not counted as heard.
      played = () =>
        decoded.duration > 0
          ? Math.min(1, Math.max(0, heardAt() / decoded.duration))
          : 0
      let latencyMs = speechOutputLatencySeconds(ctx) * 1_000
      let startedAtMs = wallNow() + latencyMs
      const prosodyInput = {
        utteranceId: `tts-${segment.segmentId}`,
        text: segment.text,
        locale: segment.locale,
        startedAtMs,
      }
      let lastProsody = alignTextProsody(prosodyInput, {
        durationMs: Math.round(decoded.duration * 1_000),
        accents: [],
      })
      hooks.onProsody?.(lastProsody, { startedAtMs })
      // A hold pauses the voice; its gestures move later by as long.
      unheld = onSpeechAudioReleased((heldMs) => {
        if (stopped) return
        startedAtMs += heldMs
        prosodyInput.startedAtMs = startedAtMs
        hooks.onProsody?.(lastProsody, { startedAtMs })
      })
      void compiled.then((cues) => {
        if (stopped) return
        // The mouth keeps to the voice: its start, its end and its pauses.
        const voicing = typeof decoded.getChannelData === 'function' && decoded.numberOfChannels > 0
          ? audioVoicing(decoded.getChannelData(0), decoded.sampleRate)
          : null
        spans = alignVisemeTimelineToAudio(cues, decoded.duration, voicing)
        const audioProsody = speechProsodyTimeline(spans, decoded.duration)
        const elapsedMs = Math.max(0, wallNow() - startedAtMs)
        const timeline = (lastProsody = alignTextProsody(prosodyInput, {
          ...audioProsody,
          accents: audioProsody.accents.filter(
            (accent) => accent.offsetMs >= elapsedMs + 140,
          ),
        }, textTimeOnAudio(segment.text, voicing)))
        hooks.onProsody?.(timeline, {
          startedAtMs,
        })
      })
      const tick = (): void => {
        if (stopped || !analyser) return
        // A new output delay moves its gestures by as much, as a hold does.
        const nextLatencyMs = speechOutputLatencySeconds(ctx) * 1_000
        if (Math.abs(nextLatencyMs - latencyMs) >= OUTPUT_LATENCY_SHIFT_MS) {
          startedAtMs += nextLatencyMs - latencyMs
          latencyMs = nextLatencyMs
          prosodyInput.startedAtMs = startedAtMs
          hooks.onProsody?.(lastProsody, { startedAtMs })
        }
        const heard = heardAt()
        const sample = held
          ? HELD_MOUTH
          : sampleDecodedMouth(
            decoded,
            spans,
            heard,
            SPEECH_MOUTH_PREDICTION_SECONDS,
          ) ?? analyserMouth(analyser, bins, spans, Math.max(0, heard))
        hooks.onEnergy(sample.energy ?? 0, sample)
        raf = requestAnimationFrame(tick)
      }
      source.onended = () => {
        if (stopped) return
        stop()
        hooks.onEnded()
      }
      tick()
      source.start()
      hooks.onStarted?.()
    })
    .catch(() => {
      if (stopped) return
      stop()
      hooks.onEnded()
    })

  return { stop, progress: () => played() }
}

export interface PcmStreamHandle {
  /** A piece of 16-bit little-endian mono PCM, as it arrived. */
  push: (bytes: Uint8Array) => void
  /** Nothing more will come: ends once what was queued has played. */
  end: () => void
  stop: () => void
}

/** Her own voice as it is spoken: 16-bit mono PCM at 24 kHz. */
export const PCM_STREAM_SAMPLE_RATE = 24_000
/** Played this far behind its arrival, so a late piece does not leave a gap. */
const PCM_LEAD_SECONDS = 0.12

/** 16-bit little-endian samples as floats; an odd last byte waits for the next piece. */
export function pcmSamples(
  carry: Uint8Array | null,
  bytes: Uint8Array,
): { samples: Float32Array; carry: Uint8Array | null } {
  const joined =
    carry && carry.length > 0
      ? (() => {
          const all = new Uint8Array(carry.length + bytes.length)
          all.set(carry)
          all.set(bytes, carry.length)
          return all
        })()
      : bytes
  const whole = joined.length - (joined.length % 2)
  const samples = new Float32Array(whole / 2)
  const view = new DataView(joined.buffer, joined.byteOffset, whole)
  for (let i = 0; i < samples.length; i += 1) {
    samples[i] = view.getInt16(i * 2, true) / 32768
  }
  return {
    samples,
    carry: whole < joined.length ? joined.slice(whole) : null,
  }
}

/**
 * Plays a voice that arrives in pieces, each right after the one before; the
 * mouth follows what is heard. Started on the first piece, ended once `end`
 * was called and the last piece has played.
 */
export function playPcmStream(
  hooks: TtsPlayHooks,
  context?: AudioContext,
): PcmStreamHandle {
  let stopped = false
  let ending = false
  let started = false
  let carry: Uint8Array | null = null
  if (!context && typeof AudioContext === 'undefined') {
    return {
      push: () => {},
      end: () => {
        if (stopped) return
        stopped = true
        hooks.onEnded()
      },
      stop: () => {
        stopped = true
      },
    }
  }
  const ctx = context ?? speechAudioContext()
  const playing = new Set<AudioBufferSourceNode>()
  // What is still to be heard, in order: the mouth reads the pieces
  // themselves at the time they reach the ear, not the audio clock's.
  const pieces: { at: number; buffer: AudioBuffer }[] = []
  let nextAt = 0
  let raf = 0

  const finish = (): void => {
    if (stopped) return
    stopped = true
    if (raf) cancelAnimationFrame(raf)
    pieces.length = 0
    hooks.onEnded()
  }
  const tick = (): void => {
    if (stopped) return
    const heard = ctx.currentTime - speechOutputLatencySeconds(ctx)
    const ahead = heard + SPEECH_MOUTH_PREDICTION_SECONDS
    while (pieces.length > 0 && pieces[0]!.at + pieces[0]!.buffer.duration <= ahead)
      pieces.shift()
    const piece = pieces[0]
    const sample = held
      ? HELD_MOUTH
      : (piece && sampleDecodedMouth(piece.buffer, [], heard - piece.at)) ??
        articulationFromEnergy(0, [], 0)
    hooks.onEnergy(sample.energy ?? 0, sample)
    raf = requestAnimationFrame(tick)
  }

  return {
    push(bytes) {
      if (stopped || ending) return
      const read = pcmSamples(carry, bytes)
      carry = read.carry
      if (read.samples.length === 0) return
      if (ctx.state === 'suspended' && !(held && ctx === sharedContext))
        void ctx.resume()
      const buffer = ctx.createBuffer(1, read.samples.length, PCM_STREAM_SAMPLE_RATE)
      buffer.getChannelData(0).set(read.samples)
      const source = ctx.createBufferSource()
      source.buffer = buffer
      source.connect(ctx.destination)
      const at = Math.max(nextAt, ctx.currentTime + PCM_LEAD_SECONDS)
      nextAt = at + buffer.duration
      pieces.push({ at, buffer })
      playing.add(source)
      source.onended = () => {
        playing.delete(source)
        source.disconnect()
        if (ending && playing.size === 0) finish()
      }
      source.start(at)
      if (!started) {
        started = true
        hooks.onBegin?.()
        hooks.onStarted?.()
        tick()
      }
    },
    end() {
      if (stopped) return
      ending = true
      if (playing.size === 0) finish()
    },
    stop() {
      if (stopped) return
      stopped = true
      if (raf) cancelAnimationFrame(raf)
      for (const source of playing) {
        try {
          source.stop()
        } catch {
        }
        source.disconnect()
      }
      playing.clear()
      pieces.length = 0
    },
  }
}

export function sampleDecodedMouth(
  buffer: Pick<
    AudioBuffer,
    'duration' | 'numberOfChannels' | 'sampleRate' | 'getChannelData'
  >,
  spans: readonly VisemeSpan[] = [],
  seconds = 0,
  predictionSeconds = SPEECH_MOUTH_PREDICTION_SECONDS,
): SpeechArticulation | null {
  if (
    !Number.isFinite(buffer.sampleRate) ||
    buffer.sampleRate <= 0 ||
    !Number.isFinite(buffer.numberOfChannels) ||
    buffer.numberOfChannels <= 0 ||
    typeof buffer.getChannelData !== 'function'
  ) {
    return null
  }
  const predicted = seconds + Math.max(0, predictionSeconds)
  // Not reaching the ear yet: the mouth waits shut.
  if (predicted < 0) return articulationFromEnergy(0, spans, 0)
  if (predicted >= buffer.duration) {
    return articulationFromEnergy(0, spans, predicted)
  }
  const start = Math.max(0, Math.floor(predicted * buffer.sampleRate))
  const windowSamples = Math.max(
    1,
    Math.round(ENERGY_WINDOW_SECONDS * buffer.sampleRate),
  )
  let sum = 0
  let count = 0
  for (let channel = 0; channel < buffer.numberOfChannels; channel += 1) {
    const samples = buffer.getChannelData(channel)
    const end = Math.min(samples.length, start + windowSamples)
    for (let index = start; index < end; index += 1) {
      const value = samples[index] ?? 0
      sum += value * value
      count += 1
    }
  }
  const energy = count > 0 ? Math.min(1, Math.sqrt(sum / count) * 2.4) : 0
  return articulationFromEnergy(energy, spans, predicted)
}

let sharedContext: AudioContext | null = null

function speechAudioContext(): AudioContext {
  if (!sharedContext || sharedContext.state === 'closed') {
    sharedContext = new AudioContext()
    if (held) void sharedContext.suspend().catch(() => {})
  }
  return sharedContext
}

export function speechAudioContextOpen(): boolean {
  return Boolean(sharedContext && sharedContext.state !== 'closed')
}

/**
 * Held: her voice stops where it is, without being thrown away. The shared
 * context is suspended, so every source and the stream schedule freeze in
 * place and continue from the same sample on release.
 */
let held = false
let heldAtMs = 0
const releaseListeners = new Set<(heldMs: number) => void>()
/** A held voice leaves the mouth shut, not frozen mid-syllable. */
const HELD_MOUTH: SpeechArticulation = { energy: 0, viseme: 'rest', amount: 0 }
let releaseWaiters: (() => void)[] = []

/** Voice that has not started yet waits too: it starts once released. */
export function holdSpeechAudio(): void {
  if (held) return
  held = true
  heldAtMs = monotonicNow()
  if (sharedContext?.state === 'running')
    void sharedContext.suspend().catch(() => {})
}

export function releaseSpeechAudio(): void {
  if (!held) return
  held = false
  const heldMs = Math.max(0, monotonicNow() - heldAtMs)
  const waiters = releaseWaiters
  releaseWaiters = []
  for (const wake of waiters) wake()
  if (sharedContext?.state === 'suspended')
    void sharedContext.resume().catch(() => {})
  for (const listener of [...releaseListeners]) listener(heldMs)
}

function onSpeechAudioReleased(listener: (heldMs: number) => void): () => void {
  releaseListeners.add(listener)
  return () => releaseListeners.delete(listener)
}

export function speechAudioHeld(): boolean {
  return held
}

function whenSpeechAudioReleased(): Promise<void> {
  if (!held) return Promise.resolve()
  return new Promise((resolve) => releaseWaiters.push(resolve))
}

export function sampleMouth(
  bins: Uint8Array,
  spans: readonly VisemeSpan[] = [],
  seconds = 0,
): SpeechArticulation {
  let sum = 0
  for (let i = 0; i < bins.length; i++) {
    const centered = (bins[i]! - 128) / 128
    sum += centered * centered
  }
  const energy = Math.min(1, Math.sqrt(sum / Math.max(1, bins.length)) * 2.4)
  return articulationFromEnergy(energy, spans, seconds)
}

function analyserMouth(
  analyser: AnalyserNode,
  bins: Uint8Array<ArrayBuffer>,
  spans: readonly VisemeSpan[],
  seconds: number,
): SpeechArticulation {
  analyser.getByteTimeDomainData(bins)
  return sampleMouth(bins, spans, seconds)
}

function articulationFromEnergy(
  energy: number,
  spans: readonly VisemeSpan[],
  seconds: number,
): SpeechArticulation {
  const span = visemeAt(spans, seconds)
  if (!span) {
    return { energy, viseme: visemeFromEnergy(energy), amount: energy }
  }
  if (energy < VISEME_SILENCE_ENERGY) {
    return { energy, viseme: 'rest', amount: 0 }
  }
  return {
    energy,
    viseme: span.viseme,
    amount: visemeAmount(energy, span.emphasis),
  }
}

function monotonicNow(): number {
  return typeof performance === 'undefined' ? Date.now() : performance.now()
}

function visemeFromEnergy(energy: number): SpeechViseme {
  if (energy < 0.06) return 'rest'
  if (energy < 0.14) return 'narrow'
  if (energy < 0.28) return 'open'
  if (energy < 0.44) return 'round'
  return 'wide'
}
