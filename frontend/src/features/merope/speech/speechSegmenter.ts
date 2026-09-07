import type { MeropeSpeechSource } from '../speechEvents'
import { getDefaultLocale } from '../../../i18n'
import { speakableText } from './speakableText'

export type SpeechInterruptMode = 'queue' | 'replace' | 'interrupt'

export interface SpeechSegment {
  segmentId: string
  sequence: number
  text: string
  messageId: string
  generation: number
  source?: MeropeSpeechSource
  interrupt: SpeechInterruptMode
  /**
   * UI language at the time the reply was written. Han characters alone cannot
   * say whether a line is Chinese or Japanese, and reading Japanese kanji as
   * pinyin gives the wrong mouth for the whole sentence.
   */
  locale?: string
}

const CJK_SENTENCE_END = /[。！？!?…]/
const MAX_CHARS = 120
const MIN_CHARS = 4

/**
 * Accumulates Lite tokens and emits stable speakable sentences.
 * A segment is ready at a sentence end, a newline, or the max length.
 */
export class SpeechSegmenter {
  private raw = ''
  private spokenOffset = 0
  private sequence = 0
  private readonly messageId: string
  private readonly generation: number
  private readonly locale: string

  constructor(
    messageId: string,
    generation = 0,
    locale = getDefaultLocale(),
    private readonly source: MeropeSpeechSource = 'reply',
  ) {
    this.messageId = messageId
    this.generation = generation
    this.locale = locale
  }

  push(
    token: string,
    interrupt: SpeechInterruptMode = 'queue',
  ): SpeechSegment[] {
    this.raw += token
    return this.flush(false, interrupt)
  }

  end(interrupt: SpeechInterruptMode = 'queue'): SpeechSegment[] {
    return this.flush(true, interrupt)
  }

  private flush(
    force: boolean,
    interrupt: SpeechInterruptMode,
  ): SpeechSegment[] {
    const spoken = speakableText(
      this.raw.slice(0, stableRawEnd(this.raw, force)),
    )
    const sealed = force || blockFenceOpen(this.raw)
    while (
      this.spokenOffset < spoken.length &&
      /\s/.test(spoken[this.spokenOffset]!)
    ) {
      this.spokenOffset += 1
    }
    const remaining = spoken.slice(this.spokenOffset)
    const cut = nextCut(remaining, sealed)
    if (cut <= 0) {
      if (force) {
        this.raw = ''
        this.spokenOffset = 0
      }
      return []
    }
    const text = remaining.slice(0, cut).trim()
    this.spokenOffset += cut
    if (!text) return force ? this.flush(true, interrupt) : []
    this.sequence += 1
    const segment: SpeechSegment = {
      segmentId: `${this.messageId}:${this.sequence}`,
      sequence: this.sequence,
      text,
      messageId: this.messageId,
      generation: this.generation,
      source: this.source,
      interrupt,
      ...(this.locale ? { locale: this.locale } : {}),
    }
    const more = this.flush(force, force ? 'queue' : interrupt)
    if (more.length > 0) return [segment, ...more]
    return [segment]
  }
}

/**
 * End of the prefix whose speakable form can no longer change.
 *
 * `speakableText` only recognises a construct once it is closed, so an
 * unterminated fence reads as plain text and would be spoken; when the closing
 * marker finally arrives the whole block collapses and the speakable string
 * gets *shorter*, invalidating `spokenOffset`. Holding back from the opener
 * keeps that string monotonic, which is what the offset assumes.
 *
 * On `force` the message is over, so only unclosed code stays unspoken; a
 * half-written link or tag is prose and is read as-is.
 */
function stableRawEnd(raw: string, force: boolean): number {
  let end = raw.length
  const code = openCode(raw)
  if (code.fence >= 0) end = code.fence
  else if (code.tick >= 0) end = code.tick
  if (force) return end
  const prose = openProseStart(raw)
  if (prose >= 0 && prose < end) end = prose
  return end
}

/**
 * True when a fence opened on its own line is still unclosed. Everything
 * before it is finished prose, so it may be cut even without a sentence end --
 * otherwise a long code block would hold the preceding line until `end()`.
 * Inline openers get no such seal: cutting there would split one sentence
 * across two utterances.
 */
function blockFenceOpen(raw: string): boolean {
  const fence = openCode(raw).fence
  return fence === 0 || (fence > 0 && raw[fence - 1] === '\n')
}

/** Start of the unterminated fence and inline code span, each -1 when closed. */
function openCode(raw: string): { fence: number; tick: number } {
  let index = 0
  let fence = -1
  let tick = -1
  while (index < raw.length) {
    if (raw.startsWith('```', index)) {
      fence = fence >= 0 ? -1 : index
      index += 3
      continue
    }
    if (fence >= 0) {
      index += 1
      continue
    }
    if (raw[index] === '`') tick = tick >= 0 ? -1 : index
    index += 1
  }
  return { fence, tick }
}

/** Start of an unterminated link, HTML tag, or URL, or -1. */
function openProseStart(raw: string): number {
  let end = -1
  const link = openLinkStart(raw)
  if (link >= 0) end = link
  const tag = openTagStart(raw)
  if (tag >= 0 && (end < 0 || tag < end)) end = tag
  const url = TRAILING_URL.exec(raw)?.index ?? -1
  if (url >= 0 && (end < 0 || url < end)) end = url
  return end
}

/** A trailing token that may still grow into a link `speakableText` strips. */
const TRAILING_URL = /(?:\bwww|\bhttp)\S*$/i

function openLinkStart(raw: string): number {
  for (let i = 0; i < raw.length; i++) {
    if (raw[i] !== '[') continue
    const close = raw.indexOf(']', i)
    if (close < 0 || close + 1 >= raw.length) return i
    if (raw[close + 1] !== '(') {
      i = close
      continue
    }
    const paren = raw.indexOf(')', close)
    if (paren < 0) return i
    i = paren
  }
  return -1
}

function openTagStart(raw: string): number {
  const at = raw.lastIndexOf('<')
  if (at < 0 || raw.includes('>', at)) return -1
  const next = raw[at + 1]
  if (next && !/[a-z/]/i.test(next)) return -1
  return at
}

function nextCut(text: string, force: boolean): number {
  if (!text) return 0
  if (text.length >= MAX_CHARS) {
    const window = text.slice(0, MAX_CHARS)
    const end = lastSentenceEnd(window)
    if (end >= MIN_CHARS) return end
    const space = window.lastIndexOf(' ')
    return space >= MIN_CHARS ? space : MAX_CHARS
  }
  const end = firstSentenceEndAtLeast(text, MIN_CHARS)
  if (end >= MIN_CHARS) return end
  const newline = firstNewlineAtLeast(text, MIN_CHARS)
  if (newline >= MIN_CHARS) return newline
  return force ? text.length : 0
}

function firstSentenceEndAtLeast(text: string, minChars: number): number {
  for (let i = 0; i < text.length; i++) {
    const end = sentenceEndAfter(text, i)
    if (end >= minChars) return end
  }
  return -1
}

function firstNewlineAtLeast(text: string, minChars: number): number {
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '\n' && i + 1 >= minChars) return i + 1
  }
  return -1
}

function lastSentenceEnd(text: string): number {
  for (let i = text.length - 1; i >= 0; i--) {
    const end = sentenceEndAfter(text, i)
    if (end > 0) return end
  }
  return -1
}

const PERIOD_ABBREVIATIONS = new Set([
  'mr',
  'mrs',
  'ms',
  'dr',
  'prof',
  'st',
  'jr',
  'sr',
  'vs',
  'inc',
  'ltd',
  'fig',
  'no',
])

/** Exclusive end index, or -1. ASCII `.` needs a following space and is not a decimal. */
function sentenceEndAfter(text: string, index: number): number {
  const ch = text[index]
  if (!ch) return -1
  if (CJK_SENTENCE_END.test(ch)) {
    let end = index + 1
    while (end < text.length && /[”’"')\]]/.test(text[end]!)) end += 1
    return end
  }
  if (ch !== '.') return -1
  const prev = text[index - 1]
  const next = text[index + 1]
  if (prev && /\d/.test(prev) && next && /\d/.test(next)) return -1
  if (next && !/\s/.test(next) && !/[”’"')\]]/.test(next)) return -1
  let end = index + 1
  while (end < text.length && /[”’"')\]]/.test(text[end]!)) end += 1
  if (isPeriodAbbreviation(text, index)) return -1
  const first = text.slice(end).match(/\S/u)?.[0]
  if (first && !looksLikeSentenceStart(first)) return -1
  return end
}

function isPeriodAbbreviation(text: string, periodIndex: number): boolean {
  let start = periodIndex - 1
  while (start >= 0 && /[A-Z]/i.test(text[start]!)) start -= 1
  const word = text.slice(start + 1, periodIndex)
  return PERIOD_ABBREVIATIONS.has(word.toLowerCase())
}

function looksLikeSentenceStart(ch: string): boolean {
  return (
    /\p{Lu}/u.test(ch) ||
    /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u.test(ch)
  )
}
