export type SingingPlaybackGap = 'active' | 'hold' | 'stop'

/** Skip keeps the body pose; a real pause winds it down. */
export function singingPlaybackGap(
  playing: boolean,
  switchingTracks: boolean,
): SingingPlaybackGap {
  if (playing) return 'active'
  if (switchingTracks) return 'hold'
  return 'stop'
}
