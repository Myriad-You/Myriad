export const SPEECH_GESTURES = [
  'question',
  'contrast',
  'laugh',
  'hesitate',
  'tease',
  'check-in',
] as const
export type SpeechGesture = (typeof SPEECH_GESTURES)[number]

/** Mask quoted/code content without moving the speech plan's UTF-16 anchors. */
export function unquotedSpeechText(text: string): string {
  return text.replace(
    /```[\s\S]*?(?:```|$)|`[^`]*(?:`|$)|“[^”]*(?:”|$)|‘[^’]*(?:’|$)|「[^」]*(?:」|$)|『[^』]*(?:』|$)|"[^"]*(?:"|$)|https?:\/\/\S+/gu,
    (match) => ' '.repeat(match.length),
  )
}

/** Conservative delivery cues, not an appraisal of the speaker's mood. */
export function speechPhraseGestures(
  text: string,
  streaming = false,
): Array<{ textOffset: number; gesture: SpeechGesture }> {
  // Preserve UTF-16 offsets used by the prosody plan. Unfinished quotations
  // and code remain inert while streaming, rather than briefly being acted.
  const unquoted = unquotedSpeechText(text)
  const cues: Array<{ textOffset: number; gesture: SpeechGesture }> = []
  for (const match of unquoted.matchAll(/[?？]+/gu))
    cues.push({ textOffset: match.index!, gesture: 'question' })
  for (const match of unquoted.matchAll(
    /但是|不过|然而|けれど|だけど|しかし|\b(?:but|however|yet)\b/giu,
  )) {
    const end = match.index! + match[0].length
    if (streaming && end === text.length && /[a-z]$/iu.test(match[0])) continue
    cues.push({ textOffset: end, gesture: 'contrast' })
  }
  for (const match of unquoted.matchAll(
    /(?:^|[。.!?！？,，;；\n])\s*(哈{2,}|へ{2,}|ふ{2,}|(?:ha){2,})(?=$|[\s。.!?！？,，;；])/giu,
  )) {
    if (streaming && match.index! + match[0].length === text.length) continue
    const start = match.index! + match[0].length - match[1]!.length
    // The first two laugh syllables identify one burst, even if more arrive.
    cues.push({
      textOffset: start + (/^ha/iu.test(match[1]!) ? 4 : 2),
      gesture: 'laugh',
    })
  }
  return cues.sort((a, b) => a.textOffset - b.textOffset)
}
