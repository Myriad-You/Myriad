import { useEffect, useRef } from 'react'
import { useMusicPlayerControl } from '../../contexts/MusicPlayerContext'
import { getMusicMotionSource } from './motion/musicSourceRuntime'

/**
 * Bind site playback to the global music sampler. Frames are consumed
 * through MotionRuntime, not written here.
 */
export function useRigSingingLifecycle(): void {
  const { isPlaying, currentSong, lyrics, verbatimLyrics, hasVerbatimLyrics } =
    useMusicPlayerControl()
  const trackId = currentSong
    ? `${currentSong.source}:${currentSong.id}`
    : ''
  const lastTrackIdRef = useRef(trackId)

  useEffect(() => {
    const source = getMusicMotionSource()
    if (trackId && trackId !== lastTrackIdRef.current) source.markSwitching()
    lastTrackIdRef.current = trackId
    source.setTrack({
      trackId,
      duration: currentSong?.duration,
      verbatim: hasVerbatimLyrics ? verbatimLyrics : undefined,
      lines: lyrics,
    })
  }, [trackId, currentSong?.duration, hasVerbatimLyrics, lyrics, verbatimLyrics])

  useEffect(() => {
    getMusicMotionSource().setPlayback(isPlaying, false)
  }, [isPlaying])
}
