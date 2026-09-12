/** 窄屏降档在 layout.downgradeForBand。2x1 仅 icon。 */

import type { KeyboardEvent, MouseEvent } from 'react'
import type { BrewItemPreview, BrewSource } from '../../../types/brew'
import type { WidgetComponentProps } from '../../widgetGridTypes'
import type { BrewTileLayout, BrewTileSize } from '../logic/layout'

import type { BrewViewerRole } from '../logic/score'
import { LuExternalLink as ExternalLink } from '@lib/icons'
import { memo, useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useAuth } from '../../../contexts/AuthContext'
import { useI18n } from '../../../contexts/I18nContext'
import { isExlight, useAnimationLevel } from '../../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../../hooks/useWidgetSize'
import { formatMessage, localeOrFallback } from '../../../i18n'
import { extractColorsFromLoadedImage } from '../../../utils/colorExtractor'
import {
  DEFAULT_THEME_COLOR,
  getIconUrl,
  getPlainText,
  normalizeThemeColor,
} from '../constants'
import { downgradeForBand, tileLayout } from '../logic/layout'
import { roleFromAuth, sortByScore } from '../logic/score'
import { Cadence, CADENCE_WINDOW_DAYS, pulsesFromTimestamps } from './Cadence'
import { MinorRow } from './MinorRow'
import { TileCover } from './TileCover'
import { TileHeader, TileMark, TileMeta, TileShell } from './TileShell'
import {
  COVER_H_FEATURE,
  fs,
  heroNumberSize,
  ICON_MARK_SIZE,
  LEAD_THUMB_SIZE,
  sp,
  SPLIT_MEDIA_WIDTH,
  T_META,
  T_MINOR,
  T_TITLE,
} from './tokens'
import { useWidgetSources } from './useWidgetSources'
import './rotation.css'

const LIST_PAGE_4X4 = 5
const LIST_PAGE_4X2 = 4
const ROTATE_PAGE_MS = 5600
const ROTATE_MAX_PAGES = 4
const WIDGET_REFRESH_INTERVAL = 60 * 1000

export interface BrewSourceTileProps {
  source: BrewSource
  size: BrewTileSize
  role: BrewViewerRole
  /** 会话内冻结时钟，构图才不会自己跳。 */
  now: number
  scale: number
  fontScale: number
  containerRef?: React.Ref<HTMLDivElement>
  onOpenSource?: (source: BrewSource) => void
  onOpenItem?: (item: BrewItemPreview, source: BrewSource) => void
  items?: BrewItemPreview[]
  layoutOverride?: BrewTileLayout
  /** icon 卡在编辑态走 onOpenSource，不 window.open。 */
  editMode?: boolean
  onThemeColorExtracted?: (sourceId: number, color: string) => void
  surface?: 'glass' | 'solid'
}

/** 只吃 t.brew 的四个相对时间键。 */
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
  // 相对时间跟界面语言，不跟浏览器。
  return new Date(ts).toLocaleDateString(locale, {
    month: 'short',
    day: 'numeric',
  })
}

function siteHost(source: BrewSource): string {
  try {
    return new URL(source.site_url || source.url).hostname.replaceAll(/^www\./g, '')
  } catch {
    return ''
  }
}

/** 只保留满页，避免轮播时行距乱跳。 */
function paginate<T>(items: T[], pageSize: number): T[][] {
  if (items.length < pageSize) return items.length > 0 ? [items] : []
  const pages: T[][] = []
  for (let i = 0; i + pageSize <= items.length; i += pageSize) {
    pages.push(items.slice(i, i + pageSize))
    if (pages.length >= ROTATE_MAX_PAGES) break
  }
  return pages
}

export const BrewSourceTile = memo(
  ({
    source,
    size,
    role,
    now,
    scale,
    fontScale,
    containerRef,
    onOpenSource,
    onOpenItem,
    items,
    layoutOverride,
    editMode = false,
    onThemeColorExtracted,
    surface,
  }: BrewSourceTileProps) => {
    const { t, locale, format } = useI18n()
    const anim = useAnimationLevel()
    const color = normalizeThemeColor(source.theme_color)
    const icon = getIconUrl(source.icon)

    const list = useMemo(
      () => items ?? source.recent_items ?? [],
      [items, source.recent_items],
    )
    const layout: BrewTileLayout =
      layoutOverride ?? tileLayout(source, role, now, list.length)

    // 未读只在登录后渲染。
    const unread = role === 'guest' ? null : source.unread_count
    // 失败态只在管理员视图报警。
    const alert = role === 'admin' && source.error_count > 0

    const timeKeys = {
      justNow: t.brew.justNow,
      minutesAgo: t.brew.minutesAgo,
      hoursAgo: t.brew.hoursAgo,
      daysAgo: t.brew.daysAgo,
    }

    // 没 theme_color 时从图标提主色写回。
    const handleIconLoad = useCallback(
      (img: HTMLImageElement) => {
        if (source.theme_color || !source.icon || !onThemeColorExtracted) return
        try {
          const palette = extractColorsFromLoadedImage(img)
          if (
            palette.primary &&
            palette.primary !== DEFAULT_THEME_COLOR &&
            palette.primary !== '#6b7280'
          ) {
            onThemeColorExtracted(source.id, palette.primary)
          }
        } catch {
        }
      },
      [source.id, source.theme_color, source.icon, onThemeColorExtracted],
    )

    const header = (
      <TileHeader
        name={source.name}
        color={color}
        scale={scale}
        fontScale={fontScale}
        icon={icon}
        unread={unread}
        alert={alert}
        onIconLoad={handleIconLoad}
      />
    )

    const openSource = onOpenSource ? () => onOpenSource(source) : undefined
    const openItem = (item: BrewItemPreview) => onOpenItem?.(item, source)
    /** 头条可点语义：role=link，Enter / 空格触发。 */
    const leadLinkProps = (item: BrewItemPreview) =>
      onOpenItem
        ? {
            role: 'link' as const,
            tabIndex: 0,
            onClick: (e: MouseEvent) => {
              e.stopPropagation()
              openItem(item)
            },
            onKeyDown: (e: KeyboardEvent) => {
              if (e.key !== 'Enter' && e.key !== ' ') return
              e.preventDefault()
              e.stopPropagation()
              openItem(item)
            },
          }
        : {}

    // 2×2 不放封面、不拆栏。
    const isSmall = size === '2x2'

    // icon 整卡开外站，不进阅读器。
    if (layout === 'icon') {
      const host = siteHost(source)
      const target = source.site_url || source.url
      const desc = source.description?.trim() || host
      const tag = source.ai_style_tags?.find((x) => x.trim())?.trim()
      const openTarget =
        editMode
          ? openSource
          : /^https?:\/\//i.test(target)
            ? () => window.open(target, '_blank', 'noopener,noreferrer')
            : undefined

      if (size === '2x1') {
        return (
          <TileShell
            color={color}
            surface={surface}
            scale={scale}
            padding={{ x: 10, y: 8 }}
            containerRef={containerRef}
            label={source.name}
            glow="single"
            contentClassName="flex min-h-0 items-center"
            onClick={openTarget}
          >
            <div
              className="flex min-w-0 flex-1 items-center"
              style={{ gap: sp(8, scale) }}
            >
              <TileMark
                name={source.name}
                color={color}
                scale={scale}
                size={ICON_MARK_SIZE * 0.56}
                icon={icon}
                onIconLoad={handleIconLoad}
              />
              <span
                className="min-w-0 flex-1 truncate font-semibold text-gray-800 dark:text-gray-100"
                style={{ fontSize: fs(T_MINOR, fontScale), lineHeight: 1.25 }}
              >
                {source.name}
              </span>
            </div>
          </TileShell>
        )
      }

      return (
        <TileShell
          color={color}
          surface={surface}
          scale={scale}
          containerRef={containerRef}
          label={source.name}
          glow="single"
          contentClassName="flex min-h-0 items-center"
          onClick={openTarget}
        >
          <div
            className="flex min-w-0 flex-1 items-center"
            style={{ gap: sp(11, scale) }}
          >
            <TileMark
              name={source.name}
              color={color}
              scale={scale}
              size={size === '2x2' ? ICON_MARK_SIZE * 0.7 : ICON_MARK_SIZE}
              icon={icon}
              onIconLoad={handleIconLoad}
            />
            <div
              className="flex min-w-0 flex-1 flex-col"
              style={{ gap: sp(3, scale) }}
            >
              <div
                className="flex min-w-0 items-center"
                style={{ gap: sp(4, scale) }}
              >
                <span
                  className="min-w-0 truncate font-semibold text-gray-800 dark:text-gray-100"
                  style={{ fontSize: fs(T_TITLE, fontScale), lineHeight: 1.25 }}
                >
                  {source.name}
                </span>
                <ExternalLink
                  className="shrink-0 text-gray-400 dark:text-gray-500"
                  style={{ width: sp(11, scale), height: sp(11, scale) }}
                  aria-label={t.brew.tileOpenSite}
                />
              </div>
              {desc ? (
                <span
                  className="text-gray-500 dark:text-gray-400"
                  style={{
                    fontSize: fs(T_MINOR, fontScale),
                    lineHeight: 1.4,
                    display: '-webkit-box',
                    WebkitBoxOrient: 'vertical',
                    WebkitLineClamp: size === '2x2' ? 2 : 3,
                    overflow: 'hidden',
                  }}
                >
                  {desc}
                </span>
              ) : null}
              {tag && size !== '2x2' ? (
                <TileMeta fontScale={fontScale} scale={scale}>
                  <span>{tag}</span>
                </TileMeta>
              ) : null}
            </div>
          </div>
        </TileShell>
      )
    }

    if (layout === 'numeric') {
      const heroSize = heroNumberSize(size)
      const rows = size === '4x4' ? list.slice(0, 5) : list.slice(0, 4)
      const hero = (
        <div
          className="flex min-w-0 items-baseline"
          style={{ gap: sp(4, scale) }}
        >
          <span
            className={alert ? 'text-red-500 dark:text-red-400' : ''}
            style={{
              fontSize: fs(heroSize, fontScale),
              lineHeight: 1,
              fontWeight: 700,
              letterSpacing: '-0.02em',
              ...(alert ? {} : { color }),
            }}
          >
            {source.unread_count}
          </span>
          <span
            className="text-gray-400 dark:text-gray-500"
            style={{ fontSize: fs(T_META, fontScale), lineHeight: 1 }}
          >
            {t.brew.tileUnreadItems}
          </span>
        </div>
      )

      if (isSmall) {
        return (
          <TileShell
            color={color}
            surface={surface}
            scale={scale}
            containerRef={containerRef}
            label={source.name}
            onClick={openSource}
            contentClassName="flex min-h-0 flex-col justify-center"
          >
            {hero}
            <span
              className="mt-1 truncate font-medium text-gray-500 dark:text-gray-400"
              style={{ fontSize: fs(T_MINOR, fontScale) }}
            >
              {source.name}
            </span>
          </TileShell>
        )
      }

      if (size !== '4x4') {
        return (
          <TileShell
            color={color}
            surface={surface}
            scale={scale}
            containerRef={containerRef}
            label={source.name}
            onClick={openSource}
            contentClassName="flex min-h-0 flex-row items-stretch"
          >
            <div
              className="flex min-w-0 shrink-0 flex-col justify-center"
              style={{ width: SPLIT_MEDIA_WIDTH, paddingRight: sp(10, scale) }}
            >
              {hero}
              <span
                className="mt-1 truncate font-medium text-gray-500 dark:text-gray-400"
                style={{ fontSize: fs(T_MINOR, fontScale) }}
              >
                {source.name}
              </span>
            </div>
            <div
              className="flex min-w-0 flex-1 flex-col justify-center"
              style={{ gap: sp(4, scale) }}
            >
              {rows.map((item, i) => (
                <MinorRow
                  key={item.id}
                  title={item.title}
                  scale={scale}
                  fontScale={fontScale}
                  dim={i >= 2 ? 1 : 0}
                  onClick={onOpenItem ? () => openItem(item) : undefined}
                />
              ))}
            </div>
          </TileShell>
        )
      }

      return (
        <TileShell
          color={color}
          surface={surface}
          scale={scale}
          containerRef={containerRef}
          label={source.name}
          onClick={openSource}
          glow="dual"
        >
          {header}
          <div style={{ marginBottom: sp(10, scale) }}>{hero}</div>
          {/* justify-evenly，避免下半张留洞。 */}
          <div className="flex min-h-0 flex-1 flex-col justify-evenly">
            {rows.map((item, i) => (
              <MinorRow
                key={item.id}
                title={item.title}
                time={relTime(item.published_at, now, timeKeys, locale)}
                scale={scale}
                fontScale={fontScale}
                dim={i >= 2 ? 1 : 0}
                onClick={onOpenItem ? () => openItem(item) : undefined}
              />
            ))}
          </div>
        </TileShell>
      )
    }

    if (layout === 'cadence') {
      const pulses =
        source.pulses && source.pulses.length > 0
          ? source.pulses
          : pulsesFromTimestamps(
              list.map((i) => i.published_at),
              now,
            )
      const oldest = pulses.length > 0 ? Math.max(...pulses) : 0
      const months = Math.max(1, Math.round(oldest / 30))
      const latest = list[0]

      const axis = (
        <TileMeta
          fontScale={fontScale}
          scale={scale}
          className="justify-between"
        >
          <span>
            {format(t.brew.tileQuietMonths, { months })}
          </span>
          <span className={alert ? 'text-red-500 dark:text-red-400' : ''}>
            {alert
              ? format(t.brew.tileFailedTimes, { count: source.error_count })
              : t.brew.tileToday}
          </span>
        </TileMeta>
      )

      if (isSmall) {
        return (
          <TileShell
            color={color}
            surface={surface}
            scale={scale}
            containerRef={containerRef}
            label={source.name}
            onClick={openSource}
            contentClassName="flex min-h-0 flex-col justify-between"
          >
            <span
              className="truncate font-semibold text-gray-800 dark:text-gray-100"
              style={{ fontSize: fs(T_TITLE, fontScale), lineHeight: 1.25 }}
            >
              {source.name}
            </span>
            <Cadence pulses={pulses} color={color} height={sp(30, scale)} />
            {axis}
          </TileShell>
        )
      }

      if (size !== '4x4') {
        return (
          <TileShell
            color={color}
            surface={surface}
            scale={scale}
            containerRef={containerRef}
            label={source.name}
            onClick={openSource}
            contentClassName="flex min-h-0 flex-row items-stretch"
          >
            <div
              className="flex shrink-0 flex-col justify-end"
              style={{ width: SPLIT_MEDIA_WIDTH, paddingRight: sp(10, scale) }}
            >
              <Cadence pulses={pulses} color={color} height={sp(34, scale)} />
            </div>
            <div
              className="flex min-w-0 flex-1 flex-col justify-center"
              style={{ gap: sp(4, scale) }}
            >
              <span
                className="truncate font-semibold text-gray-800 dark:text-gray-100"
                style={{ fontSize: fs(T_TITLE, fontScale), lineHeight: 1.25 }}
              >
                {source.name}
              </span>
              {latest ? (
                <span
                  className="truncate text-gray-500 dark:text-gray-400"
                  style={{ fontSize: fs(T_MINOR, fontScale) }}
                >
                  {latest.title}
                </span>
              ) : null}
              {axis}
            </div>
          </TileShell>
        )
      }

      return (
        <TileShell
          color={color}
          surface={surface}
          scale={scale}
          containerRef={containerRef}
          label={source.name}
          onClick={openSource}
        >
          {header}
          <div className="flex min-h-0 flex-1 flex-col justify-center">
            {/* 4×4 节律图不能只用 56px 高。 */}
            <Cadence pulses={pulses} color={color} height={sp(104, scale)} />
            <div style={{ marginTop: sp(6, scale) }}>{axis}</div>
          </div>
          {latest ? (
            <div style={{ marginTop: sp(8, scale) }}>
              <MinorRow
                title={latest.title}
                time={relTime(latest.published_at, now, timeKeys, locale)}
                scale={scale}
                fontScale={fontScale}
                onClick={onOpenItem ? () => openItem(latest) : undefined}
              />
            </div>
          ) : null}
        </TileShell>
      )
    }

    if (layout === 'list') {
      const pageSize = size === '4x4' ? LIST_PAGE_4X4 : LIST_PAGE_4X2
      const pages = paginate(list, pageSize)
      const rotating =
        pages.length > 1 && !isExlight(anim) && anim.widgetUiRotation
      // 负 delay 用 id 派生，同一张卡每次一样。
      const stagger = -((source.id * 1300) % (ROTATE_PAGE_MS * pages.length))

      // list 的 4×2 不拆栏、不放封面。
      if (size !== '4x4') {
        return (
          <TileShell
            color={color}
            surface={surface}
            scale={scale}
            containerRef={containerRef}
            label={source.name}
            onClick={openSource}
          >
            {header}
            <div className="relative min-h-0 flex-1 overflow-hidden">
              <div
                className={rotating ? 'brew-rotate-track' : undefined}
                style={
                  rotating
                    ? {
                        height: `${pages.length * 100}%`,
                        animationName: `brew-rotate-${pages.length}`,
                        animationDuration: `${(ROTATE_PAGE_MS * pages.length) / 1000}s`,
                        animationIterationCount: 'infinite',
                        animationTimingFunction: 'cubic-bezier(0.4, 0, 0.2, 1)',
                        animationDelay: `${stagger}ms`,
                      }
                    : // 不轮播也要 100% 高，避免轨道 auto 把 height:100% 塌成内容高。
                      { height: '100%' }
                }
              >
                {(rotating ? pages : pages.slice(0, 1)).map((page, pi) => (
                  <div
                    key={pi}
                    className="flex flex-col justify-evenly"
                    style={{
                      height: rotating ? `${100 / pages.length}%` : '100%',
                    }}
                  >
                    {page.map((item) => (
                      <MinorRow
                        key={item.id}
                        title={item.title}
                        time={relTime(item.published_at, now, timeKeys, locale)}
                        scale={scale}
                        fontScale={fontScale}
                        onClick={onOpenItem ? () => openItem(item) : undefined}
                      />
                    ))}
                  </div>
                ))}
              </div>
            </div>
          </TileShell>
        )
      }

      return (
        <TileShell
          color={color}
          surface={surface}
          scale={scale}
          containerRef={containerRef}
          label={source.name}
          onClick={openSource}
          glow="dual"
        >
          {header}
          <div className="relative min-h-0 flex-1 overflow-hidden">
            <div
              className={rotating ? 'brew-rotate-track' : undefined}
              style={
                rotating
                  ? {
                      height: `${pages.length * 100}%`,
                      animationName: `brew-rotate-${pages.length}`,
                      animationDuration: `${(ROTATE_PAGE_MS * pages.length) / 1000}s`,
                      animationIterationCount: 'infinite',
                      animationTimingFunction: 'cubic-bezier(0.4, 0, 0.2, 1)',
                      animationDelay: `${stagger}ms`,
                    }
                  : { height: '100%' }
              }
            >
              {(rotating ? pages : pages.slice(0, 1)).map((page, pi) => (
                <div
                  key={pi}
                  // 不满 5 条也铺满，别在下半张留洞。
                  className="flex flex-col justify-evenly"
                  style={{
                    height: rotating ? `${100 / pages.length}%` : '100%',
                  }}
                >
                  {page.map((item, i) =>
                    i === 0 ? (
                      <div
                        key={item.id}
                        className={`flex min-w-0 items-start ${onOpenItem ? 'cursor-pointer' : ''}`}
                        style={{ gap: sp(9, scale) }}
                        {...leadLinkProps(item)}
                      >
                        {/* 无图不画灰块，但行高要保住。 */}
                        <div
                          className="shrink-0"
                          style={{
                            width: item.image ? undefined : 0,
                            height: sp(LEAD_THUMB_SIZE, scale),
                          }}
                        >
                          <TileCover
                            image={item.image}
                            square={sp(LEAD_THUMB_SIZE, scale)}
                          />
                        </div>
                        <div
                          className="flex min-w-0 flex-1 flex-col"
                          style={{ gap: sp(3, scale) }}
                        >
                          <span
                            className="min-w-0 font-medium text-gray-800 dark:text-gray-100"
                            style={{
                              fontSize: fs(T_TITLE, fontScale),
                              lineHeight: 1.3,
                              display: '-webkit-box',
                              WebkitBoxOrient: 'vertical',
                              WebkitLineClamp: 2,
                              overflow: 'hidden',
                            }}
                          >
                            {item.title}
                          </span>
                          <TileMeta fontScale={fontScale} scale={scale}>
                            <span>
                              {relTime(
                                item.published_at,
                                now,
                                timeKeys,
                                locale,
                              )}
                            </span>
                          </TileMeta>
                        </div>
                      </div>
                    ) : (
                      <MinorRow
                        key={item.id}
                        title={item.title}
                        time={relTime(item.published_at, now, timeKeys, locale)}
                        scale={scale}
                        fontScale={fontScale}
                        dim={i >= 2 ? 1 : 0}
                        onClick={onOpenItem ? () => openItem(item) : undefined}
                      />
                    ),
                  )}
                </div>
              ))}
            </div>
          </div>
        </TileShell>
      )
    }

    const lead = list[0]
    const second = list[1]
    const summary = getPlainText(lead?.summary ?? null)
    const readingHint = lead?.published_at
      ? relTime(lead.published_at, now, timeKeys, locale)
      : ''

    if (size !== '4x4') {
      return (
        <TileShell
          color={color}
          surface={surface}
          scale={scale}
          containerRef={containerRef}
          label={source.name}
          onClick={openSource}
          contentClassName={
            isSmall
              ? 'flex min-h-0 flex-col justify-center'
              : 'flex min-h-0 flex-row items-stretch'
          }
        >
          {!isSmall && lead?.image ? (
            <div
              className="shrink-0"
              style={{ width: SPLIT_MEDIA_WIDTH, paddingRight: sp(10, scale) }}
            >
              <TileCover image={lead.image} className="h-full w-full" />
            </div>
          ) : null}
          <div
            className="flex min-w-0 flex-1 flex-col justify-center"
            style={{ gap: sp(4, scale) }}
          >
            <span
              className="truncate font-semibold text-gray-800 dark:text-gray-100"
              style={{ fontSize: fs(T_TITLE, fontScale), lineHeight: 1.25 }}
            >
              {source.name}
            </span>
            {lead ? (
              <span
                className="text-gray-600 dark:text-gray-300"
                style={{
                  fontSize: fs(T_MINOR, fontScale),
                  lineHeight: 1.4,
                  display: '-webkit-box',
                  WebkitBoxOrient: 'vertical',
                  WebkitLineClamp: 2,
                  overflow: 'hidden',
                }}
              >
                {lead.title}
              </span>
            ) : (
              <span
                className="text-gray-400 dark:text-gray-500"
                style={{ fontSize: fs(T_MINOR, fontScale) }}
              >
                {source.description?.trim() || siteHost(source)}
              </span>
            )}
            {readingHint ? (
              <TileMeta fontScale={fontScale} scale={scale}>
                <span>{readingHint}</span>
              </TileMeta>
            ) : null}
          </div>
        </TileShell>
      )
    }

    return (
      <TileShell
        color={color}
        surface={surface}
        scale={scale}
        containerRef={containerRef}
        label={source.name}
        onClick={openSource}
        glow="dual"
      >
        {header}
        {lead?.image ? (
          <div style={{ marginBottom: sp(9, scale) }}>
            <TileCover image={lead.image} height={sp(COVER_H_FEATURE, scale)} />
          </div>
        ) : null}
        <div
          className="flex min-h-0 flex-1 flex-col"
          style={{ gap: sp(5, scale) }}
        >
          {lead ? (
            <span
              className={`font-medium text-gray-800 dark:text-gray-100 ${onOpenItem ? 'cursor-pointer' : ''}`}
              style={{
                fontSize: fs(T_TITLE, fontScale),
                lineHeight: 1.35,
                display: '-webkit-box',
                WebkitBoxOrient: 'vertical',
                WebkitLineClamp: lead.image ? 2 : 3,
                overflow: 'hidden',
              }}
              {...leadLinkProps(lead)}
            >
              {lead.title}
            </span>
          ) : (
            // 无条目走纯文本，不画灰占位。
            <span
              className="text-gray-500 dark:text-gray-400"
              style={{
                fontSize: fs(T_MINOR, fontScale),
                lineHeight: 1.5,
                display: '-webkit-box',
                WebkitBoxOrient: 'vertical',
                WebkitLineClamp: 4,
                overflow: 'hidden',
              }}
            >
              {source.description?.trim() || siteHost(source)}
            </span>
          )}
          {/* 有封面也放摘要（clamp 2），避免卡中间空一块。 */}
          {summary ? (
            <span
              className="text-gray-500 dark:text-gray-400"
              style={{
                fontSize: fs(T_MINOR, fontScale),
                lineHeight: 1.5,
                display: '-webkit-box',
                WebkitBoxOrient: 'vertical',
                WebkitLineClamp: lead?.image ? 2 : 3,
                overflow: 'hidden',
              }}
            >
              {summary}
            </span>
          ) : null}
          <div className="mt-auto flex flex-col" style={{ gap: sp(5, scale) }}>
            {readingHint || lead?.published_at ? (
              <TileMeta fontScale={fontScale} scale={scale}>
                <span>{readingHint}</span>
              </TileMeta>
            ) : null}
            {second ? (
              <MinorRow
                title={second.title}
                time={relTime(second.published_at, now, timeKeys, locale)}
                scale={scale}
                fontScale={fontScale}
                dim={1}
                onClick={onOpenItem ? () => openItem(second) : undefined}
              />
            ) : null}
          </div>
        </div>
      </TileShell>
    )
  },
)

BrewSourceTile.displayName = 'BrewSourceTile'

export { CADENCE_WINDOW_DAYS }

/** 不新开 GET /source/:id，走 getSources() + find。 */
export const BrewSourceWidget = memo(
  ({ config, isEditMode, isPreview, onConfigChange }: WidgetComponentProps) => {
    const { t } = useI18n()
    const navigate = useNavigate()
    const { isAuthenticated, isAdmin } = useAuth()
    const role = roleFromAuth(isAuthenticated, isAdmin)
    const { containerRef, scale, fontScale, viewportBand } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const sources = useWidgetSources(
      isPreview ?? false,
      WIDGET_REFRESH_INTERVAL,
      '[BrewSourceWidget]',
    )
    // 会话内冻结时钟。
    const [now] = useState(() => Date.now())

    const sourceId = config.config?.sourceId as number | undefined

    const persist = useCallback(
      (nextId: number) => {
        const payload = { ...config.config, sourceId: nextId }
        if (typeof onConfigChange === 'function') {
          onConfigChange(payload)
        } else {
          window.dispatchEvent(
            new CustomEvent('widget-config-update', {
              detail: { widgetId: config.id, config: payload },
            }),
          )
        }
      },
      [config.config, config.id, onConfigChange],
    )

    const size = downgradeForBand(config.size as BrewTileSize, viewportBand)
    const source = sourceId
      ? sources.find((s) => s.id === sourceId)
      : isPreview
        ? sortByScore(sources, role, now)[0]
        : undefined
    const locked = isEditMode || isPreview

    if (!source) {
      const pickable = isEditMode && !isPreview && sources.length > 0
      return (
        <TileShell
          color={DEFAULT_THEME_COLOR}
          scale={scale}
          containerRef={containerRef}
          glow="none"
          contentClassName={
            pickable
              ? 'flex min-h-0 flex-col overflow-y-auto'
              : 'flex min-h-0 items-center justify-center'
          }
        >
          {pickable ? (
            sources.map((s) => (
              <button
                key={s.id}
                type="button"
                className="flex w-full items-center gap-2 rounded-md px-1 py-1 text-left hover:bg-black/4 dark:hover:bg-white/6"
                style={{ fontSize: fs(T_MINOR, fontScale) }}
                onClick={() => persist(s.id)}
              >
                <TileMark
                  name={s.name}
                  color={normalizeThemeColor(s.theme_color)}
                  scale={scale}
                  icon={getIconUrl(s.icon)}
                />
                <span className="min-w-0 flex-1 truncate text-gray-700 dark:text-gray-200">
                  {s.name}
                </span>
              </button>
            ))
          ) : (
            <span
              className="text-center text-gray-400 dark:text-gray-500"
              style={{ fontSize: fs(T_MINOR, fontScale), lineHeight: 1.5 }}
            >
              {t.brew.emptyNoSources}
            </span>
          )}
        </TileShell>
      )
    }

    return (
      <div
        className="h-full w-full"
        style={locked ? { pointerEvents: 'none' } : undefined}
      >
        <BrewSourceTile
          source={source}
          size={size}
          role={role}
          now={now}
          scale={scale}
          fontScale={fontScale}
          containerRef={containerRef}
          onOpenSource={
            locked ? undefined : (s) => navigate(`/brew?source=${s.id}`)
          }
        />
      </div>
    )
  },
)

BrewSourceWidget.displayName = 'BrewSourceWidget'
