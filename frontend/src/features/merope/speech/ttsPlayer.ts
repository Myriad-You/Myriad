import type { SpeechArticulation, SpeechViseme } from '../rig/articulation'
import type { SpeechProsodyTimeline } from './prosody'
import type { SpeechSegment } from './speechSegmenter'
import type { VisemeSpan } from './visemeTimeline'
import { speechProsodyTimeline } from './prosody'
import { alignTextProsody } from './textProsody'
import { compileTextVisemes } from './textVisemes'
import {
  alignVisemeTimeline,
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
}

const FFT = 256
/** One display frame plus the mouth driver's response, without leading speech visibly. */
export const SPEECH_MOUTH_PREDICTION_SECONDS = 0.045
const ENERGY_WINDOW_SECONDS = 0.018

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

  const stop = (): void => {
    if (stopped) return
    stopped = true
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
      const startedAt = ctx.currentTime
      const startedAtMs = wallNow()
      const prosodyInput = {
        utteranceId: `tts-${segment.segmentId}`,
        text: segment.text,
        locale: segment.locale,
        startedAtMs,
      }
      hooks.onProsody?.(
        alignTextProsody(prosodyInput, {
          durationMs: Math.round(decoded.duration * 1_000),
          accents: [],
        }),
        { startedAtMs },
      )
      void compiled.then((cues) => {
        if (stopped) return
        spans = alignVisemeTimeline(cues, decoded.duration)
        const audioProsody = speechProsodyTimeline(spans, decoded.duration)
        const elapsedMs = Math.max(0, wallNow() - startedAtMs)
        const timeline = alignTextProsody(prosodyInput, {
          ...audioProsody,
          accents: audioProsody.accents.filter(
            (accent) => accent.offsetMs >= elapsedMs + 140,
          ),
        })
        hooks.onProsody?.(timeline, {
          startedAtMs,
        })
      })
      const tick = (): void => {
        if (stopped || !analyser) return
        const elapsed = Math.max(0, ctx.currentTime - startedAt)
        const sample =
          sampleDecodedMouth(
            decoded,
            spans,
            elapsed,
            SPEECH_MOUTH_PREDICTION_SECONDS,
          ) ?? analyserMouth(analyser, bins, spans, elapsed)
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

  return { stop }
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
  const analyser = ctx.createAnalyser()
  analyser.fftSize = FFT
  analyser.connect(ctx.destination)
  const bins: Uint8Array<ArrayBuffer> = new Uint8Array(new ArrayBuffer(FFT))
  const playing = new Set<AudioBufferSourceNode>()
  let nextAt = 0
  let raf = 0

  const finish = (): void => {
    if (stopped) return
    stopped = true
    if (raf) cancelAnimationFrame(raf)
    analyser.disconnect()
    hooks.onEnded()
  }
  const tick = (): void => {
    if (stopped) return
    const sample = analyserMouth(analyser, bins, [], 0)
    hooks.onEnergy(sample.energy ?? 0, sample)
    raf = requestAnimationFrame(tick)
  }

  return {
    push(bytes) {
      if (stopped || ending) return
      const read = pcmSamples(carry, bytes)
      carry = read.carry
      if (read.samples.length === 0) return
      if (ctx.state === 'suspended') void ctx.resume()
      const buffer = ctx.createBuffer(1, read.samples.length, PCM_STREAM_SAMPLE_RATE)
      buffer.getChannelData(0).set(read.samples)
      const source = ctx.createBufferSource()
      source.buffer = buffer
      source.connect(analyser)
      const at = Math.max(nextAt, ctx.currentTime + PCM_LEAD_SECONDS)
      nextAt = at + buffer.duration
      playing.add(source)
      source.onended = () => {
        playing.delete(source)
        source.disconnect()
        if (ending && playing.size === 0) finish()
      }
      source.start(at)
      if (!started) {
        started = true
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
      analyser.disconnect()
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
  const predicted = Math.max(0, seconds + Math.max(0, predictionSeconds))
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
  }
  return sharedContext
}

export function speechAudioContextOpen(): boolean {
  return Boolean(sharedContext && sharedContext.state !== 'closed')
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
