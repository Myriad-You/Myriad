import { useEffect, useRef, useState } from 'react'
import { REPORT_ITEM_DWELL_MS, useReportDetailPage } from './reportPaging'

/** 详情一页一项。首页由报告卡按页码给；嵌在别处时每次从总览翻到详情换下一项。 */
export function useLibraryItemRotation(libraryItems: any[], showOverview: boolean) {
  const paged = useReportDetailPage(libraryItems.length, REPORT_ITEM_DWELL_MS)
  const [currentItemIndex, setCurrentItemIndex] = useState(0)
  const prevShowOverviewRef = useRef(showOverview)

  useEffect(() => {
    if (
      paged === null &&
      prevShowOverviewRef.current &&
      !showOverview &&
      libraryItems.length > 0
    ) {
      setCurrentItemIndex((prev) => (prev + 1) % libraryItems.length)
    }
    prevShowOverviewRef.current = showOverview
  }, [paged, showOverview, libraryItems.length])

  const index = paged ?? currentItemIndex
  return { currentItem: libraryItems[index], currentItemIndex: index }
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
