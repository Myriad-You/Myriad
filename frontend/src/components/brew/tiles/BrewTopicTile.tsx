/** 主题名走 i18n。主题卡永不进 2×2。 */

import type { WidgetComponentProps } from '../../widgetGridTypes'
import type { BrewTileSize } from '../logic/layout'
import type { BrewTopic, TopicItem } from '../logic/topics'

import { memo, useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useI18n } from '../../../contexts/I18nContext'
import { useWidgetSize } from '../../../hooks/useWidgetSize'
import { DEFAULT_THEME_COLOR } from '../constants'
import { downgradeForBand } from '../logic/layout'
import {
  clusterTopics,
  previewsToTopicItems,
  topicDisplayName,
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
import { useWidgetSources } from './useWidgetSources'

const TOPIC_WIDGET_REFRESH_INTERVAL = 60 * 1000

export interface BrewTopicTileProps {
  topic: BrewTopic
  size: BrewTileSize
  scale: number
  fontScale: number
  containerRef?: React.Ref<HTMLDivElement>
  onOpenTopic?: (topic: BrewTopic) => void
  onOpenItem?: (item: TopicItem) => void
  surface?: 'glass' | 'solid'
}

/** 未知 nameKey 回落到 topic.key，不写死中文。 */
function useTopicName(topic: { key: string; nameKey: BrewTopic['nameKey'] }): string {
  const { t } = useI18n()
  return topicDisplayName(topic, t.brew)
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
    const { t, format } = useI18n()
    const name = useTopicName(topic)
    const color = topic.hue
    const sourceCount = topicSourceCount(topic)
    const covers = topic.items.map((i) => i.image)

    const open = onOpenTopic ? () => onOpenTopic(topic) : undefined

    const counts = (
      <TileMeta fontScale={fontScale} scale={scale}>
        <span>{format(t.brew.articlesCount, { count: topic.items.length })}</span>
        <span aria-hidden>·</span>
        <span>{format(t.brew.topicSourceCount, { count: sourceCount })}</span>
      </TileMeta>
    )

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

        {/* 不足 4 张少格，不补灰块；没有则 null。 */}
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
              {format(t.brew.topicSourceCount, { count: sourceCount })}
            </span>
          </TileMeta>
        </div>
      </TileShell>
    )
  },
)

BrewTopicTile.displayName = 'BrewTopicTile'

/** 不为主题卡新开接口。 */
export const BrewTopicWidget = memo(
  ({ config, isEditMode, isPreview, onConfigChange }: WidgetComponentProps) => {
    const { t } = useI18n()
    const navigate = useNavigate()
    const { containerRef, scale, fontScale, viewportBand } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const sources = useWidgetSources(
      isPreview ?? false,
      TOPIC_WIDGET_REFRESH_INTERVAL,
      '[BrewTopicWidget]',
    )
    const [now] = useState(() => Date.now())

    const topicKey = config.config?.topicKey as string | undefined

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
                  {topicDisplayName(x, t.brew)}
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
