import type { StageChapter } from './stageScript'
import { useCallback, useEffect, useReducer, useRef } from 'react'
import { CHAPTER_TAIL_MS } from './stageScript'

/** 开幕后第一句出来前的停顿。 */
const FIRST_BEAT_DELAY_MS = 500

export interface StagePlayerState {
  chapter: number
  /** 这一幕已经出来几句。 */
  shown: number
  playing: boolean
  ended: boolean
}

type Action =
  | { type: 'reset'; play: boolean }
  | { type: 'tick'; chapters: readonly StageChapter[] }
  | { type: 'toggle' }
  | { type: 'pause' }
  | { type: 'jump'; chapter: number; chapters: readonly StageChapter[] }

export function stagePlayerReducer(state: StagePlayerState, action: Action): StagePlayerState {
  switch (action.type) {
    case 'reset':
      return { chapter: 0, shown: 0, playing: action.play, ended: false }
    case 'toggle':
      if (state.ended) return { chapter: 0, shown: 0, playing: true, ended: false }
      return { ...state, playing: !state.playing }
    case 'pause':
      return state.playing ? { ...state, playing: false } : state
    case 'jump': {
      const chapter = Math.max(0, Math.min(action.chapters.length - 1, action.chapter))
      return { chapter, shown: 0, playing: true, ended: false }
    }
    case 'tick': {
      const current = action.chapters[state.chapter]
      if (!current) return { ...state, playing: false, ended: true }
      if (state.shown < current.beats.length) return { ...state, shown: state.shown + 1 }
      if (state.chapter + 1 < action.chapters.length) {
        return { ...state, chapter: state.chapter + 1, shown: 0 }
      }
      return { ...state, playing: false, ended: true }
    }
  }
}

/** 下一次 tick 之前要等多久。 */
export function nextTickDelay(state: StagePlayerState, chapters: readonly StageChapter[]): number {
  const current = chapters[state.chapter]
  if (!current) return 0
  if (state.shown === 0) return FIRST_BEAT_DELAY_MS
  const last = current.beats[state.shown - 1]
  const hold = last?.holdMs ?? 0
  return state.shown < current.beats.length ? hold : hold + CHAPTER_TAIL_MS
}

/**
 * 字幕播放。暂停停在当前这句，继续从这句接着播；`running` 为 false（弹窗收进暂存、
 * 页面切走）时也停，但不改用户的播放/暂停选择。
 */
export function useStagePlayer(
  chapters: readonly StageChapter[],
  options: { running: boolean; resetKey: unknown; onEnded?: () => void },
) {
  const [state, dispatch] = useReducer(stagePlayerReducer, {
    chapter: 0,
    shown: 0,
    playing: true,
    ended: false,
  })
  const onEndedRef = useRef(options.onEnded)
  onEndedRef.current = options.onEnded

  useEffect(() => {
    dispatch({ type: 'reset', play: true })
  }, [options.resetKey])

  useEffect(() => {
    if (!options.running || !state.playing || state.ended || chapters.length === 0) return
    const timer = window.setTimeout(
      dispatch,
      nextTickDelay(state, chapters),
      { type: 'tick', chapters } as const,
    )
    return () => window.clearTimeout(timer)
  }, [chapters, options.running, state])

  const endedRef = useRef(state.ended)
  useEffect(() => {
    if (state.ended && !endedRef.current) onEndedRef.current?.()
    endedRef.current = state.ended
  }, [state.ended])

  const toggle = useCallback(() => dispatch({ type: 'toggle' }), [])
  const pause = useCallback(() => dispatch({ type: 'pause' }), [])
  const replay = useCallback(() => dispatch({ type: 'reset', play: true }), [])
  const jump = useCallback(
    (chapter: number) => dispatch({ type: 'jump', chapter, chapters }),
    [chapters],
  )
  /** 不等这句停够，直接出下一句；一幕的最后一句之后进下一幕。播放/暂停不变，计时从这句重新算。 */
  const next = useCallback(() => dispatch({ type: 'tick', chapters }), [chapters])

  return { state, toggle, pause, replay, jump, next }
}
