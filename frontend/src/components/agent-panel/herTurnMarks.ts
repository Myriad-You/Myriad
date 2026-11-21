/**
 * What she did with the owner's line, as her kept turn carries it (core
 * `myriad_merope::unanswered`): `[[quiet]]` alone, she read it and left it;
 * `[[react:👍]]`, she put that reaction on it, with words or alone. The marks
 * stay in what is kept, so the conversation she has next time shows what she
 * did; they are never shown as words.
 */
export interface HerTurnMarks {
  /** Her words, without the marks. */
  text: string
  /** She read their line and left it. */
  left: boolean
  /** The reaction she put on their line. */
  reaction?: string
}

const MARK = /\[\[([^\]]*)\]\]/g

export function herTurnMarks(content: string): HerTurnMarks {
  let quiet = false
  let reaction: string | undefined
  const text = content
    .replace(MARK, (whole: string, inner: string) => {
      const at = inner.search(/[:：]/)
      const name = (at < 0 ? inner : inner.slice(0, at)).trim().toLowerCase()
      const value = at < 0 ? '' : inner.slice(at + 1).trim()
      if (name === 'quiet') {
        quiet = true
        return ''
      }
      if (name !== 'react') {
        return whole
      }
      if (!reaction && value) {
        reaction = value.replace(/\uFE0F$/, '')
      }
      return ''
    })
    .split('\n')
    .filter((line) => line.trim() !== '')
    .join('\n')
  return {
    text,
    left: quiet && !reaction && text.trim() === '',
    ...(reaction ? { reaction } : {}),
  }
}
