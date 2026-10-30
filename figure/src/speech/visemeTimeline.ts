import type { SpeechViseme } from '../rig/articulation'
import type { TextVisemeCue } from './textVisemes'
import { visualSpeechPauseSeconds } from './textTiming'

/** Loudness is not a mouth shape. */
export interface VisemeSpan {
  viseme: SpeechViseme
  endsAt: number
  emphasis: boolean
}

export const VISEME_SILENCE_ENERGY = 0.06

export function alignVisemeTimeline(
  cues: readonly TextVisemeCue[],
  audioDurationSeconds: number,
): VisemeSpan[] {
  if (!Number.isFinite(audioDurationSeconds) || audioDurationSeconds <= 0) {
    return []
  }
  let nominal = 0
  for (const cue of cues) {
    if (Number.isFinite(cue.duration) && cue.duration > 0) nominal += cue.duration
  }
  if (nominal <= 0) return []
  const scale = audioDurationSeconds / nominal
  const spans: VisemeSpan[] = []
  let cursor = 0
  for (const cue of cues) {
    if (!Number.isFinite(cue.duration) || cue.duration <= 0) continue
    cursor += cue.duration * scale
    spans.push({ viseme: cue.viseme, endsAt: cursor, emphasis: cue.emphasis })
  }
  return spans
}

export function visemeAt(
  spans: readonly VisemeSpan[],
  seconds: number,
): VisemeSpan | null {
  if (!spans.length || !Number.isFinite(seconds) || seconds < 0) return null
  for (const span of spans) {
    if (seconds < span.endsAt) return span
  }
  return spans.at(-1) ?? null
}

export function visemeAmount(energy: number, emphasis: boolean): number {
  const amount = emphasis ? energy * 1.18 : energy
  return Math.max(0, Math.min(1, amount))
}

/** Where an utterance's audio is voiced, and where it pauses within (seconds). */
export interface AudioVoicing {
  voiceStart: number
  voiceEnd: number
  pauses: readonly (readonly [number, number])[]
}

/** Loudness is judged over this long. */
const FRAME_SECONDS = 0.01
/** A quiet stretch at least this long is a pause, not the gap within a word. */
const MIN_PAUSE_SECONDS = 0.12
/** A text rest at least this long is a pause to be placed (punctuation), not a gap. */
const TEXT_PAUSE_SECONDS = 0.1
/** What an unplaced pause costs, in shares of the utterance. */
const SKIP_COST = 0.15

/** Voiced start and end, and the pauses between, from the decoded samples. */
export function audioVoicing(samples: Float32Array, sampleRate: number): AudioVoicing | null {
  const step = Math.max(1, Math.round(sampleRate * FRAME_SECONDS))
  const frames: number[] = []
  for (let start = 0; start < samples.length; start += step) {
    let sum = 0
    const end = Math.min(samples.length, start + step)
    for (let i = start; i < end; i += 1) sum += samples[i]! * samples[i]!
    frames.push(Math.sqrt(sum / Math.max(1, end - start)))
  }
  if (frames.length === 0) return null
  const loud = [...frames].sort((a, b) => a - b)[Math.floor(frames.length * 0.9)]!
  const quiet = Math.max(0.004, loud * 0.12)
  const voiced = frames.map((rms) => rms > quiet)
  const first = voiced.indexOf(true)
  const last = voiced.lastIndexOf(true)
  if (first < 0) return null
  const pauses: [number, number][] = []
  let run = -1
  for (let i = first; i <= last + 1; i += 1) {
    if (i <= last && !voiced[i]) {
      if (run < 0) run = i
    } else if (run >= 0) {
      if ((i - run) * FRAME_SECONDS >= MIN_PAUSE_SECONDS) pauses.push([run * FRAME_SECONDS, i * FRAME_SECONDS])
      run = -1
    }
  }
  return { voiceStart: first * FRAME_SECONDS, voiceEnd: (last + 1) * FRAME_SECONDS, pauses }
}

/**
 * The text's mouth shapes laid on the audio as it was spoken: the mouth
 * starts with the voice, stops with it, and the text's pauses (its commas
 * and full stops) fall in the audio's own pauses, in order; between those
 * anchors the shapes are spread evenly. Without voicing, spread over all.
 */
export function alignVisemeTimelineToAudio(
  cues: readonly TextVisemeCue[],
  audioDurationSeconds: number,
  voicing: AudioVoicing | null,
): VisemeSpan[] {
  if (!voicing || !(voicing.voiceEnd > voicing.voiceStart)) return alignVisemeTimeline(cues, audioDurationSeconds)
  const kept = cues.filter((cue) => Number.isFinite(cue.duration) && cue.duration > 0)
  const nominal = kept.reduce((sum, cue) => sum + cue.duration, 0)
  if (nominal <= 0) return []
  // The text's own pauses, where they fall in its nominal time.
  const textPauses: [number, number][] = []
  let at = 0
  for (const cue of kept) {
    if (cue.viseme === 'rest' && cue.duration >= TEXT_PAUSE_SECONDS) textPauses.push([at, at + cue.duration])
    at += cue.duration
  }
  const voiced = voicing.voiceEnd - voicing.voiceStart
  const textShare = (pause: readonly [number, number]) => (pause[0] + pause[1]) / 2 / nominal
  const audioShare = (pause: readonly [number, number]) => ((pause[0] + pause[1]) / 2 - voicing.voiceStart) / voiced
  const matched = matchInOrder(textPauses.map(textShare), voicing.pauses.map(audioShare))
  const anchors: [number, number][] = [[0, voicing.voiceStart]]
  for (const [t, a] of matched) {
    anchors.push([textPauses[t]![0], voicing.pauses[a]![0]], [textPauses[t]![1], voicing.pauses[a]![1]])
  }
  anchors.push([nominal, Math.min(audioDurationSeconds, voicing.voiceEnd)])
  const place = (time: number): number => {
    for (let i = 1; i < anchors.length; i += 1) {
      const [n0, a0] = anchors[i - 1]!
      const [n1, a1] = anchors[i]!
      if (time <= n1) return n1 > n0 ? a0 + ((time - n0) / (n1 - n0)) * (a1 - a0) : a1
    }
    return anchors.at(-1)![1]
  }
  const spans: VisemeSpan[] = []
  if (voicing.voiceStart > 0.03) spans.push({ viseme: 'rest', endsAt: voicing.voiceStart, emphasis: false })
  let cursor = 0
  for (const cue of kept) {
    cursor += cue.duration
    spans.push({ viseme: cue.viseme, endsAt: place(cursor), emphasis: cue.emphasis })
  }
  return spans
}

/** Pairs of (text, audio) pause indices, in order, as close in place as can be. */
function matchInOrder(text: readonly number[], audio: readonly number[]): [number, number][] {
  const rows = text.length + 1
  const cols = audio.length + 1
  const cost: number[][] = Array.from({ length: rows }, () => Array.from({ length: cols }, () => 0))
  for (let i = 1; i < rows; i += 1) cost[i]![0] = i * SKIP_COST
  for (let j = 1; j < cols; j += 1) cost[0]![j] = j * SKIP_COST
  for (let i = 1; i < rows; i += 1) {
    for (let j = 1; j < cols; j += 1) {
      cost[i]![j] = Math.min(
        cost[i - 1]![j - 1]! + Math.abs(text[i - 1]! - audio[j - 1]!),
        cost[i - 1]![j]! + SKIP_COST,
        cost[i]![j - 1]! + SKIP_COST,
      )
    }
  }
  const pairs: [number, number][] = []
  let i = text.length
  let j = audio.length
  while (i > 0 && j > 0) {
    if (cost[i]![j] === cost[i - 1]![j - 1]! + Math.abs(text[i - 1]! - audio[j - 1]!)) {
      pairs.unshift([i - 1, j - 1])
      i -= 1
      j -= 1
    } else if (cost[i]![j] === cost[i - 1]![j]! + SKIP_COST) {
      i -= 1
    } else {
      j -= 1
    }
  }
  return pairs
}

/**
 * Where a place in the text (a code unit offset) is said, in seconds of its
 * audio: the text starts with the voice and ends with it, and its pauses
 * (punctuation) fall in the audio's pauses, in order; between those anchors
 * by its share of the characters. Null without voicing.
 */
export function textTimeOnAudio(
  text: string,
  voicing: AudioVoicing | null,
): ((textOffset: number) => number) | null {
  if (!voicing || !(voicing.voiceEnd > voicing.voiceStart) || !text) return null
  const pauses: [number, number][] = []
  let offset = 0
  let run: number | null = null
  for (const symbol of text) {
    const pause = visualSpeechPauseSeconds(symbol)
    if (pause !== null && pause >= TEXT_PAUSE_SECONDS) {
      run ??= offset
    } else if (run !== null) {
      // A pause before the first word or after the last is not between words.
      if (run > 0) pauses.push([run, offset])
      run = null
    }
    offset += symbol.length
  }
  const length = text.length
  const voiced = voicing.voiceEnd - voicing.voiceStart
  const matched = matchInOrder(
    pauses.map(([start, end]) => (start + end) / 2 / length),
    voicing.pauses.map(([start, end]) => ((start + end) / 2 - voicing.voiceStart) / voiced),
  )
  const anchors: [number, number][] = [[0, voicing.voiceStart]]
  for (const [t, a] of matched) {
    anchors.push([pauses[t]![0], voicing.pauses[a]![0]], [pauses[t]![1], voicing.pauses[a]![1]])
  }
  anchors.push([length, voicing.voiceEnd])
  return (textOffset: number) => {
    const at = Math.max(0, Math.min(length, textOffset))
    for (let i = 1; i < anchors.length; i += 1) {
      const [t0, a0] = anchors[i - 1]!
      const [t1, a1] = anchors[i]!
      if (at <= t1) return t1 > t0 ? a0 + ((at - t0) / (t1 - t0)) * (a1 - a0) : a1
    }
    return voicing.voiceEnd
  }
}
