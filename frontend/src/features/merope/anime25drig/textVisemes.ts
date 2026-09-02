import type { SpeechViseme } from '../rig/articulation'
import {
  VISUAL_SPEECH_ARTICULATION_SCALE,
  visualSpeechPauseSeconds,
} from '../speech/textTiming'

export interface TextVisemeCue {
  viseme: SpeechViseme
  duration: number
  emphasis: boolean
}

const MAX_COMPILED_CUES = 192
const HAN_RUN = /\p{Script=Han}+/gu
const HAN_CHAR = /\p{Script=Han}/u
const JAPANESE_CHAR = /[\p{Script=Hiragana}\p{Script=Katakana}ー]/u
const LATIN_RUN = /[a-z]+(?:['’][a-z]+)*/gi

/**
 * Converts streamed display text to a compact visual-only speech timeline.
 * It deliberately does no audio work and is only called when text chunks arrive.
 */
export async function compileTextVisemes(
  text: string,
  locale?: string,
): Promise<TextVisemeCue[]> {
  const normalized = text.normalize('NFKC').slice(0, 2_000)
  const output: TextVisemeCue[] = []
  const language = locale?.toLowerCase() || ''
  if (language.startsWith('ja')) {
    compileNonHan(normalized, language, output)
    return coalesce(output).slice(0, MAX_COMPILED_CUES)
  }
  let pinyin: (typeof import('pinyin-pro'))['pinyin'] | null = null
  if (HAN_CHAR.test(normalized)) {
    try {
      pinyin = (await import('pinyin-pro')).pinyin
    } catch {
      pinyin = null
    }
  }
  let cursor = 0
  for (const match of normalized.matchAll(HAN_RUN)) {
    const index = match.index ?? 0
    compileNonHan(normalized.slice(cursor, index), language, output)
    if (pinyin) {
      try {
        compileHan(match[0], output, pinyin)
      } catch {
        compileUnknownHan(match[0], output)
      }
    } else {
      compileUnknownHan(match[0], output)
    }
    cursor = index + match[0].length
  }
  compileNonHan(normalized.slice(cursor), language, output)
  return coalesce(output).slice(0, MAX_COMPILED_CUES)
}

function compileUnknownHan(text: string, output: TextVisemeCue[]): void {
  const options: SpeechViseme[] = ['narrow', 'open', 'wide', 'round']
  for (const symbol of text) {
    const viseme = options[symbol.codePointAt(0)! % options.length]
    push(output, viseme, 0.195, viseme === 'open')
  }
}

function compileHan(
  text: string,
  output: TextVisemeCue[],
  pinyin: (typeof import('pinyin-pro'))['pinyin'],
): void {
  const syllables = pinyin(text, {
    toneType: 'none',
    type: 'array',
  }) as string[]
  for (const raw of syllables) {
    const syllable = raw.toLowerCase().replace(/[^a-zü]/g, '')
    if (!syllable) continue
    const initial = syllable.match(/^(?:[csz]h|[b-df-hj-np-tw-z])/)?.[0]
    if (initial && /^[bpm]$/.test(initial)) {
      push(output, 'closed', 0.05, false)
    } else if (initial === 'w') {
      push(output, 'round', 0.06, false)
    } else if (initial) {
      push(output, 'narrow', 0.05, false)
    }
    const final = syllable.slice(initial?.length || 0) || syllable
    const apical = final === 'i' && /^(?:[csz]h|[crsz])$/.test(initial || '')
    const finalViseme = apical ? 'narrow' : chineseFinalViseme(final)
    push(output, finalViseme, initial ? 0.145 : 0.195, isOpenFinal(final))
  }
}

function chineseFinalViseme(final: string): SpeechViseme {
  if (/[ouüv]/.test(final)) return 'round'
  if (/[ei]/.test(final)) return 'wide'
  if (/a/.test(final)) return 'open'
  return 'narrow'
}

function isOpenFinal(final: string): boolean {
  return /a/.test(final)
}

function compileNonHan(
  text: string,
  language: string,
  output: TextVisemeCue[],
): void {
  let latinStart = 0
  for (const match of text.matchAll(LATIN_RUN)) {
    const index = match.index ?? 0
    compileSymbols(text.slice(latinStart, index), language, output)
    compileLatinWord(match[0], output)
    latinStart = index + match[0].length
  }
  compileSymbols(text.slice(latinStart), language, output)
}

function compileSymbols(
  text: string,
  language: string,
  output: TextVisemeCue[],
): void {
  const symbols = Array.from(text)
  for (let index = 0; index < symbols.length; index += 1) {
    const symbol = symbols[index]
    if (
      JAPANESE_CHAR.test(symbol) ||
      (language.startsWith('ja') && /[\p{Script=Han}ヶヵ]/u.test(symbol))
    ) {
      if (isJapaneseLabial(symbol)) push(output, 'closed', 0.04, false)
      const cue = japaneseCue(symbol, index > 0 ? symbols[index - 1] : '')
      if (cue) push(output, cue, 0.135, cue === 'open')
      continue
    }
    const pause = visualSpeechPauseSeconds(symbol)
    if (pause !== null) {
      push(output, 'rest', pause, false)
    } else if (/\p{Number}/u.test(symbol)) {
      const options: SpeechViseme[] = ['narrow', 'wide', 'open', 'round']
      const viseme = options[symbol.codePointAt(0)! % options.length]
      push(output, viseme, 0.15, viseme === 'open')
    }
  }
}

function japaneseCue(symbol: string, previous: string): SpeechViseme | null {
  if (symbol === 'ー') return previous ? japaneseCue(previous, '') : 'narrow'
  if (/[っッ]/u.test(symbol)) return 'closed'
  if (
    /[うおこごそぞとどのほぼぽもよろをぐすずつづぬふぶぷむゆるウオコゴソゾトドノホボポモヨロヲグスズツヅヌフブプムユル]/u.test(
      symbol,
    )
  ) {
    return 'round'
  }
  if (
    /[いえきぎけげしじせぜちぢてでにねひびぴへべぺみめりれゐゑイエキギケゲシジセゼチヂテデニネヒビピヘベペミメリレヰヱ]/u.test(
      symbol,
    )
  ) {
    return 'wide'
  }
  if (
    /[あかがさざただなはばぱまやらわぁゃアカガサザタダナハバパマヤラワァャ]/u.test(
      symbol,
    )
  ) {
    return 'open'
  }
  if (/\p{Script=Han}/u.test(symbol)) {
    const options: SpeechViseme[] = ['narrow', 'open', 'wide', 'round']
    return options[symbol.codePointAt(0)! % options.length]
  }
  return null
}

function isJapaneseLabial(symbol: string): boolean {
  return /[まみむめもばびぶべぼぱぴぷぺぽマミムメモバビブベボパピプペポ]/u.test(
    symbol,
  )
}

function compileLatinWord(word: string, output: TextVisemeCue[]): void {
  const lower = word.toLowerCase()
  let index = 0
  let firstVowel = true
  while (index < lower.length) {
    const rest = lower.slice(index)
    const pair = rest.slice(0, 2)
    if (/^(?:th|sh|ch|zh)/.test(rest)) {
      push(output, 'narrow', 0.055, false)
      index += 2
    } else if (rest.startsWith('j')) {
      push(output, 'narrow', 0.055, false)
      index += 1
    } else if (/^[bmp]/.test(rest)) {
      push(output, 'closed', 0.05, false)
      index += 1
    } else if (/^(?:oo|ou|ow|oa|or)/.test(rest)) {
      push(output, 'round', 0.13, firstVowel)
      firstVowel = false
      index += 2
    } else if (/^(?:ee|ea|ie|ei|ey)/.test(rest)) {
      push(output, 'wide', 0.12, firstVowel)
      firstVowel = false
      index += 2
    } else if (/^(?:ai|ay|au|aw)/.test(rest)) {
      push(output, 'open', 0.13, firstVowel)
      firstVowel = false
      index += 2
    } else if (/^[ou]/.test(rest)) {
      push(output, 'round', 0.11, firstVowel)
      firstVowel = false
      index += 1
    } else if (/^[eiy]/.test(rest)) {
      push(output, 'wide', 0.105, firstVowel)
      firstVowel = false
      index += 1
    } else if (rest.startsWith('a')) {
      push(output, 'open', 0.12, firstVowel)
      firstVowel = false
      index += 1
    } else if (/^[fv]/.test(rest)) {
      push(output, 'narrow', 0.06, false)
      index += 1
    } else if (/^[wq]/.test(rest)) {
      push(output, 'round', 0.06, false)
      index += 1
    } else {
      push(output, 'narrow', 0.05, false)
      index += pair.length > 0 ? 1 : lower.length
    }
  }
  push(output, 'rest', 0.035, false)
}

function push(
  output: TextVisemeCue[],
  viseme: SpeechViseme,
  duration: number,
  emphasis: boolean,
): void {
  // `rest` carries an authored pause; everything else is articulation.
  output.push({
    viseme,
    duration:
      viseme === 'rest' ? duration : duration * VISUAL_SPEECH_ARTICULATION_SCALE,
    emphasis,
  })
}

function coalesce(input: TextVisemeCue[]): TextVisemeCue[] {
  const output: TextVisemeCue[] = []
  for (const cue of input) {
    const previous = output[output.length - 1]
    const limit = cue.viseme === 'rest' ? 0.64 : 0.28
    if (
      previous?.viseme === cue.viseme &&
      previous.duration + cue.duration <= limit
    ) {
      previous.duration += cue.duration
      previous.emphasis ||= cue.emphasis
    } else {
      output.push({ ...cue })
    }
  }
  return output
}
