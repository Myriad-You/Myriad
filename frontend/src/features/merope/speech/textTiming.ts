export const MAX_VISUAL_SPEECH_TEXT_UNITS = 2_000
const MIN_VISUAL_SPEECH_MS = 650

export const VISUAL_SPEECH_WORD_GAP_SECONDS = 0.05
export const VISUAL_SPEECH_MINOR_PAUSE_SECONDS = 0.18
export const VISUAL_SPEECH_MAJOR_PAUSE_SECONDS = 0.32
export const VISUAL_SPEECH_HESITATION_SECONDS = 0.24

/**
 * Visual speech runs slower than the voice it stands in for. With no audio the
 * mouth is the only cue, and articulating at true speaking rate reads as
 * chattering rather than talking. Pauses keep their natural length — only
 * articulation stretches — so phrasing stays recognizable.
 */
export const VISUAL_SPEECH_ARTICULATION_SCALE = 1.28

const CJK_UNIT =
  /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u
const LATIN_OR_NUMBER_RUN = /[\p{Script=Latin}\p{Number}]+/gu

/**
 * Shared phrasing for visual-only speech. Text visemes and the lifecycle tail
 * must agree about silence, otherwise the mouth either races the sentence or
 * gets cut off before its final pause.
 */
export function visualSpeechPauseSeconds(symbol: string): number | null {
  if (/[\r\n]/u.test(symbol)) return VISUAL_SPEECH_MAJOR_PAUSE_SECONDS
  if (/\s/u.test(symbol)) return VISUAL_SPEECH_WORD_GAP_SECONDS
  if (/[。.!！？?]/u.test(symbol)) return VISUAL_SPEECH_MAJOR_PAUSE_SECONDS
  if (/[，、,;；:：]/u.test(symbol)) return VISUAL_SPEECH_MINOR_PAUSE_SECONDS
  if (/[…—–-]/u.test(symbol)) return VISUAL_SPEECH_HESITATION_SECONDS
  return null
}

export function isMajorVisualSpeechPause(durationSeconds: number): boolean {
  return durationSeconds >= VISUAL_SPEECH_MAJOR_PAUSE_SECONDS - 0.001
}

/**
 * Body expression stays present between words, softens at commas, and rests
 * completely at a sentence boundary.
 */
export function visualSpeechPauseActivity(durationSeconds: number): number {
  if (isMajorVisualSpeechPause(durationSeconds)) return 0
  if (durationSeconds >= VISUAL_SPEECH_HESITATION_SECONDS - 0.001) return 0.08
  if (durationSeconds >= VISUAL_SPEECH_MINOR_PAUSE_SECONDS - 0.001) return 0.18
  return 0.58
}

/**
 * Synchronous duration estimate for speech without audio. It follows the same
 * pause vocabulary as the viseme compiler and intentionally models a calm
 * conversational cadence rather than token arrival speed.
 */
export function estimateVisualSpeechDurationMs(
  text: string,
  locale?: string,
): number {
  return estimateSpeechMs(text, locale, 1)
}

/** The mouth may realize a slower pace; lifecycle safety is not a beat clock. */
export function estimateVisualSpeechTailMs(
  text: string,
  locale?: string,
): number {
  return estimateSpeechMs(text, locale, 1.32)
}

function estimateSpeechMs(
  text: string,
  locale: string | undefined,
  pace: number,
): number {
  const clock = visualSpeechPrefixSeconds(text, locale)
  // The text budget bounds this clock. Transport timeouts must not clamp it.
  return Math.round(
    Math.max(clock.at(-1)! * 1_000 * pace, MIN_VISUAL_SPEECH_MS),
  )
}

/** Build once per text update, then address any normalized UTF-16 boundary. */
export function visualSpeechPrefixMs(
  text: string,
  locale?: string,
): readonly number[] {
  return visualSpeechPrefixSeconds(text, locale).map((seconds) =>
    Math.round(Math.max(seconds * 1_000, MIN_VISUAL_SPEECH_MS)),
  )
}

function visualSpeechPrefixSeconds(text: string, locale?: string): number[] {
  const bounded = text.normalize('NFKC').slice(0, MAX_VISUAL_SPEECH_TEXT_UNITS)
  const language = locale?.toLowerCase() || ''
  let seconds = 0.22
  let cursor = 0
  const clock = [seconds]
  const appendSymbols = (symbols: string) => {
    for (const symbol of symbols) {
      // An incomplete surrogate has no spoken duration of its own.
      for (let unit = 1; unit < symbol.length; unit++) clock.push(seconds)
      seconds += estimateSymbols(symbol, language)
      clock.push(seconds)
    }
  }

  for (const match of bounded.matchAll(LATIN_OR_NUMBER_RUN)) {
    const index = match.index ?? 0
    appendSymbols(bounded.slice(cursor, index))
    const token = match[0]
    let numeric = true
    let length = 0
    for (const symbol of token) {
      for (let unit = 1; unit < symbol.length; unit++) {
        clock.push(seconds + tokenSeconds(length + unit, false))
      }
      length += symbol.length
      numeric &&= /^\p{Number}$/u.test(symbol)
      clock.push(seconds + tokenSeconds(length, numeric))
    }
    seconds = clock.at(-1)!
    cursor = index + token.length
  }
  appendSymbols(bounded.slice(cursor))
  return clock
}

function tokenSeconds(length: number, numeric: boolean): number {
  return (
    (numeric
      ? Math.min(0.9, Math.max(0.16, length * 0.15))
      : Math.min(0.78, Math.max(0.23, 0.17 + length * 0.052))) *
    VISUAL_SPEECH_ARTICULATION_SCALE
  )
}

function estimateSymbols(text: string, language: string): number {
  let seconds = 0
  for (const symbol of text) {
    const pause = visualSpeechPauseSeconds(symbol)
    if (pause !== null) {
      seconds += pause
    } else if (CJK_UNIT.test(symbol)) {
      if (
        language.startsWith('ja') ||
        /[\p{Script=Hiragana}\p{Script=Katakana}]/u.test(symbol)
      ) {
        seconds += 0.135 * VISUAL_SPEECH_ARTICULATION_SCALE
      } else if (
        language.startsWith('ko') ||
        /\p{Script=Hangul}/u.test(symbol)
      ) {
        seconds += 0.165 * VISUAL_SPEECH_ARTICULATION_SCALE
      } else {
        // A rounded initial (w) uses 0.06s + a 0.145s final in the compiler.
        seconds += 0.205 * VISUAL_SPEECH_ARTICULATION_SCALE
      }
    }
  }
  return seconds
}
