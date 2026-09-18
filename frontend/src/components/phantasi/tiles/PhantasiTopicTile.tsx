/** 主题名：旧 key 走 i18n，自建名原样显示。主题卡永不进 2×2。 */

import type { PhantasiTileSize } from '../logic/layout'
import type { PhantasiTopic, TopicItem } from '../logic/topics'

import { memo } from 'react'

import { useI18n } from '../../../contexts/I18nContext'
import { topicDisplayName, topicSourceCount } from '../logic/topics'
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

interface PhantasiTopicTileProps {
  topic: PhantasiTopic
  size: PhantasiTileSize
  scale: number
  fontScale: number
  containerRef?: React.Ref<HTMLDivElement>
  onOpenTopic?: (topic: PhantasiTopic) => void
  onOpenItem?: (item: TopicItem) => void
  surface?: 'glass' | 'solid'
}

/** 未知 nameKey 回落到 topic.key，不写死中文。 */
function useTopicName(topic: { key: string; nameKey: PhantasiTopic['nameKey'] }): string {
  const { t } = useI18n()
  return topicDisplayName(topic, t.phantasi)
}

export const PhantasiTopicTile = memo(
  ({
    topic,
    size,
    scale,
    fontScale,
    containerRef,
    onOpenTopic,
    onOpenItem,
    surface,
  }: PhantasiTopicTileProps) => {
    const { t, format } = useI18n()
    const name = useTopicName(topic)
    const color = topic.hue
    const sourceCount = topicSourceCount(topic)
    const covers = topic.items.map((i) => i.image)

    const open = onOpenTopic ? () => onOpenTopic(topic) : undefined

    const counts = (
      <TileMeta fontScale={fontScale} scale={scale}>
        <span>{format(t.phantasi.articlesCount, { count: topic.items.length })}</span>
        <span aria-hidden>·</span>
        <span>{format(t.phantasi.topicSourceCount, { count: sourceCount })}</span>
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
              {t.phantasi.topicAggregate}
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
              {format(t.phantasi.topicSourceCount, { count: sourceCount })}
            </span>
          </TileMeta>
        </div>
      </TileShell>
    )
  },
)

PhantasiTopicTile.displayName = 'PhantasiTopicTile'
