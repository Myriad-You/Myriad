import type { ReactNode } from 'react'

import React, { useEffect, useRef, useState } from 'react'
import { AutoHeight } from './AutoHeight'
import { prefersReducedMotion, SETTINGS_DURATION_MS } from './motion'
import './settings-motion.css'

export type SectionSwitchDirection = 'forward' | 'back'

export interface SectionSwitchProps {
  sectionKey: string
  children: (sectionKey: string) => ReactNode
  direction?: SectionSwitchDirection
  onCommit?: (sectionKey: string) => void
  className?: string
}

export const SectionSwitch: React.FC<SectionSwitchProps> = ({
  sectionKey,
  children,
  direction = 'forward',
  onCommit,
  className = '',
}) => {
  const [shownKey, setShownKey] = useState(sectionKey)
  const shownKeyRef = useRef(sectionKey)
  const [phase, setPhase] = useState<'in' | 'out' | 'idle'>('idle')
  const timerRef = useRef<number | undefined>(undefined)
  // callback in ref so identity changes don't retrigger
  const onCommitRef = useRef(onCommit)
  onCommitRef.current = onCommit

  useEffect(() => {
    window.clearTimeout(timerRef.current)

    if (sectionKey === shownKeyRef.current) {
      setPhase('idle')
      return undefined
    }

    const commitIn = () => {
      shownKeyRef.current = sectionKey
      setShownKey(sectionKey)
      setPhase('in')
      onCommitRef.current?.(sectionKey)
      timerRef.current = window.setTimeout(
        setPhase,
        SETTINGS_DURATION_MS.slow + 60,
        'idle',
      )
    }

    // reduced-motion: skip exit so there is no empty beat
    if (prefersReducedMotion()) {
      commitIn()
      return undefined
    }

    setPhase('out')
    const id = window.setTimeout(commitIn, SETTINGS_DURATION_MS.fast)
    timerRef.current = id
    return () => window.clearTimeout(timerRef.current)
  }, [sectionKey])

  useEffect(
    () => () => {
      window.clearTimeout(timerRef.current)
    },
    [],
  )

  const handleAnimationEnd = React.useCallback(
    (event: React.AnimationEvent<HTMLDivElement>) => {
      if (event.animationName !== 'sm-switch-in') return
      setPhase('idle')
    },
    [],
  )

  return (
    <div
      className={`sm-switch ${className}`.trim()}
      data-phase={phase}
      data-dir={direction}
      onAnimationEnd={handleAnimationEnd}
      aria-busy={phase === 'out' || undefined}
    >
      <AutoHeight contentKey={shownKey} className="sm-switch-height">
        {children(shownKey)}
      </AutoHeight>
    </div>
  )
}

SectionSwitch.displayName = 'SectionSwitch'

export default SectionSwitch
