import type { MusicPhrase } from './musicSignal'
import type { SingingTimelineInput } from './singingTimeline'

export const MUSIC_PHRASE_PREPARATION_SECONDS = 0.28
export const MUSIC_PHRASE_RELEASE_SECONDS = 0.75

/** Retain line boundaries before phoneme compilation flattens them away. */
export function compileMusicPhrases(
  input: SingingTimelineInput,
): MusicPhrase[] {
  if (input.verbatim?.length) {
    return input.verbatim
      .flatMap((line, index) => {
        const words = (line.words ?? []).filter(
          (word) =>
            word.text.trim() && Number.isFinite(word.time) && word.time >= 0,
        )
        if (!words.length) return []
        const first = words[0]
        const last = words[words.length - 1]
        const next = input.verbatim?.[index + 1]?.words?.[0]?.time
        const end =
          last.time +
          (last.duration > 0
            ? last.duration
            : Math.min(
                0.6,
                Math.max(0.08, (next ?? last.time + 0.4) - last.time),
              ))
        return [
          {
            start: first.time,
            end: Math.max(first.time + 0.08, end),
            confidence: last.duration > 0 ? 0.95 : 0.7,
          },
        ]
      })
      .sort((a, b) => a.start - b.start)
  }
  return (input.lines ?? [])
    .flatMap((line, index, lines) => {
      if (!line.text.trim() || !Number.isFinite(line.time) || line.time < 0)
        return []
      const next =
        lines[index + 1]?.time ?? input.songDuration ?? line.time + 2.4
      const end = Math.min(next, line.time + 6)
      return end > line.time
        ? [{ start: line.time, end, confidence: 0.55 }]
        : []
    })
    .sort((a, b) => a.start - b.start)
}

/** Seek-safe binary lookup; a close incoming phrase takes over the old release. */
export function sampleMusicPhrase(
  phrases: readonly MusicPhrase[],
  time: number,
): MusicPhrase | null {
  let low = 0
  let high = phrases.length
  while (low < high) {
    const mid = (low + high) >>> 1
    if (phrases[mid].start <= time) low = mid + 1
    else high = mid
  }
  const active = phrases[low - 1]
  if (active && time < active.end) return active
  const next = phrases[low]
  if (next && next.start - time <= MUSIC_PHRASE_PREPARATION_SECONDS) return next
  return active && time < active.end + MUSIC_PHRASE_RELEASE_SECONDS
    ? active
    : null
}
