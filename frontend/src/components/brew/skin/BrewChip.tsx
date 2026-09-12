/** 只接订阅轨 flip。 */

import type { ReactNode } from 'react'
import { useCallback } from 'react'

import { playBrewSurfaceExit } from '../../../hooks/animation/pages/brewChipPresence'
import { useBrewWaveLane } from '../ui/Chip'
import { revealFeedsTree } from './flipCards'

export function BrewViewLane({
  children,
  wave,
  className,
  onDisplayed,
}: {
  children: ReactNode
  wave: string
  className?: string
  onDisplayed?: (wave: string) => void
}) {
  const play = useCallback((node: HTMLElement | null) => {
    revealFeedsTree(node)
    return playBrewSurfaceExit(node)
  }, [])
  const { rowRef, exiting, exitHow, shown, view } = useBrewWaveLane(
    wave,
    children,
    play,
    onDisplayed,
    true,
  )

  return (
    <div
      className={`brew-view-lane${className ? ` ${className}` : ''}`}
      ref={rowRef}
      data-chip-phase={exiting ? 'exit' : 'enter'}
      data-chip-exit={exiting ? exitHow : undefined}
      data-brew-view={shown}
    >
      {view}
    </div>
  )
}
