/** outer margin never follows open; title→content gap is inner padding */

import type { ReactNode } from 'react'

import React, { useEffect, useRef, useState } from 'react'
import { prefersReducedMotion, SETTINGS_DURATION_MS } from './motion'
import './settings-motion.css'

type CollapseState = 'collapsed' | 'entering' | 'open'

export interface CollapseRegionProps {
  open: boolean
  children: ReactNode
  className?: string
}

export const CollapseRegion: React.FC<CollapseRegionProps> = ({
  open,
  children,
  className = '',
}) => {
  const [mounted, setMounted] = useState(open)
  const [state, setState] = useState<CollapseState>(open ? 'open' : 'collapsed')
  const isFirstRun = useRef(true)
  const timerRef = useRef<number | undefined>(undefined)
  const rafOuterRef = useRef<number | undefined>(undefined)
  const rafInnerRef = useRef<number | undefined>(undefined)

  useEffect(() => {
    const first = isFirstRun.current
    isFirstRun.current = false

    window.clearTimeout(timerRef.current)
    if (rafOuterRef.current != null) cancelAnimationFrame(rafOuterRef.current)
    if (rafInnerRef.current != null) cancelAnimationFrame(rafInnerRef.current)

    const settleDelay = prefersReducedMotion() ? 0 : SETTINGS_DURATION_MS.slow

    if (open) {
      setMounted(true)
      if (first) {
        setState('open')
        return undefined
      }

      // paint collapsed one frame, then open; else both states commit in one frame
      setState('collapsed')
      rafOuterRef.current = requestAnimationFrame(() => {
        rafInnerRef.current = requestAnimationFrame(() => {
          setState('entering')
          timerRef.current = window.setTimeout(() => {
            setState('open')
          }, settleDelay)
        })
      })
      return undefined
    }

    setState('collapsed')
    if (first) {
      setMounted(false)
      return undefined
    }
    timerRef.current = window.setTimeout(() => {
      setMounted(false)
    }, settleDelay)
    return undefined
  }, [open])

  useEffect(
    () => () => {
      window.clearTimeout(timerRef.current)
      if (rafOuterRef.current != null) cancelAnimationFrame(rafOuterRef.current)
      if (rafInnerRef.current != null) cancelAnimationFrame(rafInnerRef.current)
    },
    [],
  )

  if (!mounted) return null

  return (
    <div
      className={`sm-collapse${className ? ` ${className}` : ''}`}
      data-state={state}
      aria-hidden={!open || undefined}
    >
      <div className="sm-collapse-inner">{children}</div>
    </div>
  )
}

CollapseRegion.displayName = 'CollapseRegion'

export default CollapseRegion
