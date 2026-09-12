import type { ReactNode } from 'react'
import type { BrewItemPreview, BrewSource } from '../../../types/brew'
import type { FeedStory } from '../logic/feedStories'
import type { FeedsChrome, FlipBox } from './flipCards'
import type { BrewRailApi } from './useBrewRailPan'

import { FaCompress as Compress, FaExpand as Expand } from '@lib/icons'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import {
  brewMotionClaim,
  brewMotionOwns,
  brewMotionRelease,
} from '../../../hooks/animation/pages/brewMotion'
import { extractColorsFromLoadedImage } from '../../../utils/colorExtractor'
import {
  DEFAULT_THEME_COLOR,
  getIconUrl,
  getImageUrl,
  normalizeThemeColor,
} from '../constants'
import {
  completeSiteView,
  idleSitesIntent,
  requestSiteView,
} from '../logic/feedsMotion'
import { BrewManagement } from '../ui/BrewManagement'
import { SiteCard } from '../ui/SiteCard'
import { BrewStory } from './BrewStory'
import {
  brewFlipQuiet,
  clearRailExits,
  dropFlip,
  dropStaleGhosts,
  enterSites,
  enterStories,
  exitStories,

  FLIP_INTRO_STORY_MS,

  flySites,
  holdFeedsChrome,
  playFeedsChrome,
  readFeedsChrome,
  readFlipBoxes,
  revealFeedsTree,
  seatSiteTrack,
  setFeedsRevealHandler,
  settleFlip,
  siteOpenDelay,
  storyPhaseDelay,
  waitFlip,
} from './flipCards'
import { brewRelativeTime, useBrewTimes } from './time'
import { useBrewRailPan } from './useBrewRailPan'

const ARTICLES_SETTLE_MS = 200

function paintSiteInk(img: HTMLImageElement, fallback: string | null): void {
  if (fallback) return
  try {
    const primary = extractColorsFromLoadedImage(img).primary
    if (
      !primary ||
      primary === DEFAULT_THEME_COLOR ||
      primary === '#6b7280'
    ) {
      return
    }
    const card = img.closest('.brew-site')
    if (card instanceof HTMLElement) {
      card.style.setProperty('--site-ink', normalizeThemeColor(primary))
    }
  } catch {
    // 取色失败保持默认灰。
  }
}

export interface BrewFeedsProps {
  sources: BrewSource[]
  focusSourceId?: number | null
  isEditMode?: boolean
  selectedIds?: Set<number>
  onToggleSelect?: (id: number) => void
  onSourceClick: (source: BrewSource) => void
  onOpenItem?: (item: BrewItemPreview, source: BrewSource) => void
  onPeekItem?: (item: BrewItemPreview) => void
  onPeekEnd?: () => void
  onToggleStar?: (item: BrewItemPreview) => void | false | Promise<void | false>
  onEditSource?: (source: BrewSource) => void
  onSitesOpenChange?: (open: boolean) => void
  toolbar?: ReactNode
  vacant?: ReactNode
  stories?: FeedStory[]
  onReadySource?: (id: number | null) => void
}

export default function BrewFeeds({
  sources,
  focusSourceId,
  isEditMode = false,
  selectedIds,
  onToggleSelect,
  onSourceClick: _onSourceClick,
  onOpenItem,
  onPeekItem,
  onPeekEnd,
  onToggleStar,
  onEditSource,
  onSitesOpenChange,
  toolbar,
  vacant,
  stories = [],
  onReadySource,
}: BrewFeedsProps) {
  const { t, locale } = useI18n()
  const feedsRef = useRef<HTMLDivElement>(null)
  const sitesViewRef = useRef<HTMLDivElement>(null)
  const sitesTrackRef = useRef<HTMLDivElement>(null)
  const sitesApiRef = useRef<BrewRailApi | null>(null)
  const itemsViewRef = useRef<HTMLDivElement>(null)
  const itemsTrackRef = useRef<HTMLDivElement>(null)
  const [focusId, setFocusId] = useState<number | null>(sources[0]?.id ?? null)
  const [readyId, setReadyId] = useState<number | null>(sources[0]?.id ?? null)
  const [hoverStoryId, setHoverStoryId] = useState<number | null>(null)
  const [sitesOpen, setSitesOpen] = useState(false)
  const [flipping, setFlipping] = useState(false)
  const [morphing, setMorphing] = useState(false)
  const [sitesBooted, setSitesBooted] = useState(() => brewFlipQuiet())
  const [storiesBooted, setStoriesBooted] = useState(() => brewFlipQuiet())
  const sitesOpenRef = useRef(sitesOpen)
  sitesOpenRef.current = sitesOpen
  const sitesIntent = useRef(idleSitesIntent(false))
  const flipLock = useRef(false)
  const motionHolds = useRef(0)
  const pendingAlign = useRef<number | null>(null)
  const pendingFlip = useRef<Map<string, FlipBox> | null>(null)
  const pendingStory = useRef<'enter' | 'exit' | null>(null)
  const pendingStoryBoxes = useRef<Map<string, FlipBox> | null>(null)
  const pendingChrome = useRef<FeedsChrome | null>(null)
  const introSites = useRef(false)
  const introStories = useRef(false)
  const appliedFocus = useRef<number | null>(null)
  const brewLabels = t.brew
  const times = useBrewTimes()

  const focus = useMemo(
    () => sources.find((s) => s.id === focusId) ?? sources[0] ?? null,
    [sources, focusId],
  )
  const ready = useMemo(
    () => sources.find((s) => s.id === readyId) ?? focus,
    [sources, readyId, focus],
  )

  useEffect(() => {
    if (!sources.length) {
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
      setFocusId(focusSourceId)
      setReadyId(focusSourceId)
      return
    }
    if (!focusId || !sources.some((s) => s.id === focusId)) {
      setFocusId(sources[0].id)
      setReadyId(sources[0].id)
    }
  }, [sources, focusId, focusSourceId])

  useEffect(() => {
    if (focusSourceId == null || focusId !== focusSourceId) return
    sitesApiRef.current?.align(focusSourceId)
  }, [focusSourceId, focusId])

  useEffect(() => {
    if (focusId == null || focusId === readyId) return
    const timer = window.setTimeout(setReadyId, ARTICLES_SETTLE_MS, focusId)
    return () => window.clearTimeout(timer)
  }, [focusId, readyId])

  useEffect(() => {
    onReadySource?.(readyId)
  }, [onReadySource, readyId])

  const siteKey = useMemo(
    () => sources.map((source) => source.id).join(','),
    [sources],
  )
  const itemKey = `${ready?.id ?? 0}:${stories.map((item) => item.id).join(',')}`

  useEffect(() => {
    setHoverStoryId(null)
  }, [focusId])

  useEffect(() => {
    if (sitesOpen) setHoverStoryId(null)
  }, [sitesOpen])

  useEffect(() => {
    if (morphing) return
    onSitesOpenChange?.(sitesOpen)
  }, [sitesOpen, morphing, onSitesOpenChange])

  useLayoutEffect(() => {
    setFeedsRevealHandler(() => setMorphing(false))
    return () => setFeedsRevealHandler(null)
  }, [])

  const onLeadChange = useCallback((id: number) => {
    setFocusId(id)
  }, [])

  const railsReady =
    brewFlipQuiet() ||
    (introSites.current && (stories.length === 0 || introStories.current))

  useBrewRailPan(
    sitesViewRef,
    sitesTrackRef,
    !sitesOpen && !flipping && railsReady && sources.length > 0,
    siteKey,
    '.brew-site',
    onLeadChange,
    sitesApiRef,
    true,
  )
  useBrewRailPan(
    itemsViewRef,
    itemsTrackRef,
    !sitesOpen && !flipping && railsReady && stories.length > 0,
    itemKey,
    '.brew-story',
  )

  const holdMotion = () => {
    motionHolds.current += 1
    flipLock.current = true
    setFlipping(true)
  }

  const releaseMotion = () => {
    motionHolds.current = Math.max(0, motionHolds.current - 1)
    if (motionHolds.current > 0) return
    flipLock.current = false
    pendingChrome.current = null
    sitesIntent.current = {
      ...sitesIntent.current,
      flipping: false,
      displayOpen: sitesOpenRef.current,
    }
    setFlipping(false)
    setMorphing(false)
  }

  const abortMotion = () => {
    motionHolds.current = 0
    flipLock.current = false
    pendingFlip.current = null
    pendingStory.current = null
    pendingStoryBoxes.current = null
    pendingChrome.current = null
    setFlipping(false)
    setMorphing(false)
    const root = feedsRef.current
    if (root) revealFeedsTree(root)
  }

  const cancelOwn = (anims: readonly Animation[]) => {
    for (const anim of anims) {
      try {
        anim.cancel()
      } catch {
      }
    }
  }

  useLayoutEffect(() => {
    const first = pendingFlip.current
    const story = pendingStory.current
    const storyBoxes = pendingStoryBoxes.current
    const root = feedsRef.current
    const track = sitesTrackRef.current
    if (first && !sitesOpen && pendingAlign.current != null && track) {
      seatSiteTrack(track, pendingAlign.current)
    }
    if (!first || !root) return
    root.classList.add('is-sites-morphing')
    dropStaleGhosts(root)
    clearRailExits(root, '.brew-site')
    clearRailExits(root, '.brew-story')
    const lead = focusId != null ? String(focusId) : null
    const destChrome = readFeedsChrome(root)
    const anims = [
      ...flySites(root, first, lead, siteOpenDelay(story ?? 'enter'), sitesOpen),
      ...(story === 'exit'
        ? exitStories(root, '.brew-story', storyBoxes, root)
        : enterStories(
            root,
            '.brew-story',
            storyPhaseDelay(story ?? 'enter'),
          )),
    ]
    const fromChrome = pendingChrome.current
    if (fromChrome) {
      holdFeedsChrome(root, fromChrome)
      anims.push(...playFeedsChrome(root, fromChrome, destChrome))
    }
    if (story === 'enter') introStories.current = true
    const token = brewMotionClaim('flip')
    const generation = sitesIntent.current.generation
    let alive = true
    let released = false
    const finish = (teardown: boolean) => {
      if (released) return
      released = true
      if (teardown && brewMotionOwns(token)) {
        settleFlip(anims)
        dropFlip(anims, root)
        brewMotionRelease(token)
      } else {
        cancelOwn(anims)
        if (brewMotionOwns(token)) brewMotionRelease(token)
      }
      if (generation !== sitesIntent.current.generation) return
      releaseMotion()
    }
    void waitFlip(anims).then(() => {
      if (!alive) return
      const done = completeSiteView(sitesIntent.current, generation)
      if (done.action === 'ignore') return
      pendingFlip.current = null
      pendingStory.current = null
      pendingStoryBoxes.current = null
      sitesIntent.current = done.state
      finish(brewMotionOwns(token))
      if (done.action === 'flip') setSitesMode(done.state.targetOpen)
    })
    return () => {
      alive = false
      // 已钉 first：只放锁，别拆新幽灵。
      finish(!(pendingFlip.current || pendingStory.current) && brewMotionOwns(token))
    }
  }, [sitesOpen])

  useLayoutEffect(() => {
    if (sitesOpenRef.current || brewFlipQuiet()) {
      introSites.current = true
      setSitesBooted(true)
      return
    }
    if (flipLock.current || introSites.current || sources.length === 0) return
    const root = feedsRef.current
    const track = sitesTrackRef.current
    if (!root) return
    introSites.current = true
    holdMotion()
    setSitesBooted(true)
    const lead = focusId != null ? String(focusId) : null
    if (track && focusId != null) seatSiteTrack(track, focusId)
    clearRailExits(root, '.brew-site')
    const anims = enterSites(root, 0, lead)
    let alive = true
    let released = false
    const finish = () => {
      if (released) return
      released = true
      cancelOwn(anims)
      releaseMotion()
    }
    void waitFlip(anims).then(() => {
      if (!alive) return
      finish()
    })
    return () => {
      alive = false
      finish()
    }
    // 开合不能进依赖，cleanup 会碰到正在飞的开合。
  }, [sources.length])

  useLayoutEffect(() => {
    if (sitesOpenRef.current || brewFlipQuiet()) {
      introStories.current = true
      setStoriesBooted(true)
      return
    }
    if (introStories.current || stories.length === 0) return
    if (sources.length > 0 && !introSites.current) return
    if (flipLock.current && !introSites.current) return
    const root = feedsRef.current
    if (!root) return
    introStories.current = true
    holdMotion()
    setStoriesBooted(true)
    clearRailExits(root, '.brew-story')
    const extra = motionHolds.current > 1 ? FLIP_INTRO_STORY_MS : 0
    const anims = enterStories(root, '.brew-story', extra)
    let alive = true
    let released = false
    const finish = () => {
      if (released) return
      released = true
      cancelOwn(anims)
      releaseMotion()
    }
    void waitFlip(anims).then(() => {
      if (!alive) return
      finish()
    })
    return () => {
      alive = false
      finish()
    }
  }, [stories.length, sources.length])

  useLayoutEffect(() => {
    if (sitesOpen || flipping) return
    const id = pendingAlign.current
    if (id == null) return
    pendingAlign.current = null
    sitesApiRef.current?.align(id, true)
  }, [sitesOpen, flipping])

  const peeked =
    stories.find((item) => item.id === hoverStoryId) ?? null
  const scene = peeked ? getImageUrl(peeked.image) : ''

  const setSitesMode = (open: boolean, alignId?: number | null) => {
    if (alignId != null) pendingAlign.current = alignId
    else if (!open) pendingAlign.current = focusId
    const request = requestSiteView(sitesIntent.current, open)
    if (request.action === 'noop') {
      if (!open && !flipLock.current && pendingAlign.current != null) {
        sitesApiRef.current?.align(pendingAlign.current, true)
      }
      return
    }
    sitesIntent.current = request.state
    if (brewFlipQuiet() || sitesOpenRef.current === open) {
      sitesIntent.current = completeSiteView(
        request.state,
        request.state.generation,
      ).state
      if (sitesOpenRef.current === open) abortMotion()
      setSitesOpen(open)
      return
    }
    const root = feedsRef.current
    pendingFlip.current = root
      ? readFlipBoxes(root, '.brew-site')
      : new Map()
    pendingStoryBoxes.current = root
      ? readFlipBoxes(root, '.brew-story')
      : new Map()
    pendingChrome.current = root ? readFeedsChrome(root) : null
    pendingStory.current = open ? 'exit' : 'enter'
    if (request.action === 'flip') holdMotion()
    root?.classList.add('is-sites-morphing')
    setMorphing(true)
    setFlipping(true)
    setSitesOpen(open)
  }

  const foldSites = (alignId?: number | null) => {
    setSitesMode(false, alignId ?? focusId)
  }

  const activateSite = (source: BrewSource) => {
    if (isEditMode) {
      onToggleSelect?.(source.id)
      return
    }
    setFocusId(source.id)
    setReadyId(source.id)
    if (sitesOpen) {
      foldSites(source.id)
      return
    }
    sitesApiRef.current?.align(source.id)
  }

  const openArticle = (item: BrewItemPreview, source?: BrewSource) => {
    const target = source ?? ready ?? focus
    if (!target) return
    if (isEditMode) {
      onToggleSelect?.(target.id)
      return
    }
    setFocusId(target.id)
    setReadyId(target.id)
    if (sitesOpen) foldSites(target.id)
    else sitesApiRef.current?.align(target.id)
    onOpenItem?.(item, target)
  }

  return (
    <div
      ref={feedsRef}
      className={`brew-skin brew-feeds${sitesOpen ? ' is-sites-open' : ''}${flipping ? ' is-sites-flipping' : ''}${morphing ? ' is-sites-morphing' : ''}${sitesBooted ? ' is-sites-booted' : ''}${storiesBooted ? ' is-stories-booted' : ''}`}
    >
      <div className="brew-feeds__air" aria-hidden />
      <div className="brew-feeds__stage">
        <div className="brew-feeds__bar">
          <BrewManagement
            embedded
            active={isEditMode}
            displayControl={
              <button
                type="button"
                className="brew-feeds__spread"
                aria-pressed={sitesOpen}
                aria-busy={flipping}
                onClick={() => {
                  if (sitesOpen) foldSites(focusId)
                  else setSitesMode(true)
                }}
              >
                <span className="brew-feeds__mark" aria-hidden>
                  <Expand />
                  <Compress />
                </span>
                <span className="brew-feeds__spread-label">
                  <span>{t.brew.spreadSites}</span>
                  <span>{t.brew.foldSites}</span>
                </span>
              </button>
            }
          >
          {toolbar}
          </BrewManagement>
        </div>
        {vacant || null}
        <div
          className="brew-feeds__sites"
          ref={sitesViewRef}
          hidden={!!vacant}
        >
          <div
            className="brew-feeds__sites-track"
            ref={sitesTrackRef}
            data-brew-rail-track="sites"
          >
          {sources.map((source) => {
            const latest = source.recent_items?.[0] ?? null
            const cover = source.id === ready?.id ? scene : ''
            return (
              <SiteCard
                key={source.id}
                id={source.id}
                name={source.name}
                description={source.description?.trim() || ''}
                icon={getIconUrl(source.icon)}
                unread={source.unread_count}
                latestTitle={latest?.title}
                latestWhen={
                  latest
                    ? brewRelativeTime(latest.published_at, times, locale)
                    : ''
                }
                on={source.id === focus?.id}
                cover={cover}
                editing={isEditMode}
                picked={selectedIds?.has(source.id)}
                ink={normalizeThemeColor(source.theme_color)}
                emptyLabel={t.brew.noArticles}
                editLabel={t.brew.editSource}
                onActivate={() => activateSite(source)}
                onOpenLatest={
                  latest ? () => openArticle(latest, source) : undefined
                }
                onEdit={
                  onEditSource ? () => onEditSource(source) : undefined
                }
                onIconLoad={(img) => {
                  const theme = source.theme_color
                  const idle = window.requestIdleCallback
                  if (idle) {
                    idle(() => paintSiteInk(img, theme), { timeout: 800 })
                    return
                  }
                  window.setTimeout(paintSiteInk, 0, img, theme)
                }}
              />
            )
          })}
          </div>
        </div>

        <div
          className="brew-feeds__items"
          ref={itemsViewRef}
          hidden={!!vacant}
          aria-hidden={sitesOpen}
          inert={sitesOpen}
        >
          {stories.length === 0 ? (
            <p className="brew-feeds__empty">{t.brew.noArticles}</p>
          ) : (
            <div
              className="brew-feeds__items-track"
              ref={itemsTrackRef}
              data-brew-rail-track="items"
            >
            {stories.map((item) => (
              <BrewStory
                key={item.id}
                item={item}
                times={times}
                locale={locale}
                labels={brewLabels}
                onOpen={() => openArticle(item)}
                onPeek={() => {
                  setHoverStoryId(item.id)
                  onPeekItem?.(item)
                }}
                onPeekEnd={() => {
                  setHoverStoryId((id) => (id === item.id ? null : id))
                  onPeekEnd?.()
                }}
                onToggleStar={
                  onToggleStar
                    ? (story) => {
                        onToggleStar(story)
                      }
                    : undefined
                }
              />
            ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
