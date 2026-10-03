import type { CSSProperties, MouseEvent, ReactNode, RefObject } from 'react'
import type { FeedStory } from '../logic/feedStories'
import type { TimeTranslations } from '../types'
import type { PhantasiRailApi } from './usePhantasiRailPan'

import { isValidElement, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { isPlainClick } from '../../../utils/plainClick'
import {
  clipPaintedBatches,
  storyColumnCount,
  storyPaintShouldRebuildBatch,
  storySlotsByColumn,
  syncPaintedRange,
} from '../logic/feedStories'
import { peekStoryNode } from '../ui/peekLane'
import { usePhantasiPeekLane } from '../ui/StoryCard'
import { PhantasiStoryColumn } from './PhantasiStory'
import {
  eagerStoryCovers,
  onStoryMediaError,
  paintStoryAway,
  RAIL_MOUNT_BOOT_TO,
  RAIL_MOUNT_GROW_AHEAD,
  RAIL_MOUNT_LIVE_PAD,
  RAIL_MOUNT_RESERVE,
  recycleStoryDomShellsOutside,
  storyMountWindow,
  storyRailTrackSize,
} from './railPan'
import { warmStoryCovers, warmStoryFaces } from './storyFace'

export interface StorySlot { story: FeedStory; column: number; row: 1 | 2 }

function storyAtRailTarget(
  target: EventTarget | null,
  byId: ReadonlyMap<number, FeedStory>,
): FeedStory | undefined {
  const node = peekStoryNode(target)
  if (!node) return
  const id = Number(node.dataset.railId)
  if (!Number.isFinite(id)) return
  return byId.get(id)
}

const PaintedRailHead = memo(({
  nodes,
}: {
  nodes: readonly ReactNode[]
}) => nodes)
PaintedRailHead.displayName = 'PaintedRailHead'

export const PhantasiFeedsStories = memo(({
  storySlots,
  times,
  locale,
  labels,
  onOpen,
  onPeek,
  onPeekEnd,
  onToggleStar,
  trackRef,
  setMountRef,
  setLiveRef,
  mountCommittedRef,
  mountColsRef,
  liveToRef,
  pendingStoryAlignRef,
  itemsApiRef,
  eagerBandRef,
  lastEagerRef,
  warmRef,
  grabbingRef,
}: {
  storySlots: readonly StorySlot[]
  times: TimeTranslations
  locale: string
  labels: {
    unread: string
    starred: string
    unstar: string
  }
  onOpen: (item: FeedStory) => void
  onPeek: (item: FeedStory) => void
  onPeekEnd: () => void
  onToggleStar?: (item: FeedStory) => void | false | Promise<void | false>
  trackRef: RefObject<HTMLDivElement | null>
  setMountRef: RefObject<(next: { from: number; to: number }) => void>
  setLiveRef: RefObject<(next: number) => void>
  mountCommittedRef: RefObject<{ from: number; to: number }>
  mountColsRef: RefObject<{ from: number; to: number }>
  liveToRef: RefObject<number>
  pendingStoryAlignRef: RefObject<number | null>
  itemsApiRef: RefObject<PhantasiRailApi | null>
  eagerBandRef: RefObject<{ from: number; to: number }>
  lastEagerRef: RefObject<{ from: number; to: number }>
  warmRef: RefObject<() => void>
  grabbingRef: RefObject<boolean>
}) => {
  const [mountCols, setMountCols] = useState({
    from: 1,
    to: Math.min(RAIL_MOUNT_GROW_AHEAD, RAIL_MOUNT_BOOT_TO),
  })
  const [liveTo, setLiveTo] = useState(liveToRef.current)
  const setMount = useCallback((next: { from: number; to: number }) => {
    mountColsRef.current = next
    mountCommittedRef.current = next
    setMountCols((prev) =>
      prev.from === next.from && prev.to === next.to ? prev : next,
    )
  }, [mountColsRef, mountCommittedRef])
  const setLive = useCallback((next: number) => {
    liveToRef.current = next
    setLiveTo((prev) => {
      if (prev === next) {
        if (!grabbingRef.current) {
          recycleStoryDomShellsOutside(
            trackRef.current,
            prev + 1,
            Number.POSITIVE_INFINITY,
          )
        }
        return prev
      }
      return next
    })
  }, [grabbingRef, liveToRef, trackRef])
  useLayoutEffect(() => {
    setMountRef.current = setMount
  }, [setMount, setMountRef])
  useLayoutEffect(() => {
    setLiveRef.current = setLive
  }, [setLive, setLiveRef])
  useLayoutEffect(() => {
    if (grabbingRef.current) return
    recycleStoryDomShellsOutside(
      trackRef.current,
      liveTo + 1,
      Number.POSITIVE_INFINITY,
    )
  }, [grabbingRef, liveTo, mountCols.from, mountCols.to, trackRef])
  const alignId = pendingStoryAlignRef.current
  useLayoutEffect(() => {
    if (alignId == null) return
    pendingStoryAlignRef.current = null
    itemsApiRef.current?.alignColumn(alignId, true)
  }, [alignId, itemsApiRef, pendingStoryAlignRef])
  const storyCols = useMemo(() => storyColumnCount(storySlots), [storySlots])
  const grid = storyMountWindow(mountCols.from, mountCols.to, storyCols)
  const byColRef = useRef<Map<number, StorySlot[]> | undefined>(undefined)
  const byCol = useMemo(() => {
    const next = storySlotsByColumn(storySlots, byColRef.current)
    byColRef.current = next
    return next
  }, [storySlots])
  const paintedEagerRef = useRef<{ from: number; to: number } | undefined>(
    undefined,
  )
  const prebuiltColsRef = useRef<
    Map<number, { node: ReactNode; slots: readonly StorySlot[] | undefined }>
  >(new Map())
  const canStar = onToggleStar != null
  useEffect(() => {
    const band = eagerBandRef.current
    eagerStoryCovers(
      trackRef.current,
      band.from,
      band.to,
      paintedEagerRef.current,
    )
    paintStoryAway(
      trackRef.current,
      band.from,
      band.to,
      paintedEagerRef.current,
      false,
    )
    paintedEagerRef.current = band
    lastEagerRef.current = band
  }, [eagerBandRef, lastEagerRef, mountCols.from, mountCols.to, storySlots, trackRef])
  useEffect(() => {
    const track = trackRef.current
    if (!track) return
    track.addEventListener('error', onStoryMediaError, true)
    return () => track.removeEventListener('error', onStoryMediaError, true)
  }, [trackRef])
  const eagerBand = eagerBandRef.current
  const paintTo = grid.to
  useEffect(() => {
    const from = paintTo + 1
    const faceTo = Math.min(paintTo + RAIL_MOUNT_GROW_AHEAD * 2, storyCols)
    const bootTo = Math.min(RAIL_MOUNT_GROW_AHEAD, RAIL_MOUNT_BOOT_TO)
    const warm = () => {
      const holdAt = eagerBandRef.current.to
      const prebuilt = prebuiltColsRef.current
      const fill = (colFrom: number, colTo: number, covers: boolean) => {
        if (colFrom > colTo) return
        const pack: FeedStory[] = []
        for (let col = colFrom; col <= colTo; col++) {
          const slots = byCol.get(col)
          if (!slots) continue
          for (const slot of slots) pack.push(slot.story)
        }
        warmStoryFaces(pack, times, locale, labels, !covers)
        if (!covers) return
        for (let col = colFrom; col <= colTo; col++) {
          if (prebuilt.has(col)) continue
          if (col <= holdAt) {
            const slots = byCol.get(col)
            prebuilt.set(col, {
              node: (
                <PhantasiStoryColumn
                  key={col}
                  col={col}
                  slots={slots}
                  times={times}
                  locale={locale}
                  labels={labels}
                  holdCover={false}
                  canStar={canStar}
                />
              ),
              slots,
            })
          }
        }
        warmStoryCovers(pack)
      }
      if (grabbingRef.current) {
        const liveNow = liveToRef.current
        fill(
          liveNow + 1,
          Math.min(
            liveNow + RAIL_MOUNT_LIVE_PAD + RAIL_MOUNT_RESERVE,
            mountColsRef.current.to,
            storyCols,
          ),
          false,
        )
      } else if (from <= faceTo) {
        fill(from, faceTo, true)
      }
      if (grabbingRef.current) return
      const mounted = mountColsRef.current
      if (mounted.to > bootTo + RAIL_MOUNT_LIVE_PAD) return
      const padTo = Math.min(bootTo + RAIL_MOUNT_LIVE_PAD, storyCols)
      if (padTo <= mounted.to) return
      const next = { from: mounted.from, to: padTo }
      mountColsRef.current = next
      liveToRef.current = padTo
      setLive(padTo)
      setMount(next)
    }
    warmRef.current = warm
    if (typeof requestIdleCallback === 'function') {
      const idle = requestIdleCallback(warm)
      return () => cancelIdleCallback(idle)
    }
    const timer = window.setTimeout(warm, 0)
    return () => window.clearTimeout(timer)
  }, [
    byCol,
    canStar,
    eagerBandRef,
    grabbingRef,
    labels,
    locale,
    liveToRef,
    mountColsRef,
    paintTo,
    setLive,
    setMount,
    storyCols,
    times,
    warmRef,
  ])
  const trackStyle = useMemo(
    () => ({
      '--phantasi-story-cols': storyCols,
      width: storyRailTrackSize(storyCols),
      gridTemplateColumns: 'minmax(0, 100%)',
    }) as CSSProperties,
    [storyCols],
  )
  const paintedCacheRef = useRef<ReactNode[]>([])
  const paintedOutRef = useRef<ReactNode[]>([])
  const paintedHeadRef = useRef<ReactNode>(null)
  const paintedBatchesRef = useRef<{
    from: number
    to: number
    nodes: readonly ReactNode[]
    head: ReactNode
  }[]>([])
  const paintedRangeRef = useRef({ from: 0, to: 0 })
  const paintedLiveToRef = useRef(0)
  const paintedEagerToRef = useRef(0)
  const paintedByColRef = useRef(byCol)
  const paintedFaceRef = useRef({
    times,
    locale,
    labels,
    canStar,
  })
  const painted = useMemo(() => {
    const face = paintedFaceRef.current
    const prevByCol = paintedByColRef.current
    const reset =
      face.times !== times
      || face.locale !== locale
      || face.labels !== labels
      || face.canStar !== canStar
    paintedByColRef.current = byCol
    paintedFaceRef.current = {
      times,
      locale,
      labels,
      canStar,
    }
    if (reset) prebuiltColsRef.current.clear()
    const prevRange = paintedRangeRef.current
    const prevLive = paintedLiveToRef.current
    const prevEagerTo = paintedEagerToRef.current
    const jumped =
      prevRange.from > 0
      && prevRange.to >= prevRange.from
      && (grid.from > prevRange.to || grid.to < prevRange.from)
    if (jumped) {
      prebuiltColsRef.current.clear()
      paintedBatchesRef.current = []
      paintedHeadRef.current = null
      paintedOutRef.current = []
      paintedLiveToRef.current = 0
      paintedEagerToRef.current = 0
      paintedRangeRef.current = { from: 0, to: 0 }
    }
    const prev = paintedCacheRef.current
    const prebuilt = prebuiltColsRef.current
    const paintLive = Math.min(liveTo, paintTo)
    const eagerTo = eagerBand.to
    const prevPaint = {
      from: prevRange.from,
      to: prevRange.to,
      liveTo: prevLive,
      eagerTo: prevEagerTo,
    }
    const nextPaint = {
      from: grid.from,
      to: paintTo,
      liveTo: paintLive,
      eagerTo,
    }
    const makeFull = (col: number) => {
      const holdCover = col > eagerTo
      const slots = byCol.get(col)
      if (!holdCover) {
        const hit = prebuilt.get(col)
        if (hit && hit.slots === slots) return hit.node
      } else {
        prebuilt.delete(col)
      }
      const node = (
        <PhantasiStoryColumn
          key={col}
          col={col}
          slots={slots}
          times={times}
          locale={locale}
          labels={labels}
          holdCover={holdCover}
          canStar={canStar}
        />
      )
      if (!holdCover) prebuilt.set(col, { node, slots })
      return node
    }
    const make = (col: number) => (
      col > paintLive
        ? (
            <PhantasiStoryColumn
              key={col}
              col={col}
              times={times}
              locale={locale}
              labels={labels}
              holdCover={col > eagerTo}
              canStar={canStar}
            />
          )
        : makeFull(col)
    )
    const { nodes: next, remadeOverlap } = syncPaintedRange(
      prev,
      prevPaint,
      nextPaint,
      make,
      reset || jumped || prevByCol === byCol
        ? undefined
        : (col) => prevByCol.get(col) !== byCol.get(col),
      reset || jumped,
    )
    const leftoverShells = paintedBatchesRef.current.some(
      (batch) => batch.from > prevLive || batch.to > prevLive,
    )
    paintedRangeRef.current = { from: grid.from, to: paintTo }
    paintedLiveToRef.current = paintLive
    paintedEagerToRef.current = eagerTo
    paintedCacheRef.current = next
    if (next === prev && paintedOutRef.current.length > 0) {
      return paintedOutRef.current
    }
    const grew =
      !reset
      && paintedHeadRef.current
      && grid.from === prevRange.from
      && paintTo > prevRange.to
      && next.length > prev.length
      && next[0] === prev[0]
    if (grew && !leftoverShells && !remadeOverlap) {
      const addFrom = prevRange.to + 1
      let out = paintedOutRef.current
      if (addFrom <= paintLive) {
        const liveNodes = next.slice(
          addFrom - grid.from,
          paintLive - grid.from + 1,
        )
        const head = (
          <PaintedRailHead
            key={`${prevRange.to}:${paintTo}`}
            nodes={liveNodes}
          />
        )
        paintedHeadRef.current = head
        out = out.concat(head)
        paintedBatchesRef.current.push({
          from: addFrom,
          to: paintLive,
          nodes: liveNodes,
          head,
        })
      }
      if (paintLive < paintTo) {
        const shellNodes = next.slice(paintLive - grid.from + 1)
        const head = (
          <PaintedRailHead
            key={`shell:${paintTo}`}
            nodes={shellNodes}
          />
        )
        if (addFrom > paintLive) paintedHeadRef.current = head
        out = out.concat(head)
        paintedBatchesRef.current.push({
          from: paintLive + 1,
          to: paintTo,
          nodes: shellNodes,
          head,
        })
      }
      paintedOutRef.current = out
      return out
    }
    if (
      storyPaintShouldRebuildBatch(
        prevPaint,
        nextPaint,
        remadeOverlap,
        leftoverShells,
      )
      && paintedBatchesRef.current.length > 0
    ) {
      const firstHead = paintedBatchesRef.current[0]?.head
      const headKey =
        grid.from === prevRange.from
        && isValidElement(firstHead)
        && firstHead.key != null
          ? firstHead.key
          : `${grid.from}:${paintTo}`
      const head = (
        <PaintedRailHead key={headKey} nodes={next} />
      )
      paintedHeadRef.current = head
      paintedOutRef.current = [head]
      paintedBatchesRef.current = [{
        from: grid.from,
        to: paintTo,
        nodes: next,
        head,
      }]
      return paintedOutRef.current
    }
    if (reset || !paintedHeadRef.current || paintedBatchesRef.current.length === 0) {
      const head = <PaintedRailHead key={`${grid.from}:${paintTo}`} nodes={next} />
      paintedHeadRef.current = head
      paintedOutRef.current = [head]
      paintedBatchesRef.current = [{
        from: grid.from,
        to: paintTo,
        nodes: next,
        head,
      }]
      return paintedOutRef.current
    }
    const clipped = clipPaintedBatches(
      paintedBatchesRef.current,
      grid.from,
      paintTo,
      next,
    )
    const heads: ReactNode[] = []
    const batches: {
      from: number
      to: number
      nodes: readonly ReactNode[]
      head: ReactNode
    }[] = []
    for (const batch of clipped) {
      const head = batch.keep?.head ?? (
        <PaintedRailHead
          key={`${batch.from}:${batch.to}`}
          nodes={batch.nodes}
        />
      )
      heads.push(head)
      batches.push({
        from: batch.from,
        to: batch.to,
        nodes: batch.nodes,
        head,
      })
    }
    paintedHeadRef.current = heads[0] ?? null
    paintedOutRef.current = heads
    paintedBatchesRef.current = batches
    return heads
  }, [
    byCol,
    canStar,
    eagerBand.to,
    grid.from,
    labels,
    liveTo,
    locale,
    paintTo,
    times,
  ])
  const storyByIdRef = useRef<Map<number, FeedStory>>(new Map())
  const storySlotsSeenRef = useRef<typeof storySlots | null>(null)
  if (storySlotsSeenRef.current !== storySlots) {
    storySlotsSeenRef.current = storySlots
    const map = new Map<number, FeedStory>()
    for (const slot of storySlots) map.set(slot.story.id, slot.story)
    storyByIdRef.current = map
  }
  const onOpenRef = useRef(onOpen)
  onOpenRef.current = onOpen
  const onPeekRef = useRef(onPeek)
  onPeekRef.current = onPeek
  const onStarRef = useRef(onToggleStar)
  onStarRef.current = onToggleStar
  const peekLane = usePhantasiPeekLane({
    onPeek: (item) => {
      const story = storyByIdRef.current.get(item.id)
      if (story) onPeekRef.current(story)
    },
    onPeekEnd,
    blocked: () => grabbingRef.current,
  })
  const onTrackClick = useCallback((event: MouseEvent<HTMLDivElement>) => {
    const star = event.target instanceof Element
      ? event.target.closest('.phantasi-story__star')
      : null
    if (star) {
      // 卡片是链接：点星标不能顺带跳走。
      event.preventDefault()
      const story = storyAtRailTarget(star, storyByIdRef.current)
      if (story) onStarRef.current?.(story)
      return
    }
    const hit = event.target instanceof Element
      ? event.target.closest('.phantasi-story__hit')
      : null
    if (!hit) return
    // 带修饰键或中键（新标签、新窗口）交给链接本身。
    if (!isPlainClick(event)) return
    const story = storyAtRailTarget(hit, storyByIdRef.current)
    if (!story) return
    event.preventDefault()
    onOpenRef.current(story)
  }, [])
  return (
    <div
      className="phantasi-feeds__items-track"
      ref={trackRef}
      data-phantasi-rail-track="items"
      style={trackStyle}
      onClick={onTrackClick}
      {...peekLane}
    >
      {painted}
    </div>
  )
})
