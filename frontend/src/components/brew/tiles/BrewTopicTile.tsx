/**
 * 主题磁贴：跨源聚合。
 *
 * 这是「智能」的真正产出 —— 不同源的同题文章第一次出现在同一张卡里。
 * 主题名走 i18n（`nameKey`），身份色用主题自己的 hue。
 *
 * 4×2 是左 2×2 拼贴 + 右主题名 / 篇数 / 1 条。主题卡永不进 2×2。
 */

import type { BrewSource } from '../../../types/brew'
import type { WidgetComponentProps } from '../../widgetGridTypes'
import type { BrewTileSize } from '../logic/layout'
import type { BrewTopic, TopicItem } from '../logic/topics'

import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useI18n } from '../../../contexts/I18nContext'
import { useHomeVisibilityInterval } from '../../../hooks/animation'
import { useWidgetSize } from '../../../hooks/useWidgetSize'
import { getSources } from '../../../services/brewApi'
import { DEFAULT_THEME_COLOR } from '../constants'
import { downgradeForBand } from '../logic/layout'
import {
  clusterTopics,
  previewsToTopicItems,
  topicSourceCount,
} from '../logic/topics'
import { MinorRow } from './MinorRow'
import { TileCoverMosaic } from './TileCover'
import { TileMark, TileMeta, TileShell } from './TileShell'
import {
  COVER_H_TOPIC,
  COVER_H_TOPIC_WIDE,
  fs,
  sp,
  SPLIT_MEDIA_WIDTH,
  T_META,
  T_MINOR,
  T_NUM,
  T_TITLE,
} from './tokens'

/** 首页 widget 轮询间隔，与 FriendLinksWidget 一致。 */
const TOPIC_WIDGET_REFRESH_INTERVAL = 60 * 1000

export interface BrewTopicTileProps {
  topic: BrewTopic
  size: BrewTileSize
  scale: number
  fontScale: number
  containerRef?: React.Ref<HTMLDivElement>
  /** 进入 topic-feed（跨源列表） */
  onOpenTopic?: (topic: BrewTopic) => void
  onOpenItem?: (item: TopicItem) => void
  /** 表面；磁贴墙传 solid，见 TileShell.css */
  surface?: 'glass' | 'solid'
}

/** i18n key → 文案。未知 key 回落到 key 本身，不写死中文。 */
function useTopicName(nameKey: string): string {
  const { t } = useI18n()
  const table = t.brew as unknown as Record<string, string | undefined>
  return table[nameKey] ?? nameKey
}

export const BrewTopicTile = memo(
  ({
    topic,
    size,
    scale,
    fontScale,
    containerRef,
    onOpenTopic,
    onOpenItem,
    surface,
  }: BrewTopicTileProps) => {
    const { t } = useI18n()
    const name = useTopicName(topic.nameKey)
    const color = topic.hue
    const sourceCount = topicSourceCount(topic)
    const covers = topic.items.map((i) => i.image)

    const open = onOpenTopic ? () => onOpenTopic(topic) : undefined

    const counts = (
      <TileMeta fontScale={fontScale} scale={scale}>
        <span>{t.brew.articlesCount.replace('{count}', String(topic.items.length))}</span>
        <span aria-hidden>·</span>
        <span>{t.brew.topicSourceCount.replace('{count}', String(sourceCount))}</span>
      </TileMeta>
    )

    // 4×2：左拼贴 + 右主题名 / 篇数 / 1 条
    if (size !== '4x4') {
      return (
        <TileShell
          color={color}
          surface={surface}
          scale={scale}
          containerRef={containerRef}
          label={name}
          onClick={open}
          contentClassName="flex min-h-0 flex-row items-stretch"
        >
          <div
            className="flex shrink-0 flex-col justify-center"
            style={{ width: SPLIT_MEDIA_WIDTH, paddingRight: sp(10, scale) }}
          >
            <TileCoverMosaic
              images={covers}
              height={sp(COVER_H_TOPIC_WIDE, scale)}
              gap={sp(2, scale)}
            />
          </div>
          <div
            className="flex min-w-0 flex-1 flex-col justify-center"
            style={{ gap: sp(4, scale) }}
          >
            <div className="flex min-w-0 items-center" style={{ gap: sp(6, scale) }}>
              <TileMark name={name} color={color} scale={scale} />
              <span
                className="min-w-0 flex-1 truncate font-semibold text-gray-800 dark:text-gray-100"
                style={{ fontSize: fs(T_TITLE, fontScale), lineHeight: 1.25 }}
              >
                {name}
              </span>
            </div>
            {counts}
            {topic.items[0] ? (
              <MinorRow
                title={topic.items[0].title}
                scale={scale}
                fontScale={fontScale}
                onClick={onOpenItem ? () => onOpenItem(topic.items[0]) : undefined}
              />
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
        label={name}
        onClick={open}
        glow="dual"
      >
        <div
          className="flex min-w-0 items-center"
          style={{ gap: sp(7, scale), marginBottom: sp(6, scale) }}
        >
          <TileMark name={name} color={color} scale={scale} />
          <div className="flex min-w-0 flex-1 flex-col">
            <span
              className="text-gray-400 dark:text-gray-500"
              style={{ fontSize: fs(T_META, fontScale), lineHeight: 1.2 }}
            >
              {t.brew.topicAggregate}
            </span>
            <span
              className="min-w-0 truncate font-semibold text-gray-800 dark:text-gray-100"
              style={{ fontSize: fs(T_TITLE, fontScale), lineHeight: 1.25 }}
            >
              {name}
            </span>
          </div>
          <span
            className="shrink-0 font-semibold"
            style={{ fontSize: fs(T_NUM, fontScale), lineHeight: 1, color }}
          >
            {topic.items.length}
          </span>
        </div>

        {/* 拼贴：不足 4 张就少格，不补灰块；一张都没有返回 null */}
        <TileCoverMosaic
          images={covers}
          height={sp(COVER_H_TOPIC, scale)}
          gap={sp(2, scale)}
        />

        <div
          className="flex min-h-0 flex-1 flex-col justify-evenly"
          style={{ marginTop: sp(6, scale) }}
        >
          {topic.items.slice(0, 4).map((item, i) => (
            <MinorRow
              key={item.id}
              title={item.title}
              time={item.source_name ?? undefined}
              scale={scale}
              fontScale={fontScale}
              dim={i >= 2 ? 1 : 0}
              onClick={onOpenItem ? () => onOpenItem(item) : undefined}
            />
          ))}
        </div>

        <div style={{ marginTop: sp(6, scale) }}>
          <TileMeta fontScale={fontScale} scale={scale}>
            <span style={{ fontSize: fs(T_MINOR, fontScale) }}>
              {t.brew.topicSourceCount.replace('{count}', String(sourceCount))}
            </span>
          </TileMeta>
        </div>
      </TileShell>
    )
  },
)

BrewTopicTile.displayName = 'BrewTopicTile'

/**
 * 首页 widget 包装：按 `config.config.topicKey` 绑一个主题。
 *
 * 数据仍只走 `getSources()` —— 源预览里带 `topic`，聚类在前端做完，
 * 不为主题卡新开接口。选中的主题当期不成卡（不足 3 篇）时给一句提示，
 * 不画空壳。
 */
export const BrewTopicWidget = memo(
  ({ config, isEditMode, isPreview, onConfigChange }: WidgetComponentProps) => {
    const { t } = useI18n()
    const navigate = useNavigate()
    const { containerRef, scale, fontScale, viewportBand } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const [sources, setSources] = useState<BrewSource[]>([])
    const mountedRef = useRef(true)
    const [now] = useState(() => Date.now())

    const topicKey = config.config?.topicKey as string | undefined

    useEffect(() => {
      mountedRef.current = true
      return () => {
        mountedRef.current = false
      }
    }, [])

    const load = useCallback(async () => {
      // 预览态（小组件库）也拉一次：`getSources()` 走 requestCache，一屏
      // 多个磁贴只会合并成一个请求。库里全是「暂无订阅源」的空盒子时，
      // 用户根本看不出这三个磁贴是什么。轮询仍然只在非预览态开。
      try {
        const next = await getSources()
        if (mountedRef.current) setSources(next)
      } catch (error) {
        console.error('[BrewTopicWidget] failed to load sources:', error)
      }
    }, [isPreview])

    useEffect(() => {
      void load()
    }, [load])

    useHomeVisibilityInterval(load, TOPIC_WIDGET_REFRESH_INTERVAL, !isPreview)

    const topics = useMemo(
      () => clusterTopics(previewsToTopicItems(sources), now),
      [sources, now],
    )
    const topic = topicKey
      ? topics.find((x) => x.key === topicKey)
      : topics[0]

    const persist = useCallback(
      (nextKey: string) => {
        const payload = { ...config.config, topicKey: nextKey }
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
    const locked = isEditMode || isPreview

    if (!topic) {
      const pickable = isEditMode && !isPreview && topics.length > 0
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
            topics.map((x) => (
              <button
                key={x.key}
                type="button"
                className="flex w-full items-center gap-2 rounded-md px-1 py-1 text-left hover:bg-black/4 dark:hover:bg-white/6"
                style={{ fontSize: fs(T_MINOR, fontScale) }}
                onClick={() => persist(x.key)}
              >
                <span
                  className="h-2 w-2 shrink-0 rounded-full"
                  style={{ background: x.hue }}
                  aria-hidden
                />
                <span className="min-w-0 flex-1 truncate text-gray-700 dark:text-gray-200">
                  {(t.brew as unknown as Record<string, string | undefined>)[
                    x.nameKey
                  ] ?? x.key}
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
        <BrewTopicTile
          topic={topic}
          size={size}
          scale={scale}
          fontScale={fontScale}
          containerRef={containerRef}
          onOpenTopic={
            locked
              ? undefined
              : (x) => navigate(`/brew?topic=${encodeURIComponent(x.key)}`)
          }
        />
      </div>
    )
  },
)

BrewTopicWidget.displayName = 'BrewTopicWidget'
