import type { AnimationEvent, CSSProperties } from 'react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  isExlight,
  useAnimationLevel,
} from '../../hooks/useAnimationLevel'
import { isWebKit } from '../../utils/platformDetect'
import './tappShellPresence.css'

export type TappShellPresencePhase = 'enter' | 'shown' | 'exit'

const ENTER_DURATION_MS = 480
const EXIT_DURATION_MS = 320

const COLUMN_ENTER_NAMES = new Set(['tapp-shell-in', 'tapp-shell-in-fade'])
const COLUMN_EXIT_NAMES = new Set(['tapp-shell-out', 'tapp-shell-out-fade'])

export interface UseTappShellPresenceOptions {
  /** 全屏时关闭 presence：fixed 内容不能放在 transform 下。 */
  enabled: boolean
  /** 列 fade 默认在 WebKit 关闭：祖先 opacity 会弄坏 iframe。 */
  fade?: boolean
}

export interface UseTappShellPresenceResult {
  phase: TappShellPresencePhase
  isSettled: boolean
  isExiting: boolean
  shellClassName: string
  scrimClassName: string
  shellStyle: CSSProperties | undefined
  onShellAnimationEnd: (event: AnimationEvent<HTMLElement>) => void
  requestClose: (action: () => void) => void
}

export function useTappShellPresence(
  options: UseTappShellPresenceOptions,
): UseTappShellPresenceResult {
  const animConfig = useAnimationLevel()
  const motionOn = options.enabled && !isExlight(animConfig)
  const fadeColumn = options.fade ?? !isWebKit

  const [phase, setPhase] = useState<TappShellPresencePhase>(() =>
    motionOn ? 'enter' : 'shown',
  )
  const pendingCloseRef = useRef<(() => void) | null>(null)
  const phaseRef = useRef(phase)
  phaseRef.current = phase
  const flushedExitRef = useRef(false)

  const flushExit = useCallback(() => {
    if (flushedExitRef.current) return
    flushedExitRef.current = true
    const action = pendingCloseRef.current
    pendingCloseRef.current = null
    action?.()
  }, [])

  useEffect(() => {
    if (!motionOn) {
      setPhase('shown')
      if (pendingCloseRef.current) {
        flushExit()
      }
    }
  }, [motionOn, flushExit])

  const exitFallbackMs =
    Math.round(EXIT_DURATION_MS * animConfig.durationScale) + 80

  useEffect(() => {
    if (phase !== 'exit') return
    flushedExitRef.current = false
    const timer = window.setTimeout(flushExit, exitFallbackMs)
    return () => window.clearTimeout(timer)
  }, [phase, exitFallbackMs, flushExit])

  useEffect(() => {
    if (phase !== 'enter' || !motionOn) return
    const ms = Math.round(ENTER_DURATION_MS * animConfig.durationScale) + 80
    const timer = window.setTimeout(() => {
      setPhase((p) => (p === 'enter' ? 'shown' : p))
    }, ms)
    return () => window.clearTimeout(timer)
  }, [phase, motionOn, animConfig.durationScale])

  const onShellAnimationEnd = useCallback(
    (event: AnimationEvent<HTMLElement>) => {
      if (event.target !== event.currentTarget) return
      const name = event.animationName
      if (COLUMN_ENTER_NAMES.has(name) && phaseRef.current === 'enter') {
        setPhase('shown')
        return
      }
      if (COLUMN_EXIT_NAMES.has(name) && phaseRef.current === 'exit') {
        flushExit()
      }
    },
    [flushExit],
  )

  const requestClose = useCallback(
    (action: () => void) => {
      if (!motionOn) {
        action()
        return
      }
      if (phaseRef.current === 'exit') return

      flushedExitRef.current = false
      pendingCloseRef.current = action
      setPhase('exit')
    },
    [motionOn],
  )

  const shellClassName = useMemo(() => {
    if (!motionOn) return ''
    const fade = fadeColumn ? ' tapp-shell-presence--fade' : ''
    return `tapp-shell-presence${fade} tapp-shell-presence--${phase}`
  }, [motionOn, fadeColumn, phase])

  const scrimClassName = motionOn
    ? `tapp-shell-scrim tapp-shell-scrim--${phase}`
    : ''

  const shellStyle = useMemo((): CSSProperties | undefined => {
    if (!motionOn) return undefined
    const scale = animConfig.durationScale
    return {
      ['--tapp-shell-enter-duration' as string]: `${(ENTER_DURATION_MS * scale) / 1000}s`,
      ['--tapp-shell-exit-duration' as string]: `${(EXIT_DURATION_MS * scale) / 1000}s`,
    }
  }, [motionOn, animConfig.durationScale])

  return {
    phase,
    isSettled: phase === 'shown',
    isExiting: phase === 'exit',
    shellClassName,
    scrimClassName,
    shellStyle,
    onShellAnimationEnd,
    requestClose,
  }
}
