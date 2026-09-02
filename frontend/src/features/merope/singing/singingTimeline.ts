import type { LyricLine, WordLyricLine } from '../../../utils/musicPlayer'
import type { TextVisemeCue } from '../anime25drig/textVisemes'
import type { SpeechViseme } from '../rig/articulation'
import { compileTextVisemes } from '../anime25drig/textVisemes'

export interface SingingCue {
  start: number
  end: number
  viseme: SpeechViseme
  emphasis: boolean
}

export interface SingingTimelineInput {
  verbatim?: readonly WordLyricLine[]
  lines?: readonly LyricLine[]
  locale?: string
  songDuration?: number
}

const MAX_LINE_HOLD_SECONDS = 6
const FALLBACK_TOKEN_SECONDS = 0.08
const LAST_LINE_HOLD_SECONDS = 2.4

export async function compileSingingTimeline(
  input: SingingTimelineInput,
): Promise<SingingCue[]> {
  const locale = input.locale
  if (input.verbatim && input.verbatim.length > 0) {
    return coalesceCues(await compileVerbatim(input.verbatim, locale))
  }
  if (input.lines && input.lines.length > 0) {
    return coalesceCues(
      await compileLines(input.lines, locale, input.songDuration),
    )
  }
  return []
}

async function compileVerbatim(
  lines: readonly WordLyricLine[],
  locale?: string,
): Promise<SingingCue[]> {
  const cues: SingingCue[] = []
  for (const line of lines) {
    const words = line.words ?? []
    for (let index = 0; index < words.length; index += 1) {
      const token = words[index]
      const text = token.text.trim()
      if (!text) continue
      const next = words[index + 1]
      const end = tokenEnd(token.time, token.duration, next?.time)
      if (end <= token.time) continue
      cues.push(
        ...(await stretchText(text, token.time, end - token.time, locale)),
      )
    }
  }
  return cues
}

async function compileLines(
  lines: readonly LyricLine[],
  locale: string | undefined,
  songDuration: number | undefined,
): Promise<SingingCue[]> {
  const cues: SingingCue[] = []
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    const text = line.text.trim()
    if (!text) continue
    const start = finiteTime(line.time)
    const nextStart = lines[index + 1]
      ? finiteTime(lines[index + 1].time)
      : Number.NaN
    const hold = Number.isFinite(nextStart)
      ? Math.min(MAX_LINE_HOLD_SECONDS, Math.max(0, nextStart - start))
      : lastLineHold(start, songDuration)
    if (hold <= 0.02) continue
    cues.push(...(await stretchText(text, start, hold, locale)))
  }
  return cues
}

function lastLineHold(start: number, songDuration: number | undefined): number {
  if (
    typeof songDuration === 'number' &&
    Number.isFinite(songDuration) &&
    songDuration > start
  ) {
    return Math.min(MAX_LINE_HOLD_SECONDS, songDuration - start)
  }
  return LAST_LINE_HOLD_SECONDS
}

async function stretchText(
  text: string,
  start: number,
  duration: number,
  locale?: string,
): Promise<SingingCue[]> {
  const compiled = await compileTextVisemes(text, locale)
  return stretchCues(compiled, start, duration)
}

export function stretchCues(
  cues: readonly TextVisemeCue[],
  start: number,
  duration: number,
): SingingCue[] {
  if (!(duration > 0) || !Number.isFinite(start)) return []
  const voiced = cues.filter((cue) => cue.duration > 0)
  const total = voiced.reduce((sum, cue) => sum + cue.duration, 0)
  if (voiced.length === 0 || total <= 0) return []
  const scale = duration / total
  const output: SingingCue[] = []
  let cursor = start
  for (const cue of voiced) {
    const end = cursor + cue.duration * scale
    output.push({
      start: cursor,
      end,
      viseme: cue.viseme,
      emphasis: cue.emphasis,
    })
    cursor = end
  }
  return output
}

export function coalesceCues(cues: readonly SingingCue[]): SingingCue[] {
  const sorted = cues
    .filter((cue) => cue.end > cue.start && Number.isFinite(cue.start))
    .sort((left, right) => left.start - right.start)
  const output: SingingCue[] = []
  for (const cue of sorted) {
    const previous = output[output.length - 1]
    if (
      previous &&
      previous.viseme === cue.viseme &&
      cue.start <= previous.end + 0.01
    ) {
      previous.end = Math.max(previous.end, cue.end)
      previous.emphasis ||= cue.emphasis
      continue
    }
    output.push({ ...cue })
  }
  return output
}

function tokenEnd(
  start: number,
  duration: number,
  nextStart: number | undefined,
): number {
  const origin = finiteTime(start)
  if (duration > 0.01 && Number.isFinite(duration)) return origin + duration
  if (
    typeof nextStart === 'number' &&
    Number.isFinite(nextStart) &&
    nextStart > origin
  ) {
    return nextStart
  }
  return origin + FALLBACK_TOKEN_SECONDS
}

function finiteTime(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0
}
