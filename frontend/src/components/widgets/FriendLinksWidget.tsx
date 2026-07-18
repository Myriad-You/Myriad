/**
 * Brew 友情链接小组件
 *
 * 直接读取 Brew 的订阅源列表，并复用数据库中的固定分类值“友情链接”。
 * 因此 Brew 中的增删、排序、图标、主题色和标签都会同步到这里。
 */

import type { CSSProperties } from 'react'
import type { BrewSource } from '../../types/brew'
import type { WidgetComponentProps } from '../WidgetGrid'

import {
  LuChevronRight as ChevronRight,
  LuExternalLink as ExternalLink,
  LuLink as Link,
} from '@lib/icons'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useI18n } from '../../contexts/I18nContext'
import { useHomeVisibilityInterval } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { getSources } from '../../services/brewApi'
import { getIconUrl } from '../brew/constants'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'
import './FriendLinksWidget.css'

const FRIEND_LINK_CATEGORY = '友情链接'
const REFRESH_INTERVAL = 60 * 1000
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

function belongsToFriendLinks(source: BrewSource): boolean {
  return Boolean(
    source.category
      ?.split(',')
      .map((category) => category.trim())
      .includes(FRIEND_LINK_CATEGORY),
  )
}

function compareSources(a: BrewSource, b: BrewSource): number {
  const aHasOrder = typeof a.sort_order === 'number'
  const bHasOrder = typeof b.sort_order === 'number'
  if (aHasOrder && bHasOrder && a.sort_order !== b.sort_order) {
    return a.sort_order! - b.sort_order!
  }
  if (aHasOrder !== bHasOrder) return aHasOrder ? -1 : 1
  return a.name.localeCompare(b.name, 'zh-CN')
}

function safeLink(source: BrewSource): string {
  const target = source.site_url || source.url
  return /^https?:\/\//i.test(target) ? target : ''
}

function sourceSubtitle(source: BrewSource): string | null {
  const tag = source.ai_style_tags?.find((item) => item.trim())?.trim()
  if (tag) return tag

  const description = source.description?.trim()
  if (description) return description

  try {
    return new URL(safeLink(source)).hostname.replace(/^www\./, '') || null
  } catch {
    return null
  }
}

function toEntry(source: BrewSource): FriendLinkEntry {
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

export const FriendLinksWidget = memo(
  ({ config, isEditMode, isPreview }: WidgetComponentProps) => {
    const { t } = useI18n()
    const navigate = useNavigate()
    const anim = useAnimationLevel()
    const { containerRef, scale, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const mountedRef = useRef(true)
    const [sources, setSources] = useState<BrewSource[]>([])
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
      }
    }, [])

    const loadFriendLinks = useCallback(async () => {
      if (isPreview) return
      try {
        const nextSources = await getSources()
        if (!mountedRef.current) return
        setSources(nextSources)
        setFailed(false)
      } catch (error) {
        console.error('[FriendLinksWidget] Failed to load Brew sources:', error)
        if (mountedRef.current) setFailed(true)
      } finally {
        if (mountedRef.current) setLoading(false)
      }
    }, [isPreview])

    useEffect(() => {
      void loadFriendLinks()
    }, [loadFriendLinks])

    useHomeVisibilityInterval(loadFriendLinks, REFRESH_INTERVAL, !isPreview)

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
        .sort(compareSources)
        .map(toEntry)
    }, [isPreview, previewEntries, sources])

    const isStrip = config.size === '4x1'
    const isWide = config.size === '4x2'
    const batchSize = isWide ? 4 : 1
    const randomizedEntries = useMemo(
      () =>
        [...entries].sort(
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
      if (anim.level === 'none' || anim.durationScale === 0) {
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
      !isPreview && !isEditMode && batchCount > 1,
    )

    const openBrew = useCallback(() => {
      if (!isEditMode && !isPreview) navigate('/brew')
    }, [isEditMode, isPreview, navigate])

    const openFriendLink = useCallback(
      (entry: FriendLinkEntry) => {
        if (isEditMode || isPreview || !entry.url) return
        window.open(entry.url, '_blank', 'noopener,noreferrer')
      },
      [isEditMode, isPreview],
    )

    // 编辑/预览时禁用指针事件，让父级 WidgetGrid 可以拖拽
    const pointerEventsStyle =
      isEditMode || isPreview ? { pointerEvents: 'none' as const } : {}
    const shellClassName = isEditMode ? 'cursor-grab' : undefined

    if (isStrip) {
      return (
        <WidgetShell
          containerRef={containerRef}
          scale={scale}
          padding={{ x: 9, y: 8 }}
          className={shellClassName}
          style={pointerEventsStyle}
          contentClassName="relative min-h-0 overflow-hidden"
          background={
            <GlowBackground
              color={visibleEntries[0]?.color || '#f97316'}
              animLevel={anim.level}
              shouldAnimate={false}
              variant="single-left"
              size="sm"
              opacity={0.24}
            />
          }
        >
          {loading ? (
            <span className="absolute inset-0 animate-pulse rounded-xl bg-black/4 dark:bg-white/5" />
          ) : failed ? (
            <span className="absolute inset-0 flex items-center justify-center truncate px-3 text-[10px] text-gray-400 dark:text-gray-500">
              {t.friendLinksWidget.loadFailed}
            </span>
          ) : visibleEntries.length === 0 ? (
            <button
              type="button"
              onClick={openBrew}
              disabled={isEditMode || isPreview}
              className="absolute inset-0 flex items-center justify-center rounded-xl border border-dashed border-black/8 px-3 text-[10px] text-gray-400 disabled:cursor-default dark:border-white/10 dark:text-gray-500"
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
                      ? 'friend-links-batch-enter'
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
                          '--friend-links-accent': entry.color,
                        } as CSSProperties
                      }
                      onClick={() => openFriendLink(entry)}
                      disabled={
                        layer.incoming || isEditMode || isPreview || !entry.url
                      }
                      className="friend-links-entry friend-links-spotlight group/link relative flex min-w-0 flex-1 items-center gap-3 overflow-hidden rounded-xl bg-black/3 px-3 text-left transition-colors hover:bg-black/5 disabled:cursor-default dark:bg-white/4 dark:hover:bg-white/7"
                      aria-label={t.friendLinksWidget.visitSite.replace(
                        '{name}',
                        entry.name,
                      )}
                    >
                      <span
                        className="relative z-10 flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-xl"
                        style={{
                          backgroundColor: entry.icon
                            ? 'transparent'
                            : `${entry.color}1f`,
                          color: entry.color,
                        }}
                      >
                        <Link className="h-5 w-5" />
                        {entry.icon && (
                          <img
                            src={entry.icon}
                            alt=""
                            draggable={false}
                            loading="lazy"
                            decoding="async"
                            className="absolute inset-0 h-full w-full object-cover"
                            onError={(event) => {
                              event.currentTarget.style.display = 'none'
                            }}
                          />
                        )}
                      </span>
                      <span className="relative z-10 min-w-0 flex-1">
                        <span className="mb-0.5 block text-[8px] font-medium tracking-wide text-gray-400 dark:text-gray-500">
                          {t.brew.friendLinks}
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
                      <ExternalLink className="relative z-10 h-3.5 w-3.5 shrink-0 text-gray-300 transition-all group-hover/link:-translate-y-0.5 group-hover/link:translate-x-0.5 group-hover/link:text-gray-500 dark:text-gray-600 dark:group-hover/link:text-gray-400" />
                    </button>
                  ))}
                </div>
              ))}
            </>
          )}
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
          background={
            <span
              className="friend-links-square-gradient absolute inset-0"
              style={
                {
                  '--friend-links-gradient-color':
                    incomingEntries[0]?.color ||
                    visibleEntries[0]?.color ||
                    '#f97316',
                } as CSSProperties
              }
              aria-hidden="true"
            />
          }
        >
          {loading ? (
            <span className="absolute inset-0 animate-pulse rounded-2xl bg-black/4 dark:bg-white/5" />
          ) : failed ? (
            <span className="absolute inset-0 flex items-center justify-center px-4 text-center text-[10px] text-gray-400 dark:text-gray-500">
              {t.friendLinksWidget.loadFailed}
            </span>
          ) : visibleEntries.length === 0 ? (
            <button
              type="button"
              onClick={openBrew}
              disabled={isEditMode || isPreview}
              className="absolute inset-0 flex flex-col items-center justify-center rounded-2xl border border-dashed border-black/8 px-4 text-center disabled:cursor-default dark:border-white/10"
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
                      ? 'friend-links-batch-enter'
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
                          '--friend-links-accent': entry.color,
                        } as CSSProperties
                      }
                      onClick={() => openFriendLink(entry)}
                      disabled={
                        layer.incoming || isEditMode || isPreview || !entry.url
                      }
                      className="friend-links-entry group/link relative flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center overflow-hidden p-3 text-center disabled:cursor-default"
                      aria-label={t.friendLinksWidget.visitSite.replace(
                        '{name}',
                        entry.name,
                      )}
                    >
                      <span
                        className="relative z-10 flex h-16 w-16 shrink-0 items-center justify-center overflow-hidden rounded-2xl"
                        style={{
                          backgroundColor: entry.icon
                            ? 'transparent'
                            : `${entry.color}1f`,
                          color: entry.color,
                        }}
                      >
                        <Link className="h-6 w-6" />
                        {entry.icon && (
                          <img
                            src={entry.icon}
                            alt=""
                            draggable={false}
                            loading="lazy"
                            decoding="async"
                            className="absolute inset-0 h-full w-full object-cover"
                            onError={(event) => {
                              event.currentTarget.style.display = 'none'
                            }}
                          />
                        )}
                      </span>
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
        background={
          <GlowBackground
            color="#f97316"
            animLevel={anim.level}
            shouldAnimate={anim.loop && !isEditMode}
            variant="dual"
            size="md"
            opacity={0.18}
          />
        }
      >
        <button
          type="button"
          onClick={openBrew}
          disabled={isEditMode || isPreview}
          className="group/header flex w-full shrink-0 items-center gap-2 text-left disabled:cursor-default"
          aria-label={t.friendLinksWidget.openBrew}
        >
          <span
            className="min-w-0 flex-1 truncate font-semibold text-gray-800 dark:text-gray-100"
            style={{ fontSize: `${14 * fontScale}px` }}
          >
            {t.brew.friendLinks}
          </span>
          {isWide && !loading && !failed && (
            <span
              className="shrink-0 text-gray-400 dark:text-gray-500"
              style={{ fontSize: `${10 * fontScale}px` }}
            >
              {t.friendLinksWidget.siteCount.replace(
                '{count}',
                String(entries.length),
              )}
            </span>
          )}
          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-gray-400 transition-transform group-hover/header:translate-x-0.5 dark:text-gray-500" />
        </button>

        <div className="relative mt-2 min-h-0 flex-1 overflow-hidden">
          {loading ? (
            <div
              className={`absolute inset-0 grid gap-1.5 ${isWide ? 'grid-cols-2' : 'grid-cols-1'}`}
            >
              {Array.from({ length: batchSize }, (_, index) => (
                <div
                  key={index}
                  className="min-h-0 animate-pulse rounded-lg bg-black/4 dark:bg-white/5"
                />
              ))}
            </div>
          ) : failed ? (
            <div className="absolute inset-0 flex items-center justify-center text-center text-xs text-gray-400 dark:text-gray-500">
              {t.friendLinksWidget.loadFailed}
            </div>
          ) : visibleEntries.length === 0 ? (
            <button
              type="button"
              onClick={openBrew}
              disabled={isEditMode || isPreview}
              className="absolute inset-0 flex flex-col items-center justify-center rounded-lg border border-dashed border-black/8 px-3 text-center dark:border-white/10"
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
                      ? 'friend-links-batch-enter'
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
                      disabled={
                        layer.incoming || isEditMode || isPreview || !entry.url
                      }
                      className="friend-links-entry group/link flex min-h-0 items-center gap-2 overflow-hidden rounded-lg bg-black/3 px-2 text-left transition-colors hover:bg-black/6 disabled:cursor-default dark:bg-white/4 dark:hover:bg-white/8"
                      aria-label={t.friendLinksWidget.visitSite.replace(
                        '{name}',
                        entry.name,
                      )}
                    >
                      <span
                        className="relative flex h-7 w-7 shrink-0 items-center justify-center overflow-hidden rounded-lg"
                        style={{
                          backgroundColor: entry.icon
                            ? 'transparent'
                            : `${entry.color}1f`,
                          color: entry.color,
                        }}
                      >
                        <Link className="h-3.5 w-3.5" />
                        {entry.icon && (
                          <img
                            src={entry.icon}
                            alt=""
                            draggable={false}
                            loading="lazy"
                            decoding="async"
                            className="absolute inset-0 h-full w-full object-cover"
                            onError={(event) => {
                              event.currentTarget.style.display = 'none'
                            }}
                          />
                        )}
                      </span>
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
        </div>
      </WidgetShell>
    )
  },
)

FriendLinksWidget.displayName = 'FriendLinksWidget'
