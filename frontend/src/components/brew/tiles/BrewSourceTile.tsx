/**
 * 订阅源磁贴：5 种构图 × 3 档尺寸。
 *
 * 构图由 `logic/layout.tileLayout` 派生，尺寸由 `logic/layout.tileSize` 派生 ——
 * 这个组件**不判断窄屏**，只按传进来的 `config.size` 排版。
 *
 * 4×2 的硬规则：左右拆栏（feature / cadence / numeric / icon）或单行列表
 * （list）。上下堆封面再堆标题的「瘦条」是上一版被否掉的形态。
 */

import type { BrewItemPreview, BrewSource } from '../../../types/brew'
import type { WidgetComponentProps } from '../../WidgetGrid'
import type { BrewTileLayout, BrewTileSize } from '../logic/layout'
import type { BrewViewerRole } from '../logic/score'

import { LuExternalLink as ExternalLink } from '@lib/icons'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useAuth } from '../../../contexts/AuthContext'
import { useI18n } from '../../../contexts/I18nContext'
import { useHomeVisibilityInterval } from '../../../hooks/animation'
import { isExlight, useAnimationLevel } from '../../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../../hooks/useWidgetSize'
import { getSources } from '../../../services/brewApi'
import {
  DEFAULT_THEME_COLOR,
  getIconUrl,
  getPlainText,
  normalizeThemeColor,
} from '../constants'
import { downgradeForBand, tileLayout } from '../logic/layout'
import { roleFromAuth } from '../logic/score'
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
import './rotation.css'

/** 4×4 列表每页条数。 */
const LIST_PAGE_4X4 = 5
/** 4×2 列表每页条数。 */
const LIST_PAGE_4X2 = 4
/** 每页停留时长（ms），供 animationDuration 计算。 */
const ROTATE_PAGE_MS = 5600
/** 最多轮播几页（keyframes 只到 4 页）。 */
const ROTATE_MAX_PAGES = 4
/** 首页 widget 轮询间隔，与 FriendLinksWidget 一致。 */
const WIDGET_REFRESH_INTERVAL = 60 * 1000

export interface BrewSourceTileProps {
  source: BrewSource
  size: BrewTileSize
  role: BrewViewerRole
  /** 会话内冻结的时钟。同一次进入页面用同一个值，构图才不会自己跳。 */
  now: number
  scale: number
  fontScale: number
  containerRef?: React.Ref<HTMLDivElement>
  /** 整卡点击（非 icon 型） */
  onOpenSource?: (source: BrewSource) => void
  /** 点开某一篇 */
  onOpenItem?: (item: BrewItemPreview, source: BrewSource) => void
  /** 补拉后的条目（比 recent_items 更全）；不传就用 recent_items */
  items?: BrewItemPreview[]
  /** 构图覆盖，仅 DEV 预览用；生产一律走派生 */
  layoutOverride?: BrewTileLayout
}

/** 相对时间。与 SourceCard 的口径一致，但只吃 t.brew 的四个键。 */
function relTime(
  ts: number | null | undefined,
  now: number,
  t: {
    justNow: string
    minutesAgo: string
    hoursAgo: string
    daysAgo: string
  },
): string {
  if (!ts) return ''
  const diff = now - ts
  if (diff < 60_000) return t.justNow
  if (diff < 3_600_000) {
    return t.minutesAgo.replace('{minutes}', String(Math.floor(diff / 60_000)))
  }
  if (diff < 86_400_000) {
    return t.hoursAgo.replace('{hours}', String(Math.floor(diff / 3_600_000)))
  }
  if (diff < 604_800_000) {
    return t.daysAgo.replace('{days}', String(Math.floor(diff / 86_400_000)))
  }
  return new Date(ts).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
  })
}

function siteHost(source: BrewSource): string {
  try {
    return new URL(source.site_url || source.url).hostname.replace(/^www\./, '')
  } catch {
    return ''
  }
}

/**
 * 轮播分页：把条目切成每页 pageSize 条，最多 ROTATE_MAX_PAGES 页。
 *
 * **只保留满页。** 末页只有一两条时，同样高度里行数不同，轮播过去会看到
 * 行距忽然变大、内容忽上忽下 —— 也就是「切文章时高度乱跳」。宁可少转一页，
 * 也不要让每一轮的版面都不一样。
 */
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
  }: BrewSourceTileProps) => {
    const { t } = useI18n()
    const anim = useAnimationLevel()
    const color = normalizeThemeColor(source.theme_color)
    const icon = getIconUrl(source.icon)

    const list = useMemo(
      () => items ?? source.recent_items ?? [],
      [items, source.recent_items],
    )
    const layout: BrewTileLayout =
      layoutOverride ?? tileLayout(source, role, now, list.length)

    // 未读只在登录后是真数据；游客侧后端恒回 0，画出来就是假信息
    const unread = role === 'guest' ? null : source.unread_count
    // 失败态只在管理员视图报警：对游客/成员它是噪音，不是待办
    const alert = role === 'admin' && source.error_count > 0

    const timeKeys = {
      justNow: t.brew.justNow,
      minutesAgo: t.brew.minutesAgo,
      hoursAgo: t.brew.hoursAgo,
      daysAgo: t.brew.daysAgo,
    }

    const header = (
      <TileHeader
        name={source.name}
        color={color}
        scale={scale}
        fontScale={fontScale}
        icon={icon}
        unread={unread}
        alert={alert}
      />
    )

    const openSource = onOpenSource ? () => onOpenSource(source) : undefined
    const openItem = (item: BrewItemPreview) => onOpenItem?.(item, source)

    // 2×2 既不放封面（面积不够，§10.2）也不左右拆栏（拆完文字只剩 ~90px，
    // 比上下瘦条更糟）。拆栏那条规则针对的是 4×2 那个扁矩形。
    const isSmall = size === '2x2'

    // icon 型：友链入口。整卡点击直接开外站，不进阅读器
    if (layout === 'icon') {
      const host = siteHost(source)
      const target = source.site_url || source.url
      const desc = source.description?.trim() || host
      const tag = source.ai_style_tags?.find((x) => x.trim())?.trim()

      return (
        <TileShell
          color={color}
          scale={scale}
          containerRef={containerRef}
          label={source.name}
          glow="single"
          contentClassName="flex min-h-0 items-center"
          onClick={
            /^https?:\/\//i.test(target)
              ? () => window.open(target, '_blank', 'noopener,noreferrer')
              : undefined
          }
        >
          <div className="flex min-w-0 flex-1 items-center" style={{ gap: sp(11, scale) }}>
            <TileMark
              name={source.name}
              color={color}
              scale={scale}
              size={size === '2x2' ? ICON_MARK_SIZE * 0.7 : ICON_MARK_SIZE}
              icon={icon}
            />
            <div className="flex min-w-0 flex-1 flex-col" style={{ gap: sp(3, scale) }}>
              <div className="flex min-w-0 items-center" style={{ gap: sp(4, scale) }}>
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

    // numeric 型：仅登录。数字与「条未读」横排
    if (layout === 'numeric') {
      const heroSize = heroNumberSize(size)
      const rows = size === '4x4' ? list.slice(0, 5) : list.slice(0, 4)
      const hero = (
        <div className="flex min-w-0 items-baseline" style={{ gap: sp(4, scale) }}>
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

      // 2×2：只放数字 + 站名，列表塞不进去
      if (isSmall) {
        return (
          <TileShell
            color={color}
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

      // 4×2 拆栏：左数字 + 右 3–4 条标题
      if (size !== '4x4') {
        return (
          <TileShell
            color={color}
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
          scale={scale}
          containerRef={containerRef}
          label={source.name}
          onClick={openSource}
          glow="dual"
        >
          {header}
          <div style={{ marginBottom: sp(10, scale) }}>{hero}</div>
          {/* justify-evenly：4×4 有 ~320px 高，5 条紧贴顶部会在下半张卡留一个洞 */}
          <div className="flex min-h-0 flex-1 flex-col justify-evenly">
            {rows.map((item, i) => (
              <MinorRow
                key={item.id}
                title={item.title}
                time={relTime(item.published_at, now, timeKeys)}
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

    // cadence 型：沉寂源。节律图 + 轴标签
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
        <TileMeta fontScale={fontScale} scale={scale} className="justify-between">
          <span>{t.brew.tileQuietMonths.replace('{months}', String(months))}</span>
          <span className={alert ? 'text-red-500 dark:text-red-400' : ''}>
            {alert
              ? t.brew.tileFailedTimes.replace('{count}', String(source.error_count))
              : t.brew.tileToday}
          </span>
        </TileMeta>
      )

      // 2×2：站名 + 通栏节律图 + 轴标签，不拆栏
      if (isSmall) {
        return (
          <TileShell
            color={color}
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

      // 4×2 拆栏：左节律图 + 右站名 / 最新一篇 / 跨度
      if (size !== '4x4') {
        return (
          <TileShell
            color={color}
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
              <Cadence
                pulses={pulses}
                color={color}
                height={sp(34, scale)}
              />
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
          scale={scale}
          containerRef={containerRef}
          label={source.name}
          onClick={openSource}
        >
          {header}
          <div className="flex min-h-0 flex-1 flex-col justify-center">
            {/* 4×4 的节律图是主视觉，56px 撑不起 320px 的卡，也看不出密度差 */}
            <Cadence pulses={pulses} color={color} height={sp(104, scale)} />
            <div style={{ marginTop: sp(6, scale) }}>{axis}</div>
          </div>
          {latest ? (
            <div style={{ marginTop: sp(8, scale) }}>
              <MinorRow
                title={latest.title}
                time={relTime(latest.published_at, now, timeKeys)}
                scale={scale}
                fontScale={fontScale}
                onClick={onOpenItem ? () => openItem(latest) : undefined}
              />
            </div>
          ) : null}
        </TileShell>
      )
    }

    // list 型：站名行 + 头条 + 次条，整页轮播
    if (layout === 'list') {
      const pageSize = size === '4x4' ? LIST_PAGE_4X4 : LIST_PAGE_4X2
      const pages = paginate(list, pageSize)
      const rotating = pages.length > 1 && !isExlight(anim) && anim.widgetUiRotation
      // 每张卡不同的负 delay 错峰；用 id 派生，保证同一张卡每次一样
      const stagger = -((source.id * 1300) % (ROTATE_PAGE_MS * pages.length))

      // 4×2 不拆栏、不放封面：吃水平宽度，站名一行 + 4 条单行标题
      if (size !== '4x4') {
        return (
          <TileShell
            color={color}
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
                    // 不轮播时也要显式给 100% 高：轨道 auto 高会让里面的
                    // `height: 100%` 退化成内容高，几条标题全挤在卡片上半部
                    : { height: '100%' }
                }
              >
                {(rotating ? pages : pages.slice(0, 1)).map((page, pi) => (
                  <div
                    key={pi}
                    className="flex flex-col justify-evenly"
                    style={{ height: rotating ? `${100 / pages.length}%` : '100%' }}
                  >
                    {page.map((item) => (
                      <MinorRow
                        key={item.id}
                        title={item.title}
                        time={relTime(item.published_at, now, timeKeys)}
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
                  className="flex flex-col"
                  style={{
                    height: rotating ? `${100 / pages.length}%` : '100%',
                    gap: sp(6, scale),
                  }}
                >
                  {page.map((item, i) =>
                    i === 0 ? (
                      <div
                        key={item.id}
                        className="flex min-w-0 cursor-pointer items-start"
                        style={{ gap: sp(9, scale) }}
                        onClick={(e) => {
                          if (!onOpenItem) return
                          e.stopPropagation()
                          openItem(item)
                        }}
                      >
                        {/* 无图不画灰块，但行高要保住：否则有图页 52px、
                            无图页塌成一行，轮播过去像在抽搐 */}
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
                        <div className="flex min-w-0 flex-1 flex-col" style={{ gap: sp(3, scale) }}>
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
                            <span>{relTime(item.published_at, now, timeKeys)}</span>
                          </TileMeta>
                        </div>
                      </div>
                    ) : (
                      <MinorRow
                        key={item.id}
                        title={item.title}
                        time={relTime(item.published_at, now, timeKeys)}
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

    // feature 型：头条独大
    const lead = list[0]
    const second = list[1]
    const summary = getPlainText(lead?.summary ?? null)
    const readingHint = lead?.published_at
      ? relTime(lead.published_at, now, timeKeys)
      : ''

    // 4×2 拆栏：左封面 + 右站名 / 标题两行 / meta。
    // 2×2 走同一段结构但不放封面（isSmall），于是自然退化成纯文本。
    if (size !== '4x4') {
      return (
        <TileShell
          color={color}
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
        <div className="flex min-h-0 flex-1 flex-col" style={{ gap: sp(5, scale) }}>
          {lead ? (
            <span
              className="cursor-pointer font-medium text-gray-800 dark:text-gray-100"
              style={{
                fontSize: fs(T_TITLE, fontScale),
                lineHeight: 1.35,
                display: '-webkit-box',
                WebkitBoxOrient: 'vertical',
                WebkitLineClamp: lead.image ? 2 : 3,
                overflow: 'hidden',
              }}
              onClick={(e) => {
                if (!onOpenItem) return
                e.stopPropagation()
                openItem(lead)
              }}
            >
              {lead.title}
            </span>
          ) : (
            // 无条目：走纯文本，用站点简介，不画灰占位
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
          {/* 有封面时摘要也照放（clamp 2）：4×4 的封面 96px + 标题两行只占掉
              三分之一，不给摘要就在卡中间留一块空白 —— 空白不是留白，是没排完。 */}
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
                time={relTime(second.published_at, now, timeKeys)}
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

/** 节律窗口再导出，方便 DEV 预览生成 fixture。 */
export { CADENCE_WINDOW_DAYS }

/**
 * 首页 widget 包装：按 `config.config.sourceId` 绑一个源。
 *
 * 数据走 `getSources()`（cache key `brew:sources`）+ `find(sourceId)`，
 * 不为磁贴新开 `GET /source/:id` —— `requestCache` 会把多张卡的请求合并成一次。
 *
 * 编辑模式下未绑源时原地列出可选源，选中即 `onConfigChange` 落盘。
 */
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
    const [sources, setSources] = useState<BrewSource[]>([])
    const mountedRef = useRef(true)
    // 会话内冻结时钟：构图不因为「过了一分钟」而跳
    const [now] = useState(() => Date.now())

    const sourceId = config.config?.sourceId as number | undefined

    useEffect(() => {
      mountedRef.current = true
      return () => {
        mountedRef.current = false
      }
    }, [])

    const load = useCallback(async () => {
      if (isPreview) return
      try {
        const next = await getSources()
        if (mountedRef.current) setSources(next)
      } catch (error) {
        console.error('[BrewSourceWidget] failed to load sources:', error)
      }
    }, [isPreview])

    useEffect(() => {
      void load()
    }, [load])

    useHomeVisibilityInterval(load, WIDGET_REFRESH_INTERVAL, !isPreview)

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
    const source = sourceId ? sources.find((s) => s.id === sourceId) : undefined
    const locked = isEditMode || isPreview

    // 未绑源：编辑模式给一个原地选择器，其它情况给一句提示
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
