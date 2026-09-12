import { useEffect, useRef, useState } from 'react'

export function useLibraryItemRotation(libraryItems: any[], showOverview: boolean) {
  const [currentItemIndex, setCurrentItemIndex] = useState(0)
  const prevShowOverviewRef = useRef(showOverview)

  useEffect(() => {
    if (
      prevShowOverviewRef.current &&
      !showOverview &&
      libraryItems.length > 0
    ) {
      setCurrentItemIndex((prev) => (prev + 1) % libraryItems.length)
    }
    prevShowOverviewRef.current = showOverview
  }, [showOverview, libraryItems.length])

  return { currentItem: libraryItems[currentItemIndex], currentItemIndex }
}

export function useCountUp(value: number, duration = 800, delay = 0) {
  const [display, setDisplay] = useState(() => (duration > 0 ? 0 : value))

  useEffect(() => {
    if (duration <= 0) {
      setDisplay(value)
      return
    }
    let raf = 0
    const start = performance.now() + delay
    let last = -1
    const tick = (now: number) => {
      const p = Math.min(Math.max((now - start) / duration, 0), 1)
      const eased = 1 - (1 - p) ** 3
      const next = Math.round(value * eased)
      if (next !== last) {
        last = next
        setDisplay(next)
      }
      if (p < 1) raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [value, duration, delay])

  return display
}
