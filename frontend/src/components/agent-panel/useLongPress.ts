import { useCallback, useEffect, useRef, useState } from 'react'

interface LongPressIndicator {
  x: number
  y: number
  active: boolean
}

const EXCLUDED_SELECTORS =
  '.agent-panel-overlay-anchor, input, textarea, button, a, [contenteditable], [data-merope-touch-active], .global-control-bar, .control-panel-overlay, .tour-overlay, .tour-card'

export const LONG_PRESS_DURATION = 500

export function useLongPress(
  duration: number,
  onTrigger: (() => void) | undefined,
  enabled: boolean,
) {
  const timerRef = useRef<NodeJS.Timeout | null>(null)
  const startRef = useRef<{ x: number; y: number } | null>(null)
  const isPressing = useRef(false)
  const [indicator, setIndicator] = useState<LongPressIndicator>({
    x: 0,
    y: 0,
    active: false,
  })

  const dropDragRef = useRef<() => void>(() => {})

  const cancel = useCallback(() => {
    dropDragRef.current()
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
    isPressing.current = false
    startRef.current = null
    setIndicator((prev) => ({ ...prev, active: false }))
  }, [])

  const start = useCallback(
    (e: MouseEvent | TouchEvent) => {
      if (!enabled) return
      if ('button' in e && e.button !== 0) return
      const target = e.target as HTMLElement
      if (target.closest(EXCLUDED_SELECTORS)) return

      const point = 'touches' in e ? e.touches[0] : e
      startRef.current = { x: point.clientX, y: point.clientY }
      isPressing.current = true
      setIndicator({ x: point.clientX, y: point.clientY, active: true })

      timerRef.current = setTimeout(() => {
        if (isPressing.current) {
          setIndicator((prev) => ({ ...prev, active: false }))
          onTrigger?.()
          if (navigator.vibrate) navigator.vibrate(50)
        }
      }, duration)
    },
    [enabled, duration, onTrigger],
  )

  const checkMovement = useCallback(
    (e: MouseEvent | TouchEvent) => {
      if (!startRef.current || !isPressing.current) return
      const point = 'touches' in e ? e.touches[0] : e
      const dx = Math.abs(point.clientX - startRef.current.x)
      const dy = Math.abs(point.clientY - startRef.current.y)
      if (dx > 10 || dy > 10) cancel()
    },
    [cancel],
  )

  useEffect(() => {
    if (!enabled) {
      cancel()
      return undefined
    }

    let dragging = false
    const handleMove = (e: MouseEvent | TouchEvent) => checkMovement(e)
    const handleUp = () => cancel()
    const dropDrag = () => {
      if (!dragging) return
      dragging = false
      document.removeEventListener('mouseup', handleUp)
      document.removeEventListener('touchend', handleUp)
      document.removeEventListener('touchcancel', handleUp)
      document.removeEventListener('mousemove', handleMove)
      document.removeEventListener('touchmove', handleMove)
      document.removeEventListener('contextmenu', handleUp)
    }
    const takeDrag = () => {
      if (dragging) return
      dragging = true
      document.addEventListener('mouseup', handleUp)
      document.addEventListener('touchend', handleUp)
      document.addEventListener('touchcancel', handleUp)
      document.addEventListener('mousemove', handleMove)
      document.addEventListener('touchmove', handleMove, { passive: true })
      document.addEventListener('contextmenu', handleUp)
    }
    dropDragRef.current = dropDrag

    const handleMouseDown = (e: MouseEvent) => {
      start(e)
      if (isPressing.current) takeDrag()
    }
    const handleTouchStart = (e: TouchEvent) => {
      start(e)
      if (isPressing.current) takeDrag()
    }

    document.addEventListener('mousedown', handleMouseDown)
    document.addEventListener('touchstart', handleTouchStart, {
      passive: true,
    })

    return () => {
      document.removeEventListener('mousedown', handleMouseDown)
      document.removeEventListener('touchstart', handleTouchStart)
      cancel()
      dropDragRef.current = () => {}
    }
  }, [enabled, start, cancel, checkMovement])

  return { indicator }
}
