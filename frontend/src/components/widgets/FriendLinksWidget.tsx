import type { CSSProperties } from 'react'
import type { PhantasiSource } from '../../types/phantasi'
import type { WidgetComponentProps } from '../widgetGridTypes'

import {
  LuChevronRight as ChevronRight,
  LuExternalLink as ExternalLink,
  LuLink as Link,
} from '@lib/icons'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useI18n } from '../../contexts/I18nContext'
import { useHomeVisibilityInterval } from '../../hooks/animation'
import {
  isExlight,
  useAnimationLevel,
} from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { fetchFriendLinkSources } from '../../services/friendLinkSources'
import { getIconUrl, isFriendLinkCategory } from '../phantasi/constants'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeletonCover } from './shared/WidgetSkeleton'
import './FriendLinksWidget.css'

const BATCH_INTERVAL = 5 * 1000
const BATCH_TRANSITION_DURATION = 810

interface FriendLinkEntry {
  id: number
  name: string
  url: string
  icon: string | null
  description: string | null
  color: string
}

function belongsToFriendLinks(source: PhantasiSource): boolean {
  return Boolean(
    source.category
      ?.split(',')
      .map((category) => category.trim())
      .some(isFriendLinkCategory),
  )
}

function compareSources(
  a: PhantasiSource,
  b: PhantasiSource,
  locale: string,
): number {
  const aHasOrder = typeof a.sort_order === 'number'
  const bHasOrder = typeof b.sort_order === 'number'
  if (aHasOrder && bHasOrder && a.sort_order !== b.sort_order) {
    return a.sort_order! - b.sort_order!
  }
  if (aHasOrder !== bHasOrder) return aHasOrder ? -1 : 1
  return a.name.localeCompare(b.name, locale)
}

function safeLink(source: PhantasiSource): string {
  const target = source.site_url || source.url
  return /^https?:\/\//i.test(target) ? target : ''
}

function sourceSubtitle(source: PhantasiSource): string | null {
  const tag = source.ai_style_tags?.find((item) => item.trim())?.trim()
  if (tag) return tag

  const description = source.description?.trim()
  if (description) return description

  try {
    return new URL(safeLink(source)).hostname.replaceAll(/^www\./g, '') || null
  } catch {
    return null
  }
}

function toEntry(source: PhantasiSource): FriendLinkEntry {
  return {
    id: source.id,
    name: source.name,
    url: safeLink(source),
    icon: getIconUrl(source.icon),
    description: sourceSubtitle(source),
    color: source.theme_color || '#f97316',
  }
}

function randomRank(id: number, seed: number): number {
  const value = Math.sin(id * 12.9898 + seed * 78.233) * 43758.5453
  return value - Math.floor(value)
}

function FriendLinkIcon({
  icon,
  color,
  className,
  iconClassName,
}: {
  icon: string | null
  color: string
  className: string
  iconClassName: string
}) {
  const [failed, setFailed] = useState(false)
  const showImage = Boolean(icon) && !failed

  useEffect(() => {
    setFailed(false)
  }, [icon])

  return (
    <span
      className={className}
      style={{
        backgroundColor: showImage ? 'transparent' : `${color}1f`,
        color,
      }}
    >
      {showImage ? (
        <img
          src={icon!}
          alt=""
          draggable={false}
          loading="lazy"
          decoding="async"
          className="absolute inset-0 h-full w-full object-cover"
          onLoad={(e) => {
            // 失效远程图标代理回 1×1 PNG。
            const img = e.currentTarget
            if (img.naturalWidth <= 1 && img.naturalHeight <= 1) {
              setFailed(true)
            }
          }}
          onError={() => setFailed(true)}
        />
      ) : (
        <Link className={iconClassName} />
      )}
    </span>
  )
}

export const FriendLinksWidget = memo(
  ({ config, isEditMode, isPreview }: WidgetComponentProps) => {
    const { t, format, locale } = useI18n()
    const navigate = useNavigate()
    const anim = useAnimationLevel()
    const { containerRef: sizeRef, scale, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const mountedRef = useRef(true)
    const shownRef = useRef(false)
    const ioRef = useRef<IntersectionObserver | null>(null)
    const [shown, setShown] = useState(false)
    const [sources, setSources] = useState<PhantasiSource[]>([])
    const [loading, setLoading] = useState(!isPreview)
    const [failed, setFailed] = useState(false)
    const [randomSeed] = useState(() => Math.random())
    const [batchIndex, setBatchIndex] = useState(0)
    const [incomingBatchIndex, setIncomingBatchIndex] = useState<number | null>(
      null,
    )
    const batchIndexRef = useRef(0)
    const batchTransitioningRef = useRef(false)
    const batchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

    useEffect(() => {
      mountedRef.current = true
      return () => {
        mountedRef.current = false
        ioRef.current?.disconnect()
        ioRef.current = null
      }
    }, [])

    const markShown = useCallback(() => {
      if (shownRef.current) return
      shownRef.current = true
      setShown(true)
    }, [])

    const containerRef = useCallback(
      (node: HTMLDivElement | null) => {
        sizeRef(node)
        ioRef.current?.disconnect()
        ioRef.current = null
        if (!node || isPreview || shownRef.current) return
        if (typeof IntersectionObserver === 'undefined') {
          markShown()
          return
        }
        const io = new IntersectionObserver((entries) => {
          if (!entries.some((entry) => entry.isIntersecting)) return
          markShown()
          io.disconnect()
          ioRef.current = null
        })
        ioRef.current = io
        io.observe(node)
      },
      [isPreview, markShown, sizeRef],
    )

    const loadFriendLinks = useCallback(async () => {
      if (isPreview) return
      try {
        const nextSources = await fetchFriendLinkSources('widget')
        if (!mountedRef.current) return
        setSources(nextSources)
        setFailed(false)
      } catch (error) {
        console.error('[FriendLinksWidget] Failed to load Phantasi sources:', error)
        if (mountedRef.current) setFailed(true)
      } finally {
        if (mountedRef.current) setLoading(false)
      }
    }, [isPreview])

    useEffect(() => {
      if (!shown) return
      void loadFriendLinks()
    }, [loadFriendLinks, shown])

    const previewEntries = useMemo<FriendLinkEntry[]>(
      () => [
        {
          id: -1,
          name: t.friendLinksWidget.samplePersonalBlog,
          url: '',
          icon: null,
          description: t.friendLinksWidget.sampleLifeAndNotes,
          color: '#f97316',
        },
        {
          id: -2,
          name: t.friendLinksWidget.sampleTechNotes,
          url: '',
          icon: null,
          description: t.friendLinksWidget.sampleCodeAndIdeas,
          color: '#3b82f6',
        },
        {
          id: -3,
          name: t.friendLinksWidget.sampleDesignJournal,
          url: '',
          icon: null,
          description: t.friendLinksWidget.sampleDesignAndInspiration,
          color: '#ec4899',
        },
        {
          id: -4,
          name: t.friendLinksWidget.samplePhotoAlbum,
          url: '',
          icon: null,
          description: t.friendLinksWidget.sampleLightAndJourneys,
          color: '#14b8a6',
        },
      ],
      [t.friendLinksWidget],
    )

    const entries = useMemo(() => {
      if (isPreview) return previewEntries
      return sources
        .filter(belongsToFriendLinks)
        .toSorted((left, right) => compareSources(left, right, locale))
        .map(toEntry)
    }, [isPreview, previewEntries, sources, locale])

    const isStrip = config.size === '4x1'
    const isWide = config.size === '4x2'
    const batchSize = isWide ? 4 : 1
    const randomizedEntries = useMemo(
      () =>
        entries.toSorted(
          (a, b) => randomRank(a.id, randomSeed) - randomRank(b.id, randomSeed),
        ),
      [entries, randomSeed],
    )
    const batchCount = Math.max(
      1,
      Math.ceil(randomizedEntries.length / batchSize),
    )
    const getBatchEntries = useCallback(
      (index: number) => {
        if (randomizedEntries.length === 0) return []
        const start = (index % batchCount) * batchSize
        const count = Math.min(batchSize, randomizedEntries.length)
        return Array.from(
          { length: count },
          (_, offset) =>
            randomizedEntries[(start + offset) % randomizedEntries.length],
        )
      },
      [batchCount, batchSize, randomizedEntries],
    )
    const visibleEntries = useMemo(
      () => getBatchEntries(batchIndex),
      [batchIndex, getBatchEntries],
    )
    const incomingEntries = useMemo(
      () =>
        incomingBatchIndex === null ? [] : getBatchEntries(incomingBatchIndex),
      [getBatchEntries, incomingBatchIndex],
    )

    useEffect(() => {
      if (batchTimerRef.current) clearTimeout(batchTimerRef.current)
      batchTimerRef.current = null
      batchTransitioningRef.current = false
      batchIndexRef.current = 0
      setBatchIndex(0)
      setIncomingBatchIndex(null)
    }, [batchSize, randomizedEntries.length])

    useEffect(
      () => () => {
        if (batchTimerRef.current) clearTimeout(batchTimerRef.current)
      },
      [],
    )

    const advanceBatch = useCallback(() => {
      if (batchCount <= 1 || batchTransitioningRef.current) return

      const nextIndex = (batchIndexRef.current + 1) % batchCount
      if (isExlight(anim)) {
        batchIndexRef.current = nextIndex
        setBatchIndex(nextIndex)
        return
      }

      batchTransitioningRef.current = true
      setIncomingBatchIndex(nextIndex)
      batchTimerRef.current = setTimeout(() => {
        batchIndexRef.current = nextIndex
        setBatchIndex(nextIndex)
        setIncomingBatchIndex(null)
        batchTransitioningRef.current = false
        batchTimerRef.current = null
      }, BATCH_TRANSITION_DURATION)
    }, [anim.durationScale, anim.level, batchCount])

    useHomeVisibilityInterval(
      advanceBatch,
      BATCH_INTERVAL,
      !isPreview && !isEditMode && batchCount > 1 && anim.widgetUiRotation,
    )

    const openPhantasi = useCallback(() => {
      if (!isEditMode && !isPreview) {
        void import('../../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.FRIEND_LINKS_PHANTASI, {
              throttleMs: 3000,
            })
          },
        )
        navigate('/journal/friends')
      }
    }, [isEditMode, isPreview, navigate])

    const openFriendLink = useCallback(
      (entry: FriendLinkEntry) => {
        if (isEditMode || isPreview || !entry.url) return
        void import('../../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            let host = ''
            try {
              host = new URL(entry.url).hostname
            } catch {
              host = entry.name || 'link'
            }
            trackProductEvent(AnalyticsEvents.FRIEND_LINK_CLICK, {
              target: host,
              throttleMs: 2000,
            })
          },
        )
        window.open(entry.url, '_blank', 'noopener,noreferrer')
      },
      [isEditMode, isPreview],
    )

    // 不要用 native disabled：全局 button:disabled 会把预览洗灰。
    const interactionLocked = isEditMode || isPreview
    const pointerEventsStyle = interactionLocked
      ? { pointerEvents: 'none' as const }
      : {}
    const shellClassName = isEditMode ? 'cursor-grab' : undefined
    const entryDisabled = (entryUrl: string, incoming: boolean) =>
      !interactionLocked && (incoming || !entryUrl)

    if (isStrip) {
      return (
        <WidgetShell
          containerRef={containerRef}
          scale={scale}
          padding={{ x: 9, y: 8 }}
          className={shellClassName}
          style={pointerEventsStyle}
          contentClassName="relative min-h-0 overflow-hidden"
        >
          {failed ? (
            <span className="absolute inset-0 flex items-center justify-center truncate px-3 text-[10px] text-gray-400 dark:text-gray-500">
              {t.friendLinksWidget.loadFailed}
            </span>
          ) : !loading && visibleEntries.length === 0 ? (
            <button
              type="button"
              onClick={openPhantasi}
              className="absolute inset-0 flex cursor-pointer items-center justify-center rounded-lg border border-dashed border-black/8 px-3 text-[10px] text-gray-400 dark:border-white/10 dark:text-gray-500"
            >
              {t.friendLinksWidget.emptyTitle}
            </button>
          ) : (
            <>
              {[
                {
                  id: `current-${batchIndex}`,
                  entries: visibleEntries,
                  incoming: false,
                },
                ...(incomingBatchIndex === null
                  ? []
                  : [
                      {
                        id: `incoming-${incomingBatchIndex}`,
                        entries: incomingEntries,
                        incoming: true,
                      },
                    ]),
              ].map((layer) => (
                <div
                  key={layer.id}
                  className={`absolute inset-0 flex ${
                    layer.incoming
                      ? 'friend-links-batch-enter pointer-events-none'
                      : incomingBatchIndex === null
                        ? ''
                        : 'friend-links-batch-exit'
                  }`}
                  aria-hidden={layer.incoming || undefined}
                >
                  {layer.entries.map((entry, entryIndex) => (
                    <button
                      key={entry.id}
                      type="button"
                      style={
                        {
                          '--friend-links-entry-index': entryIndex,
                        } as CSSProperties
                      }
                      onClick={() => openFriendLink(entry)}
                      disabled={entryDisabled(entry.url, layer.incoming)}
                      className="friend-links-entry group/link relative flex min-w-0 flex-1 cursor-pointer items-center gap-3 overflow-hidden rounded-lg px-3 text-left disabled:cursor-default disabled:opacity-100"
                      aria-label={format(t.friendLinksWidget.visitSite, {
                        name: entry.name,
                      })}
                    >
                      <FriendLinkIcon
                        icon={entry.icon}
                        color={entry.color}
                        className="relative z-10 flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-md"
                        iconClassName="h-5 w-5"
                      />
                      <span className="relative z-10 min-w-0 flex-1">
                        <span className="mb-0.5 block text-[8px] font-medium tracking-wide text-gray-400 dark:text-gray-500">
                          {t.widgets.friendLinks}
                        </span>
                        <span
                          className="block truncate font-semibold leading-tight text-gray-800 dark:text-gray-100"
                          style={{ fontSize: `${13 * fontScale}px` }}
                        >
                          {entry.name}
                        </span>
                        {entry.description && (
                          <span
                            className="mt-0.5 block truncate leading-tight text-gray-400 dark:text-gray-500"
                            style={{ fontSize: `${9 * fontScale}px` }}
                          >
                            {entry.description}
                          </span>
                        )}
                      </span>
                      <ExternalLink className="relative z-10 h-3.5 w-3.5 shrink-0 text-gray-300 dark:text-gray-600" />
                    </button>
                  ))}
                </div>
              ))}
            </>
          )}
          <WidgetSkeletonCover
            active={loading}
            preset="media-row"
            accent="#f97316"
            label={t.common.loading}
          />
        </WidgetShell>
      )
    }

    if (!isWide) {
      return (
        <WidgetShell
          containerRef={containerRef}
          scale={scale}
          padding={{ x: 10, y: 10 }}
          className={shellClassName}
          style={pointerEventsStyle}
          contentClassName="relative min-h-0 overflow-hidden"
        >
          {failed ? (
            <span className="absolute inset-0 flex items-center justify-center px-4 text-center text-[10px] text-gray-400 dark:text-gray-500">
              {t.friendLinksWidget.loadFailed}
            </span>
          ) : !loading && visibleEntries.length === 0 ? (
            <button
              type="button"
              onClick={openPhantasi}
              className="absolute inset-0 flex cursor-pointer flex-col items-center justify-center rounded-lg border border-dashed border-black/8 px-4 text-center dark:border-white/10"
            >
              <span className="text-xs font-medium text-gray-500 dark:text-gray-400">
                {t.friendLinksWidget.emptyTitle}
              </span>
              <span className="mt-1 text-[9px] text-gray-400 dark:text-gray-500">
                {t.friendLinksWidget.emptyDescription}
              </span>
            </button>
          ) : (
            <>
              {[
                {
                  id: `current-${batchIndex}`,
                  entries: visibleEntries,
                  incoming: false,
                },
                ...(incomingBatchIndex === null
                  ? []
                  : [
                      {
                        id: `incoming-${incomingBatchIndex}`,
                        entries: incomingEntries,
                        incoming: true,
                      },
                    ]),
              ].map((layer) => (
                <div
                  key={layer.id}
                  className={`absolute inset-0 flex ${
                    layer.incoming
                      ? 'friend-links-batch-enter pointer-events-none'
                      : incomingBatchIndex === null
                        ? ''
                        : 'friend-links-batch-exit'
                  }`}
                  aria-hidden={layer.incoming || undefined}
                >
                  {layer.entries.map((entry, entryIndex) => (
                    <button
                      key={entry.id}
                      type="button"
                      style={
                        {
                          '--friend-links-entry-index': entryIndex,
                        } as CSSProperties
                      }
                      onClick={() => openFriendLink(entry)}
                      disabled={entryDisabled(entry.url, layer.incoming)}
                      className="friend-links-entry group/link relative flex min-h-0 min-w-0 flex-1 cursor-pointer flex-col items-center justify-center overflow-hidden p-3 text-center disabled:cursor-default disabled:opacity-100"
                      aria-label={format(t.friendLinksWidget.visitSite, {
                        name: entry.name,
                      })}
                    >
                      <FriendLinkIcon
                        icon={entry.icon}
                        color={entry.color}
                        className="relative z-10 flex h-16 w-16 shrink-0 items-center justify-center overflow-hidden rounded-lg"
                        iconClassName="h-6 w-6"
                      />
                      <span
                        className="relative z-10 mt-2.5 block w-full truncate font-semibold leading-tight text-gray-800 dark:text-gray-100"
                        style={{ fontSize: `${14 * fontScale}px` }}
                      >
                        {entry.name}
                      </span>
                      {entry.description && (
                        <span
                          className="relative z-10 mt-1 line-clamp-2 w-full leading-snug text-gray-400 dark:text-gray-500"
                          style={{ fontSize: `${9.5 * fontScale}px` }}
                        >
                          {entry.description}
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              ))}
            </>
          )}
          <WidgetSkeletonCover
            active={loading}
            preset="media-row"
            accent="#f97316"
            label={t.common.loading}
          />
        </WidgetShell>
      )
    }

    return (
      <WidgetShell
        containerRef={containerRef}
        scale={scale}
        padding={{ x: 14, y: 10 }}
        className={shellClassName}
        style={pointerEventsStyle}
        contentClassName="flex min-h-0 flex-col"
      >
        <button
          type="button"
          onClick={openPhantasi}
          className="group/header flex w-full shrink-0 cursor-pointer items-center gap-2 text-left"
          aria-label={t.friendLinksWidget.openPhantasi}
        >
          <span
            className="min-w-0 flex-1 truncate font-semibold text-gray-800 dark:text-gray-100"
            style={{ fontSize: `${14 * fontScale}px` }}
          >
            {t.widgets.friendLinks}
          </span>
          {isWide && !loading && !failed && (
            <span
              className="shrink-0 text-gray-400 dark:text-gray-500"
              style={{ fontSize: `${10 * fontScale}px` }}
            >
              {format(t.friendLinksWidget.siteCount, {
                count: entries.length,
              })}
            </span>
          )}
          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-gray-400 transition-transform group-hover/header:translate-x-0.5 dark:text-gray-500" />
        </button>

        <div className="relative mt-2 min-h-0 flex-1 overflow-hidden">
          {failed ? (
            <div className="absolute inset-0 flex items-center justify-center text-center text-xs text-gray-400 dark:text-gray-500">
              {t.friendLinksWidget.loadFailed}
            </div>
          ) : !loading && visibleEntries.length === 0 ? (
            <button
              type="button"
              onClick={openPhantasi}
              className="absolute inset-0 flex cursor-pointer flex-col items-center justify-center rounded-lg border border-dashed border-black/8 px-3 text-center dark:border-white/10"
            >
              <span className="text-xs font-medium text-gray-500 dark:text-gray-400">
                {t.friendLinksWidget.emptyTitle}
              </span>
              <span className="mt-0.5 text-[10px] text-gray-400 dark:text-gray-500">
                {t.friendLinksWidget.emptyDescription}
              </span>
            </button>
          ) : (
            <>
              {[
                {
                  id: `current-${batchIndex}`,
                  entries: visibleEntries,
                  incoming: false,
                },
                ...(incomingBatchIndex === null
                  ? []
                  : [
                      {
                        id: `incoming-${incomingBatchIndex}`,
                        entries: incomingEntries,
                        incoming: true,
                      },
                    ]),
              ].map((layer) => (
                <div
                  key={layer.id}
                  className={`absolute inset-0 grid gap-1.5 ${
                    isWide ? 'grid-cols-2' : 'grid-cols-1'
                  } ${
                    layer.incoming
                      ? 'friend-links-batch-enter pointer-events-none'
                      : incomingBatchIndex === null
                        ? ''
                        : 'friend-links-batch-exit'
                  }`}
                  aria-hidden={layer.incoming || undefined}
                >
                  {layer.entries.map((entry, entryIndex) => (
                    <button
                      key={entry.id}
                      type="button"
                      style={
                        {
                          '--friend-links-entry-index': entryIndex,
                        } as CSSProperties
                      }
                      onClick={() => openFriendLink(entry)}
                      disabled={entryDisabled(entry.url, layer.incoming)}
                      className="friend-links-entry group/link flex min-h-0 cursor-pointer items-center gap-2 overflow-hidden rounded-lg bg-black/3 px-2 text-left transition-colors hover:bg-black/6 disabled:cursor-default disabled:opacity-100 dark:bg-white/4 dark:hover:bg-white/8"
                      aria-label={format(t.friendLinksWidget.visitSite, {
                        name: entry.name,
                      })}
                    >
                      <FriendLinkIcon
                        icon={entry.icon}
                        color={entry.color}
                        className="relative flex h-7 w-7 shrink-0 items-center justify-center overflow-hidden rounded-md"
                        iconClassName="h-3.5 w-3.5"
                      />
                      <span className="min-w-0 flex-1">
                        <span
                          className="block truncate font-medium leading-tight text-gray-700 dark:text-gray-200"
                          style={{ fontSize: `${12 * fontScale}px` }}
                        >
                          {entry.name}
                        </span>
                        {entry.description && (
                          <span
                            className="mt-0.5 block truncate leading-tight text-gray-400 dark:text-gray-500"
                            style={{ fontSize: `${9 * fontScale}px` }}
                          >
                            {entry.description}
                          </span>
                        )}
                      </span>
                      <ExternalLink className="h-3 w-3 shrink-0 text-gray-300 opacity-0 transition-opacity group-hover/link:opacity-100 dark:text-gray-600" />
                    </button>
                  ))}
                </div>
              ))}
            </>
          )}
          <WidgetSkeletonCover
            active={loading}
            preset="list"
            rows={batchSize}
            accent="#f97316"
            label={t.common.loading}
          />
        </div>
      </WidgetShell>
    )
  },
)

FriendLinksWidget.displayName = 'FriendLinksWidget'
