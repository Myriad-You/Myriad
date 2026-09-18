/** 首页精选：跨源挑最近文章。点标题进阅读器，点空白进手帐。 */

import type { KeyboardEvent, MouseEvent } from 'react'
import type { PhantasiSource } from '../../../types/phantasi'
import type { WidgetComponentProps } from '../../widgetGridTypes'
import type { PhantasiTileSize } from '../logic/layout'
import type { FeaturedPick } from './featuredPicks'

import { memo, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useI18n, withI18nNamespace } from '../../../contexts/I18nContext'
import { useWidgetSize } from '../../../hooks/useWidgetSize'
import { formatMessage, localeOrFallback } from '../../../i18n'
import { WidgetSkeletonCover } from '../../widgets/shared/WidgetSkeleton'
import {
  DEFAULT_THEME_COLOR,
  getPlainText,
} from '../constants'
import { JOURNAL_ROOT, journalItemPath } from '../logic/journalRoutes'
import { downgradeForBand } from '../logic/layout'
import { trackPhantasi } from '../phantasiTrack'
import { collectFeaturedPicks } from './featuredPicks'
import { MinorRow } from './MinorRow'
import { TileCover } from './TileCover'
import { TileMeta, TileShell } from './TileShell'
import {
  COVER_H_FEATURED,
  fs,
  sp,
  SPLIT_MEDIA_WIDTH,
  T_FEATURED_BODY,
  T_FEATURED_META,
  T_FEATURED_TITLE,
} from './tokens'
import { useWidgetSources } from './useWidgetSources'

interface PhantasiFeaturedTileProps {
  size: PhantasiTileSize
  scale: number
  fontScale: number
  containerRef?: React.Ref<HTMLDivElement>
  sources: PhantasiSource[]
  now: number
  kicker?: string
  emptyHint?: string
  loading?: boolean
  failed?: boolean
  picks?: FeaturedPick[]
  onOpenJournal?: () => void
  onOpenItem?: (pick: FeaturedPick) => void
}

function relTime(
  ts: number | null | undefined,
  now: number,
  t: {
    justNow: string
    minutesAgo: string
    hoursAgo: string
    daysAgo: string
  },
  locale: string,
): string {
  if (!ts) return ''
  const diff = now - ts
  if (diff < 60_000) return t.justNow
  const loc = localeOrFallback(locale)
  if (diff < 3_600_000) {
    return formatMessage(loc, t.minutesAgo, {
      minutes: Math.floor(diff / 60_000),
    })
  }
  if (diff < 86_400_000) {
    return formatMessage(loc, t.hoursAgo, {
      hours: Math.floor(diff / 3_600_000),
    })
  }
  if (diff < 604_800_000) {
    return formatMessage(loc, t.daysAgo, {
      days: Math.floor(diff / 86_400_000),
    })
  }
  return new Date(ts).toLocaleDateString(locale, {
    month: 'short',
    day: 'numeric',
  })
}

function itemLinkProps(open?: () => void, label?: string) {
  if (!open) return {}
  return {
    role: 'link' as const,
    tabIndex: 0,
    'aria-label': label,
    onClick: (e: MouseEvent) => {
      e.stopPropagation()
      open()
    },
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key !== 'Enter' && e.key !== ' ') return
      e.preventDefault()
      e.stopPropagation()
      open()
    },
  }
}

export const PhantasiFeaturedTile = memo(
  ({
    size,
    scale,
    fontScale,
    containerRef,
    sources,
    now,
    kicker,
    emptyHint,
    loading = false,
    failed = false,
    picks: picksOverride,
    onOpenJournal,
    onOpenItem,
  }: PhantasiFeaturedTileProps) => {
    const { t, locale } = useI18n()
    const picks = useMemo(
      () => picksOverride ?? collectFeaturedPicks(sources),
      [picksOverride, sources],
    )
    const hero = picks[0]
    const rest = picks.slice(1)
    const isWide = size !== '4x4'
    const color = hero?.source_color ?? DEFAULT_THEME_COLOR
    const timeKeys = {
      justNow: t.phantasi.justNow,
      minutesAgo: t.phantasi.minutesAgo,
      hoursAgo: t.phantasi.hoursAgo,
      daysAgo: t.phantasi.daysAgo,
    }

    if (loading) {
      return (
        <TileShell
          color={DEFAULT_THEME_COLOR}
          scale={scale}
          containerRef={containerRef}
          glow="none"
          contentClassName="relative min-h-0"
        >
          <WidgetSkeletonCover
            active
            preset={isWide ? 'media-row' : 'hero'}
            deferMs={0}
          />
        </TileShell>
      )
    }

    if (failed || !hero) {
      return (
        <TileShell
          color={DEFAULT_THEME_COLOR}
          scale={scale}
          containerRef={containerRef}
          glow="none"
          label={onOpenJournal ? t.phantasi.featuredOpenJournal : kicker}
          onClick={onOpenJournal}
          contentClassName="flex min-h-0 items-center justify-center"
        >
          <span
            className="text-center text-gray-400 dark:text-gray-500"
            style={{ fontSize: fs(T_FEATURED_BODY, fontScale), lineHeight: 1.5 }}
          >
            {emptyHint}
          </span>
        </TileShell>
      )
    }

    const openHero = onOpenItem ? () => onOpenItem(hero) : undefined
    const heroLabel = openHero
      ? formatMessage(localeOrFallback(locale), t.phantasi.featuredOpenArticle, {
          title: hero.title,
        })
      : undefined
    const summary = getPlainText(hero.summary)
    const when = relTime(hero.published_at, now, timeKeys, locale)
    const sideCount = isWide ? (hero.image ? 1 : 2) : hero.image ? 2 : 3
    const side = rest.slice(0, sideCount)

    const heroCopy = (
      <div
        className="flex min-w-0 flex-col"
        style={{ gap: sp(isWide ? 4 : 5, scale) }}
      >
        <span
          className={`min-w-0 font-semibold text-gray-800 dark:text-gray-100 ${openHero ? 'cursor-pointer' : ''}`}
          style={{
            fontSize: fs(T_FEATURED_TITLE, fontScale),
            lineHeight: 1.35,
            display: '-webkit-box',
            WebkitBoxOrient: 'vertical',
            WebkitLineClamp: isWide ? 2 : hero.image ? 2 : 3,
            overflow: 'hidden',
          }}
          {...itemLinkProps(openHero, heroLabel)}
        >
          {hero.title}
        </span>
        <TileMeta fontScale={fontScale} scale={scale} size={T_FEATURED_META}>
          <span className="truncate">{hero.source_name}</span>
          {when ? (
            <>
              <span aria-hidden>·</span>
              <span>{when}</span>
            </>
          ) : null}
        </TileMeta>
        {!isWide && summary ? (
          <span
            className="text-gray-400 dark:text-gray-500"
            style={{
              fontSize: fs(T_FEATURED_BODY, fontScale),
              lineHeight: 1.45,
              display: '-webkit-box',
              WebkitBoxOrient: 'vertical',
              WebkitLineClamp: hero.image ? 1 : 3,
              overflow: 'hidden',
            }}
          >
            {summary}
          </span>
        ) : null}
      </div>
    )

    const sideList =
      side.length > 0 ? (
        <div
          className="flex min-w-0 flex-col"
          style={{ gap: sp(isWide ? 5 : 6, scale) }}
        >
          {side.map((item, i) => (
            <MinorRow
              key={item.id}
              title={item.title}
              time={item.source_name}
              scale={scale}
              fontScale={fontScale}
              titleSize={T_FEATURED_BODY}
              metaSize={T_FEATURED_META}
              dim={item.is_read || i > 0 ? 2 : 1}
              onClick={onOpenItem ? () => onOpenItem(item) : undefined}
            />
          ))}
        </div>
      ) : null

    if (isWide) {
      return (
        <TileShell
          color={color}
          scale={scale}
          containerRef={containerRef}
          label={kicker}
          onClick={onOpenJournal}
          contentClassName={
            hero.image
              ? 'flex min-h-0 flex-row items-stretch'
              : 'flex min-h-0 flex-col justify-center'
          }
        >
          {hero.image ? (
            <div
              className={`shrink-0 ${openHero ? 'cursor-pointer' : ''}`}
              style={{ width: SPLIT_MEDIA_WIDTH, paddingRight: sp(12, scale) }}
              {...itemLinkProps(openHero, heroLabel)}
            >
              <TileCover image={hero.image} className="h-full w-full" />
            </div>
          ) : null}
          <div className="flex min-w-0 flex-1 flex-col justify-center">
            {heroCopy}
            {sideList ? (
              <div style={{ marginTop: sp(10, scale) }}>{sideList}</div>
            ) : null}
          </div>
        </TileShell>
      )
    }

    return (
      <TileShell
        color={color}
        scale={scale}
        containerRef={containerRef}
        label={kicker}
        onClick={onOpenJournal}
        glow="dual"
      >
        {kicker ? (
          <span
            className="min-w-0 truncate text-gray-400/80 dark:text-gray-500/80"
            style={{
              fontSize: fs(T_FEATURED_META, fontScale),
              lineHeight: 1.2,
              marginBottom: sp(8, scale),
            }}
          >
            {kicker}
          </span>
        ) : null}
        {hero.image ? (
          <div
            className={openHero ? 'cursor-pointer' : undefined}
            style={{ marginBottom: sp(10, scale) }}
            {...itemLinkProps(openHero, heroLabel)}
          >
            <TileCover
              image={hero.image}
              height={sp(COVER_H_FEATURED, scale)}
            />
          </div>
        ) : null}
        <div className="flex min-h-0 flex-1 flex-col">
          {heroCopy}
          {sideList ? (
            <div className="mt-auto" style={{ paddingTop: sp(12, scale) }}>
              {sideList}
            </div>
          ) : null}
        </div>
      </TileShell>
    )
  },
)

PhantasiFeaturedTile.displayName = 'PhantasiFeaturedTile'

const SAMPLE_COVER = `data:image/svg+xml;utf8,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180">' +
    '<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">' +
    '<stop offset="0" stop-color="#f97316"/><stop offset="1" stop-color="#6366f1"/>' +
    '</linearGradient></defs><rect width="320" height="180" fill="url(#g)"/></svg>',
)}`

function samplePicks(t: {
  featuredSampleHero: string
  featuredSampleSecond: string
  featuredSampleThird: string
  featuredSampleFourth: string
  featuredSampleSource: string
}): FeaturedPick[] {
  const now = Date.now()
  return [
    {
      id: -1,
      title: t.featuredSampleHero,
      summary: t.featuredSampleSecond,
      image: SAMPLE_COVER,
      published_at: now - 3_600_000,
      is_read: false,
      source_id: -1,
      source_name: t.featuredSampleSource,
      source_icon: null,
      source_color: '#f97316',
    },
    {
      id: -2,
      title: t.featuredSampleSecond,
      summary: null,
      image: null,
      published_at: now - 7_200_000,
      is_read: false,
      source_id: -2,
      source_name: t.featuredSampleSource,
      source_icon: null,
      source_color: '#0ea5e9',
    },
    {
      id: -3,
      title: t.featuredSampleThird,
      summary: null,
      image: null,
      published_at: now - 14_400_000,
      is_read: false,
      source_id: -3,
      source_name: t.featuredSampleSource,
      source_icon: null,
      source_color: '#8b5cf6',
    },
    {
      id: -4,
      title: t.featuredSampleFourth,
      summary: null,
      image: null,
      published_at: now - 28_800_000,
      is_read: false,
      source_id: -1,
      source_name: t.featuredSampleSource,
      source_icon: null,
      source_color: '#f97316',
    },
  ]
}

const PhantasiFeaturedWidgetBody = memo(
  ({ config, isEditMode, isPreview }: WidgetComponentProps) => {
    const { t } = useI18n()
    const navigate = useNavigate()
    const { containerRef, scale, fontScale, viewportBand } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const { sources, loading, failed } = useWidgetSources(
      isPreview ?? false,
      '[PhantasiFeaturedWidget]',
    )
    const [now] = useState(() => Date.now())
    const size = downgradeForBand(
      (config.size === '4x4' ? '4x4' : '4x2') as PhantasiTileSize,
      viewportBand,
    )
    const locked = isEditMode || isPreview
    const previewPicks = useMemo(
      () => (isPreview ? samplePicks(t.phantasi) : undefined),
      [isPreview, t.phantasi],
    )
    const emptyHint = failed
      ? t.phantasi.featuredLoadFailed
      : sources.length === 0
        ? t.phantasi.emptyNoSources
        : t.phantasi.featuredEmpty

    return (
      <div
        className="h-full w-full"
        style={locked ? { pointerEvents: 'none' } : undefined}
      >
        <PhantasiFeaturedTile
          size={size}
          scale={scale}
          fontScale={fontScale}
          containerRef={containerRef}
          sources={sources}
          picks={previewPicks}
          now={now}
          loading={isPreview ? false : loading}
          failed={isPreview ? false : failed}
          kicker={t.widgets.phantasiFeatured}
          emptyHint={emptyHint}
          onOpenJournal={
            locked
              ? undefined
              : () => navigate(JOURNAL_ROOT)
          }
          onOpenItem={
            locked
              ? undefined
              : (pick) => {
                  trackPhantasi('PHANTASI_OPEN_ITEM', pick.source_id, 1500)
                  navigate(journalItemPath(pick.id))
                }
          }
        />
      </div>
    )
  },
)

PhantasiFeaturedWidgetBody.displayName = 'PhantasiFeaturedWidgetBody'

export const PhantasiFeaturedWidget = withI18nNamespace(
  ['phantasi'],
  PhantasiFeaturedWidgetBody,
)
