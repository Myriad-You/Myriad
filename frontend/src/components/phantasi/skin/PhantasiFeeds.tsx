import type { ReactNode } from 'react'
import type { PhantasiItemPreview, PhantasiSource } from '../../../types/phantasi'
import type { FeedStory } from '../logic/feedStories'
import type { PeekStoryPreview } from '../ui/peekLane'
import type { StorySlot } from './PhantasiFeedsStories'

import type { PhantasiRailApi } from './usePhantasiRailPan'
import { memo, useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { getIconUrl, normalizeThemeColor } from '../constants'
import {
  firstStoryForRailGroup,
  isAggregateFeedId,
  isLatestFeedId,
  LATEST_FEED_ID,
  LATEST_FEED_STACK_POOL,
  latestFeedStackFaces,
  railGroupSourceCount,
  sourceColumnStarts,
  sourceScrollStarts,
  storyColumnCount,
  storyColumnLeads,
  storyColumnShift,
  storyRailGroup,
  storyRailSlots,
  storySlotAtColumn,
  storySlotsByColumn,
  topicFeedId,
} from '../logic/feedStories'
import { topicDisplayName } from '../logic/topics'
import { PhantasiVacant } from '../ui/Empty'
import { PhantasiRailTitle } from '../ui/PhantasiRailTitle'
import { clearPhantasiStoryPeeks, schedulePhantasiPeekResume } from '../ui/StoryCard'
import { paintSiteInk, paintSiteOn, PhantasiFeedsSites } from './PhantasiFeedsSites'
import { PhantasiFeedsStories } from './PhantasiFeedsStories'
import {
  dropStoryDomShells,
  eagerStoryCovers,
  ensureStoryShells,
  followRailScroll,
  paintStoryAway,
  paintStoryLiveCols,
  RAIL_MOUNT_BOOT_TO,
  RAIL_MOUNT_GRAB_AHEAD,
  RAIL_MOUNT_GROW_AHEAD,
  RAIL_MOUNT_LIVE_PAD,
  RAIL_MOUNT_RESERVE,
  railCardOffset,
  railLeadColumn,
  railLiveTo,
  railMountColumns,
  railMountColumnsCovered,
  railMountColumnsPan,
  railTrackScroll,
  recycleStoryDomShellsOutside,
  sourceAtScroll,
} from './railPan'
import { phantasiRelativeTime, usePhantasiTimes } from './time'
import { usePhantasiRailPan } from './usePhantasiRailPan'

const ARTICLES_SETTLE_MS = 200

interface PhantasiFeedsProps {
  sources: PhantasiSource[]
  focusSourceId?: number | null
  isEditMode?: boolean
  selectedIds?: Set<number>
  onToggleSelect?: (id: number) => void
  onOpenItem?: (item: PhantasiItemPreview, source: PhantasiSource) => void
  onPeekItem?: (item: PeekStoryPreview) => void
  onPeekEnd?: () => void
  onToggleStar?: (item: PhantasiItemPreview) => void | false | Promise<void | false>
  onEditSource?: (source: PhantasiSource) => void
  toolbar?: ReactNode
  vacant?: ReactNode
  stories?: FeedStory[]
  onJumpSource?: (sourceId: number) => void
  onHoldStories?: () => void
  onReleaseStories?: () => void
  railEpoch?: number | string
  onReadySource?: (id: number | null) => void
  onRailFocus?: (sourceId: number | null) => void
  sourceTags?: ReactNode
  topicCards?: string[]
}

function PhantasiFeeds({
  sources,
  focusSourceId,
  isEditMode = false,
  selectedIds,
  onToggleSelect,
  onOpenItem,
  onPeekItem,
  onPeekEnd,
  onToggleStar,
  onEditSource,
  toolbar,
  vacant,
  stories = [],
  onJumpSource,
  onHoldStories,
  onReleaseStories,
  railEpoch = 0,
  onReadySource,
  onRailFocus,
  sourceTags,
  topicCards = [],
}: PhantasiFeedsProps) {
  const { t, locale, format } = useI18n()
  const sitesTitleId = useId()
  const itemsTitleId = useId()
  const feedsRef = useRef<HTMLDivElement>(null)
  const sitesViewRef = useRef<HTMLDivElement>(null)
  const sitesTrackRef = useRef<HTMLDivElement>(null)
  const sitesApiRef = useRef<PhantasiRailApi | null>(null)
  const itemsViewRef = useRef<HTMLDivElement>(null)
  const itemsTrackRef = useRef<HTMLDivElement>(null)
  const itemsApiRef = useRef<PhantasiRailApi | null>(null)
  const skipStoryAlignRef = useRef(false)
  const pendingStoryAlignRef = useRef<number | null>(null)
  const pendingSourceAlignRef = useRef<number | null>(null)
  const railDriverRef = useRef<'sites' | 'stories' | null>(null)
  const [focusId, setFocusId] = useState<number | null>(
    sources.length > 0 ? LATEST_FEED_ID : null,
  )
  const [readyId, setReadyId] = useState<number | null>(
    sources.length > 0 ? LATEST_FEED_ID : null,
  )
  const lastSourceRef = useRef<number | null>(
    sources.length > 0 ? LATEST_FEED_ID : null,
  )
  const paintedOnRef = useRef<number | null>(
    sources.length > 0 ? LATEST_FEED_ID : null,
  )
  const paintedElRef = useRef<HTMLElement | null>(null)
  const siteElsRef = useRef(new Map<number, HTMLElement>())
  const focusTimerRef = useRef(0)
  const growFrameRef = useRef(0)
  const lastEagerRef = useRef<{ from: number; to: number }>({ from: 1, to: 8 })
  const storyWarmRef = useRef<() => void>(() => {})
  const grabbingRef = useRef(false)
  const appliedFocus = useRef<number | null>(null)
  const seatSitesRef = useRef<number | null>(null)
  const phantasiLabels = t.phantasi
  const times = usePhantasiTimes()
  const storyPaintRef = useRef({
    times,
    locale,
    labels: phantasiLabels,
    canStar: onToggleStar != null,
  })
  storyPaintRef.current = {
    times,
    locale,
    labels: phantasiLabels,
    canStar: onToggleStar != null,
  }

  const focus = useMemo(
    () => sources.find((s) => s.id === focusId) ?? null,
    [sources, focusId],
  )
  const ready = useMemo(
    () => sources.find((s) => s.id === readyId) ?? focus,
    [sources, readyId, focus],
  )

  useEffect(() => {
    if (!sources.length) {
      lastSourceRef.current = null
      paintedOnRef.current = null
      paintedElRef.current = null
      setFocusId(null)
      setReadyId(null)
      return
    }
    if (
      focusSourceId &&
      appliedFocus.current !== focusSourceId &&
      sources.some((s) => s.id === focusSourceId)
    ) {
      appliedFocus.current = focusSourceId
      railDriverRef.current = 'sites'
      lastSourceRef.current = focusSourceId
      paintedOnRef.current = focusSourceId
      paintedElRef.current = null
      setFocusId(focusSourceId)
      setReadyId(focusSourceId)
      onJumpSource?.(focusSourceId)
      seatSitesRef.current = focusSourceId
      return
    }
    if (
      focusId == null ||
      (!isLatestFeedId(focusId) &&
        !topicCards.some((_, index) => topicFeedId(index) === focusId) &&
        !sources.some((s) => s.id === focusId))
    ) {
      lastSourceRef.current = LATEST_FEED_ID
      paintedOnRef.current = LATEST_FEED_ID
      paintedElRef.current = null
      setFocusId(LATEST_FEED_ID)
      setReadyId(LATEST_FEED_ID)
    }
  }, [sources, focusId, focusSourceId, onJumpSource, topicCards])

  useLayoutEffect(() => {
    const id = seatSitesRef.current
    if (id == null || id !== focusId) return
    seatSitesRef.current = null
    sitesApiRef.current?.align(id, true)
  }, [focusId])

  useEffect(() => {
    if (focusId == null || focusId === readyId) return
    const timer = window.setTimeout(setReadyId, ARTICLES_SETTLE_MS, focusId)
    return () => window.clearTimeout(timer)
  }, [focusId, readyId])

  useEffect(() => {
    onReadySource?.(readyId)
  }, [onReadySource, readyId])

  const siteKey = useMemo(
    () =>
      sources.length > 0
        ? `${LATEST_FEED_ID},${topicCards.join('|')},${sources.map((source) => source.id).join(',')}`
        : '',
    [sources, topicCards],
  )
  const inboxLead = useMemo(
    () => firstStoryForRailGroup(stories, LATEST_FEED_ID),
    [stories],
  )
  const inbox = useMemo(() => {
    if (sources.length === 0) return null
    return {
      name: phantasiLabels.latestFeed,
      description: format(phantasiLabels.topicSourceCount, { count: sources.length }),
      latestTitle: inboxLead?.title,
      latestWhen: inboxLead
        ? phantasiRelativeTime(inboxLead.published_at, times, locale)
        : '',
      stack: latestFeedStackFaces(sources, stories).map((face) => ({
        key: face.key,
        src: getIconUrl(face.src),
        mark: face.mark,
        ink: normalizeThemeColor(face.ink),
      })),
    }
  }, [phantasiLabels, format, inboxLead, locale, sources, stories, times])

  const topicMixes = useMemo(() => {
    if (sources.length === 0) return []
    return topicCards.map((name, index) => {
      const id = topicFeedId(index)
      const lead = firstStoryForRailGroup(stories, id)
      return {
        id,
        name: topicDisplayName({ key: name }, phantasiLabels),
        description: format(phantasiLabels.topicSourceCount, {
          count: railGroupSourceCount(stories, id),
        }),
        latestTitle: lead?.title,
        latestWhen: lead
          ? phantasiRelativeTime(lead.published_at, times, locale)
          : '',
        stack: latestFeedStackFaces(
          sources,
          stories,
          LATEST_FEED_STACK_POOL,
          id,
        ).map((face) => ({
          key: face.key,
          src: getIconUrl(face.src),
          mark: face.mark,
          ink: normalizeThemeColor(face.ink),
        })),
      }
    })
  }, [format, locale, phantasiLabels, sources, stories, times, topicCards])

  const onLeadChange = useCallback((id: number) => {
    lastSourceRef.current = id
    paintSiteOn(
      sitesTrackRef.current,
      id,
      paintedOnRef,
      paintedElRef,
      siteElsRef.current,
    )
  }, [])

  useEffect(() => {
    return () => {
      if (focusTimerRef.current) window.clearTimeout(focusTimerRef.current)
      if (growFrameRef.current) window.cancelAnimationFrame(growFrameRef.current)
    }
  }, [])
  const storiesRef = useRef(stories)
  const storyIndexRef = useRef<Map<number, number>>(new Map())
  if (storyIndexRef.current.size === 0 || storiesRef.current !== stories) {
    const map = new Map<number, number>()
    for (let i = 0; i < stories.length; i++) map.set(stories[i].id, i)
    storyIndexRef.current = map
  }
  storiesRef.current = stories
  const sourcesRef = useRef(sources)
  sourcesRef.current = sources
  const activateSiteRef = useRef<(id: number) => void>(() => {})
  const onEditSourceRef = useRef(onEditSource)
  onEditSourceRef.current = onEditSource
  const onActivateSite = useCallback((id: number | string) => {
    activateSiteRef.current(Number(id))
  }, [])
  const onEditSite = useCallback((id: number | string) => {
    const source = sourcesRef.current.find((entry) => entry.id === Number(id))
    if (source) onEditSourceRef.current?.(source)
  }, [])
  const onIconLoadSite = useCallback((img: HTMLImageElement) => {
    const id = Number(img.closest('.phantasi-site')?.getAttribute('data-rail-id'))
    const theme = sourcesRef.current.find((entry) => entry.id === id)?.theme_color
    const idle = window.requestIdleCallback
    if (idle) {
      idle(() => paintSiteInk(img, theme ?? null), { timeout: 800 })
      return
    }
    window.setTimeout(paintSiteInk, 0, img, theme ?? null)
  }, [])
  const slotCacheRef = useRef<StorySlot[]>([])
  const storySlots = useMemo(() => {
    const next = storyRailSlots(stories, slotCacheRef.current)
    slotCacheRef.current = next
    return next
  }, [stories])
  const storyCols = useMemo(() => storyColumnCount(storySlots), [storySlots])
  const storyColsRef = useRef(storyCols)
  storyColsRef.current = storyCols
  const colCacheRef = useRef<Array<{ id: number; column: number }>>([])
  const sourceCols = useMemo(() => {
    const next = sourceColumnStarts(storySlots, colCacheRef.current)
    colCacheRef.current = next
    return next
  }, [storySlots])
  const sourceColsRef = useRef(sourceCols)
  sourceColsRef.current = sourceCols
  const storySlotsRef = useRef(storySlots)
  storySlotsRef.current = storySlots
  const colLeadRef = useRef(new Map<number, number>())
  const colLeadSlotsRef = useRef<readonly StorySlot[] | null>(null)
  const storyByColRef = useRef(new Map<number, StorySlot[]>())
  if (colLeadSlotsRef.current !== storySlots) {
    colLeadSlotsRef.current = storySlots
    colLeadRef.current = storyColumnLeads(storySlots)
    storyByColRef.current = storySlotsByColumn(storySlots, storyByColRef.current)
  }
  const driveStopsRef = useRef<number[]>([])
  const followStopsRef = useRef<number[]>([])
  const sourceStartsRef = useRef<Array<{ id: number; start: number }>>([])
  const stopsColsRef = useRef(sourceCols)
  const stopsColWRef = useRef(0)
  const colWRef = useRef(276)
  const lastSeekRef = useRef(0)
  const followHintRef = useRef({ i: 0 })
  const siteFollowHintRef = useRef({ i: 0 })
  const lastScrollRef = useRef(0)
  const lastViewWRef = useRef(800)
  const siteCardsRef = useRef<Array<{ id: number; left: number }>>([])
  const siteIndexRef = useRef<Map<number, number>>(new Map())
  const siteIndexCardsRef = useRef(siteCardsRef.current)
  const lastSiteViewWRef = useRef(-1)
  const siteCardsMissRef = useRef(false)
  const eagerBandRef = useRef({ from: 1, to: 8 })
  const storyColRef = useRef(0)
  const lastWindowViewRef = useRef(-1)
  const lastWindowColWRef = useRef(-1)
  const siteKeyRef = useRef(siteKey)
  if (siteKeyRef.current !== siteKey) {
    siteKeyRef.current = siteKey
    siteCardsRef.current = []
    lastSiteViewWRef.current = -1
    siteCardsMissRef.current = false
    siteElsRef.current.clear()
  }
  const rebuildFollowStops = useCallback((colW: number, viewW?: number) => {
    const columns = sourceColsRef.current
    const sites = sitesApiRef.current
    const knownView = lastSiteViewWRef.current
    const viewChanged =
      viewW != null
      && knownView >= 0
      && Math.abs(knownView - viewW) > 8
    if (viewChanged) siteCardsMissRef.current = false
    if (
      sites
      && (
        siteCardsRef.current.length === 0
        || viewChanged
      )
      && !(siteCardsRef.current.length === 0 && siteCardsMissRef.current)
    ) {
      siteCardsRef.current = sites.cards(viewChanged)
      siteCardsMissRef.current = siteCardsRef.current.length === 0
      if (viewW != null) lastSiteViewWRef.current = viewW
    } else if (viewW != null && knownView < 0) {
      lastSiteViewWRef.current = viewW
    }
    const sameDrive =
      stopsColsRef.current === columns
      && Math.abs(stopsColWRef.current - colW) <= 0.5
      && sourceStartsRef.current.length === columns.length
      && driveStopsRef.current.length === columns.length
    stopsColsRef.current = columns
    stopsColWRef.current = colW
    if (!sameDrive) {
      const starts = sourceScrollStarts(columns, colW)
      driveStopsRef.current = starts.map((block) => block.start)
      sourceStartsRef.current = starts
    }
    const siteCards = siteCardsRef.current
    const cardsChanged = siteIndexCardsRef.current !== siteCards
    if (cardsChanged) {
      siteIndexCardsRef.current = siteCards
      const map = new Map<number, number>()
      for (let i = 0; i < siteCards.length; i++) {
        const id = siteCards[i]?.id
        if (id != null) map.set(id, i)
      }
      siteIndexRef.current = map
    }
    if (
      sameDrive
      && !cardsChanged
      && followStopsRef.current.length === columns.length
    ) {
      return
    }
    const siteIndex = siteIndexRef.current
    const prevFollow = followStopsRef.current
    followStopsRef.current = columns.map((block, i) => {
      const at = siteIndex.get(block.id)
      if (at == null) return prevFollow[i] ?? 0
      return railCardOffset(siteCards, at)
    })
  }, [])
  useLayoutEffect(() => {
    rebuildFollowStops(colWRef.current)
  }, [rebuildFollowStops, sourceCols])
  const mountColsRef = useRef({
    from: 1,
    to: Math.min(RAIL_MOUNT_GROW_AHEAD, RAIL_MOUNT_BOOT_TO),
  })
  const storyMountCommittedRef = useRef(mountColsRef.current)
  const storySetMountRef = useRef<(next: { from: number; to: number }) => void>(
    () => {},
  )
  const liveToRef = useRef(Math.min(RAIL_MOUNT_GROW_AHEAD, RAIL_MOUNT_BOOT_TO))
  const storySetLiveRef = useRef<(next: number) => void>(() => {})
  const holdStoriesRef = useRef(onHoldStories)
  holdStoriesRef.current = onHoldStories
  const releaseStoriesRef = useRef(onReleaseStories)
  releaseStoriesRef.current = onReleaseStories
  const openArticleRef = useRef<
    (item: PhantasiItemPreview & { source_id?: number }, source?: PhantasiSource) => void
  >(() => {})
  const onOpenStory = useCallback((item: FeedStory) => {
    openArticleRef.current(item)
  }, [])
  const onPeekEndRef = useRef(onPeekEnd)
  onPeekEndRef.current = onPeekEnd
  const dropPeek = () => {
    clearPhantasiStoryPeeks(itemsTrackRef.current)
    onPeekEndRef.current?.()
  }
  const onPeekStory = useCallback((item: FeedStory) => {
    if (grabbingRef.current) return
    onPeekItem?.(item)
  }, [onPeekItem])
  const onPeekEndStory = useCallback(() => {
    if (grabbingRef.current) return
    onPeekEnd?.()
  }, [onPeekEnd])
  const onToggleStarRef = useRef(onToggleStar)
  onToggleStarRef.current = onToggleStar
  const onStarStory = useCallback((item: FeedStory) => {
    return onToggleStarRef.current?.(item)
  }, [])
  const grabFillRef = useRef({ armed: false, from: 0, to: -1, frame: 0 })
  useEffect(() => {
    return () => {
      if (!grabFillRef.current.frame) return
      window.cancelAnimationFrame(grabFillRef.current.frame)
      grabFillRef.current.frame = 0
      grabFillRef.current.armed = false
    }
  }, [])
  const paintGrabLive = (from: number, to: number) => {
    if (from > to) return
    const paintTo = Math.min(to, eagerBandRef.current.to)
    if (from > paintTo) return
    const paint = storyPaintRef.current
    const ready = ensureStoryShells(
      itemsTrackRef.current,
      from,
      paintTo,
      storySlotsRef.current,
    )
    paintStoryLiveCols(
      itemsTrackRef.current,
      from,
      paintTo,
      storyByColRef.current,
      paint.times,
      paint.locale,
      paint.labels,
      eagerBandRef.current.to,
      paint.canStar,
      true,
      ready,
    )
  }
  const fillGrabLive = (from: number, to: number) => {
    if (from > to) return
    const q = grabFillRef.current
    if (q.armed) {
      q.from = Math.min(q.from, from)
      q.to = Math.max(q.to, to)
      return
    }
    q.armed = true
    q.from = from
    q.to = to
    q.frame = window.requestAnimationFrame(() => {
      q.frame = 0
      q.armed = false
      const a = q.from
      const b = q.to
      q.to = -1
      paintGrabLive(a, b)
    })
  }
  const flushGrabFill = () => {
    const q = grabFillRef.current
    if (q.frame) {
      window.cancelAnimationFrame(q.frame)
      q.frame = 0
    }
    if (!q.armed) return
    q.armed = false
    const a = q.from
    const b = q.to
    q.to = -1
    paintGrabLive(a, b)
  }
  const primeStoryMount = () => {
    const colW = colWRef.current
    if (colW <= 1) return
    const totalCols = storyColsRef.current
    const viewW =
      lastViewWRef.current || itemsViewRef.current?.clientWidth || 800
    const ideal = railMountColumns(
      lastScrollRef.current,
      viewW,
      colW,
      totalCols,
    )
    const prevBand = eagerBandRef.current
    eagerBandRef.current = {
      from: Math.min(prevBand.from, ideal.from),
      to: Math.max(prevBand.to, ideal.to),
    }
    const prev = mountColsRef.current
    const next = {
      from: Math.max(1, Math.min(prev.from, ideal.from)),
      to: Math.min(totalCols, Math.max(prev.to, ideal.to + RAIL_MOUNT_GRAB_AHEAD)),
    }
    if (next.from !== prev.from || next.to !== prev.to) {
      mountColsRef.current = next
    }
    const prevLive = liveToRef.current
    liveToRef.current = railLiveTo(ideal.to, next.to, prevLive)
    paintGrabLive(Math.min(prevLive + 1, ideal.from), liveToRef.current)
  }
  const onStoryScroll = useCallback(
    (state: { scroll: number; viewW: number; colW: number }) => {
      if (state.colW > 1) colWRef.current = state.colW
      lastScrollRef.current = state.scroll
      lastViewWRef.current = state.viewW
      const colW = colWRef.current
      const columns = sourceColsRef.current
      const sites = sitesApiRef.current
      let needStops =
        stopsColsRef.current !== columns
        || driveStopsRef.current.length !== columns.length
        || Math.abs(stopsColWRef.current - colW) > 0.5
      if (
        sites
        && (
          (
            siteCardsRef.current.length === 0
            && !siteCardsMissRef.current
          )
          || (
            lastSiteViewWRef.current >= 0
            && Math.abs(lastSiteViewWRef.current - state.viewW) > 8
          )
        )
      ) {
        needStops = true
      } else if (lastSiteViewWRef.current < 0) {
        lastSiteViewWRef.current = state.viewW
      }
      if (needStops) {
        rebuildFollowStops(colW, state.viewW)
        followHintRef.current.i = 0
      }
      if (grabbingRef.current && railDriverRef.current === 'stories') {
        const siteX = followRailScroll(
          state.scroll,
          driveStopsRef.current,
          followStopsRef.current,
          followHintRef.current,
        )
        if (Math.abs(siteX - lastSeekRef.current) > 0.5) {
          lastSeekRef.current = siteX
          sitesApiRef.current?.seek(siteX)
        }
      }
      const col = railLeadColumn(state.scroll, colW)
      if (
        col === storyColRef.current
        && Math.abs(state.viewW - lastWindowViewRef.current) <= 8
        && Math.abs(colW - lastWindowColWRef.current) <= 0.5
      ) {
        return
      }
      storyColRef.current = col
      lastWindowViewRef.current = state.viewW
      lastWindowColWRef.current = colW
      const lead = storySlotAtColumn(
        storySlotsRef.current,
        col,
        colLeadRef.current,
      )
      const totalCols = storyColsRef.current
      const ideal = railMountColumns(
        state.scroll,
        state.viewW,
        colW,
        totalCols,
      )
      if (
        ideal.from !== eagerBandRef.current.from
        || ideal.to !== eagerBandRef.current.to
      ) {
        const prevBand = eagerBandRef.current
        eagerBandRef.current = ideal
        const mounted = mountColsRef.current
        const eagerFrom = Math.max(ideal.from, mounted.from)
        const eagerTo = Math.min(ideal.to, mounted.to)
        paintStoryAway(
          itemsTrackRef.current,
          ideal.from,
          ideal.to,
          prevBand,
          !grabbingRef.current,
        )
        if (grabbingRef.current) {
          recycleStoryDomShellsOutside(
            itemsTrackRef.current,
            ideal.from,
            ideal.to,
          )
          if (ideal.to > prevBand.to) {
            fillGrabLive(prevBand.to + 1, ideal.to)
          }
          if (ideal.from < prevBand.from) {
            fillGrabLive(ideal.from, prevBand.from - 1)
          }
        }
        if (!grabbingRef.current) {
          eagerStoryCovers(
            itemsTrackRef.current,
            eagerFrom,
            eagerTo,
            prevBand,
          )
          lastEagerRef.current = { from: eagerFrom, to: eagerTo }
        }
      }
      const prevCols = mountColsRef.current
      const grown = railMountColumnsCovered(
        prevCols,
        ideal,
        totalCols,
        Number.POSITIVE_INFINITY,
        RAIL_MOUNT_RESERVE,
      )
        ? prevCols
        : railMountColumnsPan(
            prevCols,
            state.scroll,
            state.viewW,
            colW,
            totalCols,
          )
      if (grown.from !== prevCols.from || grown.to !== prevCols.to) {
        const prevLive = liveToRef.current
        mountColsRef.current = grown
        liveToRef.current = railLiveTo(
          ideal.to,
          grown.to,
          liveToRef.current,
        )
        if (grabbingRef.current) {
          fillGrabLive(prevLive + 1, liveToRef.current)
        }
      }
      if (ideal.to + RAIL_MOUNT_RESERVE > liveToRef.current) {
        const prevLive = liveToRef.current
        const nextLive = railLiveTo(
          ideal.to,
          mountColsRef.current.to,
          liveToRef.current,
        )
        if (nextLive > prevLive) {
          liveToRef.current = nextLive
          if (grabbingRef.current) {
            fillGrabLive(prevLive + 1, nextLive)
          }
        }
      }
      if (railDriverRef.current === 'sites') return
      const sourceId =
        (lead ? storyRailGroup(lead.story) : undefined)
        ?? sourceAtScroll(state.scroll, sourceStartsRef.current)
      if (sourceId != null && sourceId !== lastSourceRef.current) {
        lastSourceRef.current = sourceId
        paintSiteOn(
          sitesTrackRef.current,
          sourceId,
          paintedOnRef,
          paintedElRef,
          siteElsRef.current,
        )
        skipStoryAlignRef.current = true
      }
    },
    [rebuildFollowStops],
  )
  const onSiteScroll = useCallback(
    (state: { scroll: number; viewW: number; colW: number }) => {
      if (railDriverRef.current !== 'sites') return
      if (!grabbingRef.current) return
      const colW = colWRef.current
      if (
        driveStopsRef.current.length === 0
        || followStopsRef.current.length === 0
      ) {
        rebuildFollowStops(colW, state.viewW)
      }
      const storyX = followRailScroll(
        state.scroll,
        followStopsRef.current,
        driveStopsRef.current,
        siteFollowHintRef.current,
      )
      if (
        grabbingRef.current
        && Math.abs(storyX - lastScrollRef.current) > 0.5
      ) {
        itemsApiRef.current?.seek(storyX)
      }
      const viewW =
        lastViewWRef.current
        || itemsViewRef.current?.clientWidth
        || state.viewW
      lastScrollRef.current = storyX
      lastViewWRef.current = viewW
      const col = railLeadColumn(storyX, colW)
      if (
        col === storyColRef.current
        && Math.abs(viewW - lastWindowViewRef.current) <= 8
        && Math.abs(colW - lastWindowColWRef.current) <= 0.5
      ) {
        return
      }
      onStoryScroll({
        scroll: storyX,
        viewW,
        colW,
      })
    },
    [onStoryScroll, rebuildFollowStops],
  )
  const paintGrabbing = (on: boolean) => {
    grabbingRef.current = on
    feedsRef.current?.classList.toggle('is-rail-grabbing', on)
  }
  const paintFollowLayer = (on: boolean) => {
    const value = on ? 'transform' : ''
    const sites = sitesTrackRef.current
    if (sites) sites.style.willChange = value
    const items = itemsTrackRef.current
    if (items) items.style.willChange = value
  }
  useLayoutEffect(() => {
    feedsRef.current?.classList.toggle('is-rail-grabbing', grabbingRef.current)
  })
  const onSiteGrab = useCallback(() => {
    railDriverRef.current = 'sites'
    paintGrabbing(true)
    paintFollowLayer(true)
    holdStoriesRef.current?.()
    dropPeek()
    primeStoryMount()
  }, [])
  const onStoryGrab = useCallback(() => {
    railDriverRef.current = 'stories'
    paintGrabbing(true)
    paintFollowLayer(true)
    holdStoriesRef.current?.()
    dropPeek()
    primeStoryMount()
  }, [])
  const clearStorySettleTimers = () => {
    if (focusTimerRef.current) {
      window.clearTimeout(focusTimerRef.current)
      focusTimerRef.current = 0
    }
    if (growFrameRef.current) {
      window.cancelAnimationFrame(growFrameRef.current)
      growFrameRef.current = 0
    }
  }
  const onSiteIdle = useCallback(() => {
    clearStorySettleTimers()
    paintGrabbing(false)
    paintFollowLayer(false)
    flushGrabFill()
  }, [])
  const onStoryIdle = useCallback(() => {
    clearStorySettleTimers()
    paintGrabbing(false)
    paintFollowLayer(false)
    flushGrabFill()
  }, [])

  usePhantasiRailPan(
    sitesViewRef,
    sitesTrackRef,
    sources.length > 0,
    siteKey,
    '.phantasi-site',
    onLeadChange,
    sitesApiRef,
    true,
    onSiteScroll,
    onSiteGrab,
    onSiteIdle,
  )

  usePhantasiRailPan(
    itemsViewRef,
    itemsTrackRef,
    stories.length > 0,
    railEpoch,
    '.phantasi-story',
    undefined,
    itemsApiRef,
    true,
    onStoryScroll,
    onStoryGrab,
    onStoryIdle,
  )

  const alignStoryGroup = (groupId: number | null): boolean => {
    if (groupId == null) return false
    const start = sourceColsRef.current.find((block) => block.id === groupId)
    if (!start) return false
    const column = start.column
    const mounted = mountColsRef.current
    const leadCol =
      storyColRef.current ||
      railLeadColumn(lastScrollRef.current, colWRef.current)
    const far = Math.abs(column - leadCol) > 1
    if (column < mounted.from || column > mounted.to || far) {
      const colW = colWRef.current
      const totalCols = storyColsRef.current
      const ideal = railMountColumns(
        (column - 1) * colW,
        itemsViewRef.current?.clientWidth || 800,
        colW,
        totalCols,
      )
      const next = {
        from: ideal.from,
        to: Math.min(totalCols, ideal.to + RAIL_MOUNT_GROW_AHEAD),
      }
      eagerBandRef.current = ideal
      pendingStoryAlignRef.current = column
      dropStoryDomShells(itemsTrackRef.current)
      clearPhantasiStoryPeeks(itemsTrackRef.current)
      if (!grabbingRef.current && onPeekItem) {
        schedulePhantasiPeekResume(onPeekItem)
      }
      mountColsRef.current = next
      liveToRef.current = Math.min(
        next.to,
        Math.max(
          column + RAIL_MOUNT_LIVE_PAD,
          railLiveTo(ideal.to, next.to, 1),
        ),
      )
      storySetMountRef.current(next)
      storySetLiveRef.current(liveToRef.current)
    }
    itemsApiRef.current?.alignColumn(column, true)
    return true
  }

  const prevStorySlotsRef = useRef(storySlots)
  useLayoutEffect(() => {
    const prev = prevStorySlotsRef.current
    prevStorySlotsRef.current = storySlots
    if (prev === storySlots) return
    const colW = colWRef.current
    if (colW <= 1) return
    const scroll = railTrackScroll(itemsTrackRef.current, lastScrollRef.current)
    lastScrollRef.current = scroll
    const leadCol = railLeadColumn(scroll, colW)
    const shift = storyColumnShift(prev, storySlots, leadCol)
    if (shift) itemsApiRef.current?.nudge(shift * colW)
  }, [storySlots])

  useLayoutEffect(() => {
    if (skipStoryAlignRef.current) {
      skipStoryAlignRef.current = false
      return
    }
    pendingSourceAlignRef.current = alignStoryGroup(focusId) ? null : focusId
  }, [focusId])

  useLayoutEffect(() => {
    const pending = pendingSourceAlignRef.current
    if (pending == null || pending !== focusId) return
    if (alignStoryGroup(pending)) pendingSourceAlignRef.current = null
  }, [focusId, sourceCols])

  const activateSite = (id: number) => {
    if (!isAggregateFeedId(id) && isEditMode) {
      onToggleSelect?.(id)
      return
    }
    if (isAggregateFeedId(id) && isEditMode) return
    railDriverRef.current = 'sites'
    lastSourceRef.current = id
    paintedOnRef.current = id
    paintedElRef.current = null
    if (focusTimerRef.current) {
      window.clearTimeout(focusTimerRef.current)
      focusTimerRef.current = 0
    }
    setFocusId(id)
    setReadyId(id)
    onJumpSource?.(id)
    onRailFocus?.(isAggregateFeedId(id) ? null : id)
    releaseStoriesRef.current?.()
    sitesApiRef.current?.align(id, true)
    const aligned = alignStoryGroup(id)
    skipStoryAlignRef.current = aligned
    pendingSourceAlignRef.current = aligned ? null : id
  }
  activateSiteRef.current = activateSite

  const openArticle = (
    item: PhantasiItemPreview & { source_id?: number },
    source?: PhantasiSource,
  ) => {
    const target =
      source ??
      (item.source_id != null
        ? sources.find((entry) => entry.id === item.source_id)
        : undefined) ??
      ready ??
      focus
    if (!target) return
    if (isEditMode) {
      onToggleSelect?.(target.id)
      return
    }
    lastSourceRef.current = target.id
    paintedOnRef.current = target.id
    paintedElRef.current = null
    setFocusId(target.id)
    setReadyId(target.id)
    sitesApiRef.current?.align(target.id)
    onOpenItem?.(item, target)
  }
  openArticleRef.current = openArticle

  return (
    <div
      ref={feedsRef}
      className="phantasi-skin phantasi-feeds"
    >
      <div className="phantasi-feeds__air" aria-hidden />
      <div className="phantasi-feeds__stage">
        {toolbar ? <div className="phantasi-feeds__bar">{toolbar}</div> : null}
        <section
          data-tour="journal-sources"
          className="phantasi-feeds__sites"
          ref={sitesViewRef}
          aria-labelledby={sitesTitleId}
        >
          <div className="phantasi-feeds__source-chrome">
          <PhantasiRailTitle
            id={sitesTitleId}
            action={sourceTags}
            pinned={isEditMode}
          >
            {t.phantasi.sources}
          </PhantasiRailTitle>
          </div>
          {vacant
            ?? (sources.length === 0 ? (
              <PhantasiVacant
                layout="friends"
                title={t.phantasi.emptyNoSources}
                articleTitle={t.phantasi.latestArticles}
              />
            ) : null)}
          <div
            className="phantasi-feeds__sites-track"
            ref={sitesTrackRef}
            data-phantasi-rail-track="sites"
            hidden={!!vacant || sources.length === 0}
          >
          <PhantasiFeedsSites
            sources={sources}
            inbox={inbox}
            mixes={topicMixes}
            onId={lastSourceRef.current ?? focusId}
            times={times}
            locale={locale}
            isEditMode={isEditMode}
            selectedIds={selectedIds}
            emptyLabel={t.phantasi.noArticles}
            editLabel={t.phantasi.editSource}
            canEdit={!!onEditSource}
            onActivate={onActivateSite}
            onEdit={onEditSite}
            onIconLoad={onIconLoadSite}
          />
          </div>
        </section>

        <section
          data-tour="journal-articles"
          className="phantasi-feeds__items"
          ref={itemsViewRef}
          hidden={!!vacant || sources.length === 0}
          aria-labelledby={itemsTitleId}
        >
          <PhantasiRailTitle id={itemsTitleId}>
            {t.phantasi.latestArticles}
          </PhantasiRailTitle>
          {stories.length === 0 ? (
            <PhantasiVacant title={t.phantasi.noArticles} />
          ) : (
            <PhantasiFeedsStories
              storySlots={storySlots}
              times={times}
              locale={locale}
              labels={phantasiLabels}
              onOpen={onOpenStory}
              onPeek={onPeekStory}
              onPeekEnd={onPeekEndStory}
              onToggleStar={onToggleStar ? onStarStory : undefined}
              trackRef={itemsTrackRef}
              setMountRef={storySetMountRef}
              setLiveRef={storySetLiveRef}
              mountCommittedRef={storyMountCommittedRef}
              mountColsRef={mountColsRef}
              liveToRef={liveToRef}
              pendingStoryAlignRef={pendingStoryAlignRef}
              itemsApiRef={itemsApiRef}
              eagerBandRef={eagerBandRef}
              lastEagerRef={lastEagerRef}
              warmRef={storyWarmRef}
              grabbingRef={grabbingRef}
            />
          )}
        </section>
      </div>
    </div>
  )
}

export default memo(PhantasiFeeds)
