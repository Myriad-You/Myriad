import type { MutableRefObject } from 'react'
import type {
  LyricLine,
  Song,
  VerbatimLyricsSource,
  WordLyricLine,
} from '../../utils/musicPlayer'

import { useCallback, useEffect, useRef, useState } from 'react'
import { getLyricsWithVerbatim } from '../../utils/musicPlayer'
import { getGlobalState, patchLiveGlobalState } from './globalState'

export interface MusicLyricsApi {
  lyrics: LyricLine[]
  verbatimLyrics: WordLyricLine[]
  verbatimLyricsSource: VerbatimLyricsSource
  currentLyricIndex: number
  lyricsRef: MutableRefObject<LyricLine[]>
  verbatimLyricsRef: MutableRefObject<WordLyricLine[]>
  currentLyricIndexRef: MutableRefObject<number>
  setCurrentLyricIndex: (index: number) => void
  resetLyrics: () => void
  loadLyricsForSong: (song: Song) => void
}

export function useLyrics(): MusicLyricsApi {
  const [lyrics, setLyrics] = useState<LyricLine[]>([])
  const [verbatimLyrics, setVerbatimLyrics] = useState<WordLyricLine[]>([])
  const [verbatimLyricsSource, setVerbatimLyricsSource] =
    useState<VerbatimLyricsSource>('')
  const [currentLyricIndex, setCurrentLyricIndex] = useState(-1)

  const lyricsRef = useRef<LyricLine[]>([])
  const verbatimLyricsRef = useRef<WordLyricLine[]>([])
  const currentLyricIndexRef = useRef(-1)
  const lyricRequestKeyRef = useRef('')

  useEffect(() => {
    const globalState = getGlobalState()
    if (!globalState) return
    if (Array.isArray(globalState.lyrics)) {
      setLyrics(globalState.lyrics as LyricLine[])
    }
    if (Array.isArray(globalState.verbatimLyrics)) {
      setVerbatimLyrics(globalState.verbatimLyrics as WordLyricLine[])
    }
    if (typeof globalState.verbatimLyricsSource === 'string') {
      setVerbatimLyricsSource(
        globalState.verbatimLyricsSource as VerbatimLyricsSource,
      )
    }
    if (typeof globalState.currentLyricIndex === 'number') {
      setCurrentLyricIndex(globalState.currentLyricIndex)
    }
  }, [])

  useEffect(() => {
    lyricsRef.current = lyrics
    // 直接补丁 globalState，进度 tick 不依赖广播。
    patchLiveGlobalState({ lyrics })
  }, [lyrics])

  useEffect(() => {
    verbatimLyricsRef.current = verbatimLyrics
    patchLiveGlobalState({
      verbatimLyrics,
      hasVerbatimLyrics: verbatimLyrics.length > 0,
      verbatimLyricsSource,
    })
  }, [verbatimLyrics, verbatimLyricsSource])

  useEffect(() => {
    currentLyricIndexRef.current = currentLyricIndex
    patchLiveGlobalState({ currentLyricIndex })
  }, [currentLyricIndex])

  const resetLyrics = useCallback(() => {
    setLyrics([])
    setVerbatimLyrics([])
    setVerbatimLyricsSource('')
    setCurrentLyricIndex(-1)
    patchLiveGlobalState({
      lyrics: [],
      verbatimLyrics: [],
      hasVerbatimLyrics: false,
      verbatimLyricsSource: '',
      currentLyricIndex: -1,
    })
  }, [])

  const loadLyricsForSong = useCallback(
    (song: Song) => {
      const requestKey = `${song.source}-${song.id}`
      lyricRequestKeyRef.current = requestKey
      resetLyrics()

      // 不走 loadResource.completed：同 id 二次点会被 addTask 跳过，歌词永久空白。
      void (async () => {
        try {
          const result = await getLyricsWithVerbatim(song)
          if (lyricRequestKeyRef.current !== requestKey) return

          setLyrics(result.lines)
          setVerbatimLyrics(result.verbatim)
          setVerbatimLyricsSource(result.verbatimSource)
          setCurrentLyricIndex(-1)
        } catch {
          if (lyricRequestKeyRef.current !== requestKey) return
          resetLyrics()
        }
      })()
    },
    [resetLyrics],
  )

  return {
    lyrics,
    verbatimLyrics,
    verbatimLyricsSource,
    currentLyricIndex,
    lyricsRef,
    verbatimLyricsRef,
    currentLyricIndexRef,
    setCurrentLyricIndex,
    resetLyrics,
    loadLyricsForSong,
  }
}
