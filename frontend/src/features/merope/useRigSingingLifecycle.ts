import { useEffect, useRef } from 'react'
import { useMusicPlayerControl } from '../../contexts/MusicPlayerContext'
import { getMusicMotionSource } from './motion/musicSourceRuntime'

export function useRigSingingLifecycle(enabled = true): void {
  const { isPlaying, currentSong, lyrics, verbatimLyrics, hasVerbatimLyrics } =
    useMusicPlayerControl()
  const trackId = currentSong
    ? `${currentSong.source}:${currentSong.id}`
    : ''
  const lastTrackIdRef = useRef(trackId)

  useEffect(() => {
    if (!enabled) return
    const source = getMusicMotionSource()
    if (trackId && trackId !== lastTrackIdRef.current) source.markSwitching()
    lastTrackIdRef.current = trackId
    source.setTrack({
      trackId,
      duration: currentSong?.duration,
      verbatim: hasVerbatimLyrics ? verbatimLyrics : undefined,
      lines: lyrics,
    })
  }, [
    enabled,
    trackId,
    currentSong?.duration,
    hasVerbatimLyrics,
    lyrics,
    verbatimLyrics,
  ])

  useEffect(() => {
    if (!enabled) return
    getMusicMotionSource().setPlayback(isPlaying, false)
  }, [enabled, isPlaying])
}
