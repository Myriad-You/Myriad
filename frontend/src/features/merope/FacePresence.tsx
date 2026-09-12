import type { ReactNode, RefObject } from 'react'
import {
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
} from 'react'
import {
  copySurfaceFrame,
  facePresenceDurationMs,
  INITIAL_FACE_PRESENCE,
  reduceFacePresence,
} from './reduceFacePresence'
import './facePresence.css'

function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

export function FacePresence({
  present,
  packageKey,
  ready,
  children,
  vacant,
  onLiveUnmounted,
  hostRef,
}: {
  present: boolean
  packageKey: string
  ready: boolean
  children: ReactNode | ((mounted: boolean) => ReactNode)
  vacant?: ReactNode
  /** Outfit swap keeps the same lease after unmount. */
  onLiveUnmounted?: () => void
  /** Host writes `data-face-phase` for siblings; no `:has()`. */
  hostRef?: RefObject<HTMLElement | null>
}) {
  const rootRef = useRef<HTMLDivElement>(null)
  const [state, dispatch] = useReducer(reduceFacePresence, INITIAL_FACE_PRESENCE)
  const [hold, setHold] = useState<HTMLCanvasElement | null>(null)
  const packageRef = useRef('')
  const presentRef = useRef(false)
  const liveMountedRef = useRef(false)
  const onLiveUnmountedRef = useRef(onLiveUnmounted)
  onLiveUnmountedRef.current = onLiveUnmounted

  const readyRef = useRef(false)

  useLayoutEffect(() => {
    const wasPresent = presentRef.current
    const previousKey = packageRef.current
    presentRef.current = present
    packageRef.current = packageKey
    if (!present) {
      readyRef.current = false
      if (wasPresent) {
        setHold(copySurfaceFrame(rootRef.current))
        dispatch({ type: 'hide' })
      }
      return
    }
    if (!wasPresent) {
      readyRef.current = false
      setHold(null)
      dispatch({ type: 'show', packageKey })
      return
    }
    if (previousKey && previousKey !== packageKey) {
      readyRef.current = false
      setHold(copySurfaceFrame(rootRef.current))
      dispatch({ type: 'swap', packageKey })
    }
  }, [packageKey, present])

  useEffect(() => {
    const wasReady = readyRef.current
    readyRef.current = ready
    if (!present || !ready || wasReady) return
    dispatch({ type: 'ready' })
  }, [present, ready, packageKey])

  useEffect(() => {
    if (
      state.phase !== 'enter' &&
      state.phase !== 'exit' &&
      state.phase !== 'rest'
    ) {
      return undefined
    }
    const timer = window.setTimeout(
      dispatch,
      facePresenceDurationMs(state.phase, prefersReducedMotion()),
      { type: 'elapsed' },
    )
    return () => window.clearTimeout(timer)
  }, [state.phase])

  useLayoutEffect(() => {
    const host = hostRef?.current
    if (!host) return undefined
    host.dataset.facePhase = state.phase
    return () => {
      delete host.dataset.facePhase
    }
  }, [hostRef, state.phase])

  useEffect(() => {
    if (state.phase === 'exit') return
    setHold(null)
  }, [state.phase])

  useLayoutEffect(() => {
    const wasMounted = liveMountedRef.current
    liveMountedRef.current = state.liveMounted
    if (wasMounted && !state.liveMounted) onLiveUnmountedRef.current?.()
  }, [state.liveMounted])

  useLayoutEffect(() => {
    return () => {
      if (!liveMountedRef.current) return
      liveMountedRef.current = false
      onLiveUnmountedRef.current?.()
    }
  }, [])

  const mounted = state.liveMounted
  const live =
    typeof children === 'function' ? children(mounted) : mounted ? children : null
  const showHold = Boolean(hold) && state.phase === 'exit'
  const showVacant = state.vacant && !present ? vacant : null

  return (
    <div
      ref={rootRef}
      className="face-presence"
      data-phase={state.phase}
      data-vacant={showVacant ? '' : undefined}
    >
      {showHold && hold ? <PresenceHold source={hold} /> : null}
      {mounted ? (
        <div className="face-presence__live" key={state.packageKey}>
          {live}
        </div>
      ) : null}
      {showVacant ? (
        <div className="face-presence__vacant">{showVacant}</div>
      ) : null}
    </div>
  )
}

function PresenceHold({ source }: { source: HTMLCanvasElement }) {
  const ref = useRef<HTMLCanvasElement>(null)
  useLayoutEffect(() => {
    const canvas = ref.current
    if (!canvas) return
    canvas.width = source.width
    canvas.height = source.height
    const context = canvas.getContext('2d')
    context?.clearRect(0, 0, canvas.width, canvas.height)
    context?.drawImage(source, 0, 0)
  }, [source])
  return (
    <canvas
      ref={ref}
      className="face-presence__hold"
      aria-hidden
    />
  )
}
