import type { ReactNode } from 'react'

import './Skeleton.css'

interface QuickTransitionProps {
  transitioning: boolean
  children: ReactNode
  className?: string
}

export function QuickTransition({
  transitioning,
  children,
  className = '',
}: QuickTransitionProps) {
  return (
    <div
      className={`quick-transition ${transitioning ? 'transitioning' : 'visible'} ${className}`}
    >
      {children}
    </div>
  )
}
