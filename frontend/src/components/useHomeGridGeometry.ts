import type { HomeLayoutMode } from '../utils/homeLayout'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'

export function useHomeGridMotionMode(
  layoutMode: HomeLayoutMode,
): HomeLayoutMode {
  const [motionMode, setMotionMode] = useState(layoutMode)
  useLayoutEffect(() => {
    if (motionMode === layoutMode) return
    let second = 0
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => setMotionMode(layoutMode))
    })
    return () => {
      cancelAnimationFrame(first)
      cancelAnimationFrame(second)
    }
  }, [layoutMode, motionMode])
  return motionMode
}

export function useRowCountMorphing(currentGridHeight: number): boolean {
  const [rowCountMorphing, setRowCountMorphing] = useState(false)
  const prevRowCountRef = useRef(currentGridHeight)
  useEffect(() => {
    if (prevRowCountRef.current === currentGridHeight) return
    prevRowCountRef.current = currentGridHeight
    setRowCountMorphing(true)
    const timer = window.setTimeout(setRowCountMorphing, 500, false)
    return () => window.clearTimeout(timer)
  }, [currentGridHeight])
  return rowCountMorphing
}
