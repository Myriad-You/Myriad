export type SingingPlaybackGap = 'active' | 'hold' | 'stop'

export function singingPlaybackGap(
  playing: boolean,
  switchingTracks: boolean,
): SingingPlaybackGap {
  if (playing) return 'active'
  if (switchingTracks) return 'hold'
  return 'stop'
}
