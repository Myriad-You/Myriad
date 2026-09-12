/** 不改 veil 本身。 */

import { useEffect } from 'react'
import { playBrewVeilEnter, playBrewVeilExit } from '../../hooks/animation'

export function useBrewSurface(loading: boolean, lockViewport: boolean) {
  useEffect(() => {
    if (loading) return
    playBrewVeilEnter()
    return () => {
      playBrewVeilExit()
    }
  }, [loading])

  useEffect(() => {
    if (!lockViewport) return
    const root = document.documentElement
    const previous = root.style.overflow
    root.style.overflow = 'hidden'
    return () => {
      root.style.overflow = previous
    }
  }, [lockViewport])
}
