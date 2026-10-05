import type { BodyPose, ScoreBeat, ScoreMove } from '../../../services/agent/types'
import type { SpeechAccentAnchor } from '../speech/prosody'

/** A beat placed on the clock (`performance.now()` milliseconds). */
export interface ResolvedScoreBeat {
  id: string
  atMs: number
  pose?: BodyPose
  move?: ScoreMove
}

/** The beats of the current score that have found their moment so far. */
export interface ResolvedScore {
  id: number
  beats: readonly ResolvedScoreBeat[]
}

/** One spoken stretch of a reply, as the speech pipeline times it. */
export interface SpokenUtterance {
  messageKey: string
  utteranceId: string
  text: string
  startedAtMs: number
  /** Zero while the audio length is not known yet. */
  durationMs: number
  accents: readonly SpeechAccentAnchor[]
}

/** A beat this close to its moment is committed: new timing does not move it. */
const COMMIT_MS = 250
/** A beat whose words were said this long ago is past, not late. */
const LATE_MS = 150
/** Speaking pace when nothing better is known, per character. */
const DEFAULT_MS_PER_CHAR = 200
const MAX_UTTERANCES = 12

interface Placed {
  utteranceId: string
  textOffset: number
}

interface Score {
  id: number
  messageKey: string | null
  receivedAtMs: number
  beats: readonly ScoreBeat[]
  placed: Map<number, Placed>
  resolved: Map<number, ResolvedScoreBeat>
  /** Words already used, per utterance: the next beat on the same words is a later saying. */
  cursors: Map<string, number>
}

/**
 * Places the director's beats on the clock. A beat on time is placed when the
 * score arrives; a beat on words is placed when the speech pipeline says
 * those words, interpolated between the utterance's timed accents, and
 * re-timed with better timing until it is about to start.
 */
export class ScoreTimeline {
  private score: Score | null = null
  private nextId = 1
  private readonly utterances: SpokenUtterance[] = []
  private published: ResolvedScore | null = null

  set(beats: readonly ScoreBeat[], messageKey: string | null, receivedAtMs: number): void {
    if (beats.length === 0) return
    const score: Score = {
      id: this.nextId++,
      messageKey,
      receivedAtMs,
      beats,
      placed: new Map(),
      resolved: new Map(),
      cursors: new Map(),
    }
    beats.forEach((beat, index) => {
      if (beat.text === undefined) {
        score.resolved.set(index, resolvedBeat(score.id, index, beat, receivedAtMs + beat.atMs))
      }
    })
    this.score = score
    for (const utterance of this.utterances) {
      if (utterance.messageKey === messageKey) this.place(utterance, receivedAtMs)
    }
    this.publish()
  }

  /** The speech pipeline's latest timing for an utterance; true when the score changed. */
  noteUtterance(utterance: SpokenUtterance, nowMs: number): boolean {
    const known = this.utterances.findIndex((item) => item.utteranceId === utterance.utteranceId)
    if (known >= 0) this.utterances.splice(known, 1)
    this.utterances.push(utterance)
    while (this.utterances.length > MAX_UTTERANCES) this.utterances.shift()
    if (!this.score || this.score.messageKey !== utterance.messageKey) return false
    const before = this.published
    this.place(utterance, nowMs)
    this.publish()
    return this.published !== before
  }

  clear(): void {
    this.score = null
    this.publish()
  }

  current(): ResolvedScore | null {
    return this.published
  }

  private place(utterance: SpokenUtterance, nowMs: number): void {
    const score = this.score
    if (!score) return
    const text = utterance.text.normalize('NFKC')
    // Re-time what this utterance already placed, unless it is about to start.
    for (const [index, placed] of score.placed) {
      if (placed.utteranceId !== utterance.utteranceId) continue
      const current = score.resolved.get(index)
      if (current && current.atMs - nowMs <= COMMIT_MS) continue
      const beat = score.beats[index]!
      const atMs = utterance.startedAtMs + timeAtText(utterance, text, placed.textOffset) + beat.offsetMs
      if (current && Math.abs(current.atMs - atMs) < 1) continue
      score.resolved.set(index, resolvedBeat(score.id, index, beat, atMs))
    }
    // Then place the beats on words not yet placed, in order, from where the last one was found.
    let cursor = score.cursors.get(utterance.utteranceId) ?? 0
    score.beats.forEach((beat, index) => {
      if (beat.text === undefined || score.placed.has(index)) return
      const found = text.indexOf(beat.text, cursor)
      if (found < 0) return
      cursor = found + 1
      const atMs = utterance.startedAtMs + timeAtText(utterance, text, found) + beat.offsetMs
      score.placed.set(index, { utteranceId: utterance.utteranceId, textOffset: found })
      if (atMs < nowMs - LATE_MS) return
      score.resolved.set(index, resolvedBeat(score.id, index, beat, atMs))
    })
    score.cursors.set(utterance.utteranceId, cursor)
  }

  private publish(): void {
    const score = this.score
    if (!score) {
      this.published = null
      return
    }
    const beats = [...score.resolved.entries()]
      .sort(([a], [b]) => a - b)
      .map(([, beat]) => beat)
    const previous = this.published
    if (
      previous?.id === score.id &&
      previous.beats.length === beats.length &&
      previous.beats.every((beat, index) => beat === beats[index])
    ) {
      return
    }
    this.published = { id: score.id, beats }
  }
}

function resolvedBeat(scoreId: number, index: number, beat: ScoreBeat, atMs: number): ResolvedScoreBeat {
  return {
    id: `${scoreId}:${index}`,
    atMs,
    ...(beat.pose ? { pose: beat.pose } : {}),
    ...(beat.move ? { move: beat.move } : {}),
  }
}

/**
 * When the words at `offset` are said, in milliseconds into the utterance:
 * between the timed accents around them, or at the measured pace beyond them.
 */
export function timeAtText(utterance: Readonly<SpokenUtterance>, text: string, offset: number): number {
  const points: Array<[number, number]> = [[0, 0]]
  for (const accent of utterance.accents) {
    if (accent.textOffset === undefined) continue
    if (accent.textOffset > points.at(-1)![0] && accent.offsetMs >= points.at(-1)![1]) {
      points.push([accent.textOffset, accent.offsetMs])
    }
  }
  if (utterance.durationMs > 0 && text.length > points.at(-1)![0]) {
    points.push([text.length, utterance.durationMs])
  }
  for (let i = 1; i < points.length; i++) {
    const [x1, y1] = points[i]!
    if (offset > x1) continue
    const [x0, y0] = points[i - 1]!
    return y0 + ((offset - x0) / Math.max(1, x1 - x0)) * (y1 - y0)
  }
  const [x0, y0] = points.at(-1)!
  const pace = points.length > 1 && x0 > 0 ? y0 / x0 : DEFAULT_MS_PER_CHAR
  return y0 + (offset - x0) * pace
}
