/**
 * Turns model output into something a TTS engine should read.
 * Markdown, code, and URLs stay on screen; they are not spoken.
 */

const FENCE = /```[\s\S]*?```/g
const INLINE_CODE = /`[^`]+`/g
const IMAGE = /!\[[^\]]*\]\([^)]+\)/g
const LINK = /\[[^\]]*\]\([^)]+\)/g
const URL = /\bhttps?:\/\/\S+/gi
const WWW = /\bwww\.\S+/gi
const HEADING = /^#{1,6}\s+/gm
const LIST = /^\s*(?:[-*+]|\d+\.)\s+/gm
const EMPHASIS = /[*_~]{1,3}/g
const HTML = /<\/?[a-z][^>]*>/gi
const TABLE_ROW = /^\s*\|.*\|\s*$/gm

export function speakableText(raw: string): string {
  let text = raw.replace(/\r\n/g, '\n')
  text = text.replace(FENCE, ' ')
  text = text.replace(INLINE_CODE, ' ')
  text = text.replace(IMAGE, ' ')
  text = text.replace(LINK, ' ')
  text = text.replace(URL, ' ')
  text = text.replace(WWW, ' ')
  text = text.replace(HEADING, '')
  text = text.replace(LIST, '')
  text = text.replace(TABLE_ROW, ' ')
  text = text.replace(HTML, ' ')
  text = text.replace(EMPHASIS, '')
  text = text.replace(/[^\S\n]+/g, ' ')
  text = text.replace(/ *\n */g, '\n')
  return text.replace(/\n{2,}/g, '\n').trim()
}
