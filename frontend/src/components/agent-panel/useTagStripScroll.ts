/**
 * 中间贴条横滑：滚轮改左右、溢出时给两端淡、新贴进来滑到最右。
 */

import type { RefObject } from 'react'
import { useCallback, useEffect, useLayoutEffect, useRef } from 'react'
import { AGENT_ROW_MS } from './agentPresenceState'

export function tagStripOverflow(
  scrollLeft: number,
  scrollWidth: number,
  clientWidth: number,
): 'start' | 'end' | 'both' | '' {
  const max = scrollWidth - clientWidth
  if (max <= 1) return ''
  const start = scrollLeft > 1
  const end = scrollLeft < max - 1
  if (start && end) return 'both'
  if (start) return 'start'
  if (end) return 'end'
  return ''
}

function scrollTagStripEnd(node: HTMLElement | null): void {
  if (!node || node.scrollWidth <= node.clientWidth) return
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  node.scrollTo({
    left: node.scrollWidth,
    behavior: reduce ? 'auto' : 'smooth',
  })
}

function writeOverflow(node: HTMLElement | null): void {
  if (!node) return
  const side = tagStripOverflow(
    node.scrollLeft,
    node.scrollWidth,
    node.clientWidth,
  )
  if (side) node.dataset.overflow = side
  else delete node.dataset.overflow
}

export function useTagStripScroll(
  itemCount: number,
): RefObject<HTMLDivElement | null> {
  const ref = useRef<HTMLDivElement>(null)
  const previousCount = useRef(-1)

  const syncOverflow = useCallback(() => {
    writeOverflow(ref.current)
  }, [])

  useLayoutEffect(() => {
    const previous = previousCount.current
    previousCount.current = itemCount
    const node = ref.current
    if (previous >= 0 && itemCount > previous) {
      scrollTagStripEnd(node)
      const slide = window.setTimeout(scrollTagStripEnd, AGENT_ROW_MS, node)
      const edge = window.setTimeout(syncOverflow, AGENT_ROW_MS + 32)
      return () => {
        window.clearTimeout(slide)
        window.clearTimeout(edge)
      }
    }
    syncOverflow()
    return undefined
  }, [itemCount, syncOverflow])

  useEffect(() => {
    const node = ref.current
    if (!node) return
    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey) return
      if (node.scrollWidth <= node.clientWidth) return
      const delta =
        Math.abs(event.deltaY) >= Math.abs(event.deltaX)
          ? event.deltaY
          : event.deltaX
      if (delta === 0) return
      const max = node.scrollWidth - node.clientWidth
      const next = Math.max(0, Math.min(max, node.scrollLeft + delta))
      if (next === node.scrollLeft) return
      event.preventDefault()
      node.scrollLeft = next
      writeOverflow(node)
    }
    const onScroll = () => writeOverflow(node)
    node.addEventListener('wheel', onWheel, { passive: false })
    node.addEventListener('scroll', onScroll, { passive: true })
    const resize = new ResizeObserver(() => writeOverflow(node))
    resize.observe(node)
    writeOverflow(node)
    return () => {
      node.removeEventListener('wheel', onWheel)
      node.removeEventListener('scroll', onScroll)
      resize.disconnect()
    }
  }, [])

  return ref
}
