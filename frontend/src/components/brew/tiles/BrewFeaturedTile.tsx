/** 不新开 REST。主题优先，没有则退回最高分源。 */

import type { BrewSource } from '../../../types/brew'
import type { WidgetComponentProps } from '../../widgetGridTypes'
import type { BrewTileSize } from '../logic/layout'
import type { BrewTopic } from '../logic/topics'

import { memo, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useAuth } from '../../../contexts/AuthContext'
import { useI18n } from '../../../contexts/I18nContext'
import { useWidgetSize } from '../../../hooks/useWidgetSize'
import { DEFAULT_THEME_COLOR } from '../constants'
import { downgradeForBand } from '../logic/layout'
import { roleFromAuth, sortByScore } from '../logic/score'
import { clusterTopics, previewsToTopicItems } from '../logic/topics'
import { BrewSourceTile } from './BrewSourceTile'
import { BrewTopicTile } from './BrewTopicTile'
import { TileShell } from './TileShell'
import { fs, T_MINOR } from './tokens'
import { useWidgetSources } from './useWidgetSources'

const REFRESH_INTERVAL = 60 * 1000

export interface BrewFeaturedTileProps {
  size: BrewTileSize
  scale: number
  fontScale: number
  containerRef?: React.Ref<HTMLDivElement>
  sources: BrewSource[]
  now: number
  onOpenSource?: (source: BrewSource) => void
  onOpenTopic?: (topic: BrewTopic) => void
  emptyHint?: string
}

export const BrewFeaturedTile = memo(
  ({
    size,
    scale,
    fontScale,
    containerRef,
    sources,
    now,
    onOpenSource,
    onOpenTopic,
    emptyHint,
  }: BrewFeaturedTileProps) => {
    const { isAuthenticated, isAdmin } = useAuth()
    const role = roleFromAuth(isAuthenticated, isAdmin)

    const topTopic = useMemo(() => {
      const topics = clusterTopics(previewsToTopicItems(sources), now)
      return topics[0] ?? null
    }, [sources, now])

    const topSource = useMemo(() => {
      const ranked = sortByScore(sources, role, now)
      return ranked[0] ?? null
    }, [sources, role, now])

    if (topTopic) {
      return (
        <BrewTopicTile
          topic={topTopic}
          size={size}
          scale={scale}
          fontScale={fontScale}
          containerRef={containerRef}
          onOpenTopic={onOpenTopic}
        />
      )
    }

    if (topSource) {
      return (
        <BrewSourceTile
          source={topSource}
          size={size}
          role={role}
          now={now}
          scale={scale}
          fontScale={fontScale}
          containerRef={containerRef}
          onOpenSource={onOpenSource}
        />
      )
    }

    return (
      <TileShell
        color={DEFAULT_THEME_COLOR}
        scale={scale}
        containerRef={containerRef}
        glow="none"
        contentClassName="flex min-h-0 items-center justify-center"
      >
        <span
          className="text-center text-gray-400 dark:text-gray-500"
          style={{ fontSize: fs(T_MINOR, fontScale), lineHeight: 1.5 }}
        >
          {emptyHint}
        </span>
      </TileShell>
    )
  },
)

BrewFeaturedTile.displayName = 'BrewFeaturedTile'

/** 首页不在原地打开阅读器。 */
export const BrewFeaturedWidget = memo(
  ({ config, isEditMode, isPreview }: WidgetComponentProps) => {
    const { t } = useI18n()
    const navigate = useNavigate()
    const { containerRef, scale, fontScale, viewportBand } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const sources = useWidgetSources(
      isPreview ?? false,
      REFRESH_INTERVAL,
      '[BrewFeaturedWidget]',
    )
    // 会话内冻结时钟。
    const [now] = useState(() => Date.now())

    const size = downgradeForBand(
      (config.size === '4x4' ? '4x4' : '4x2') as BrewTileSize,
      viewportBand,
    )
    const locked = isEditMode || isPreview

    return (
      <div
        className="h-full w-full"
        style={locked ? { pointerEvents: 'none' } : undefined}
      >
        <BrewFeaturedTile
          size={size}
          scale={scale}
          fontScale={fontScale}
          containerRef={containerRef}
          sources={sources}
          now={now}
          emptyHint={t.brew.emptyNoSources}
          onOpenSource={
            locked ? undefined : (s) => navigate(`/brew?source=${s.id}`)
          }
          onOpenTopic={
            locked
              ? undefined
              : (topic) => navigate(`/brew?topic=${encodeURIComponent(topic.key)}`)
          }
        />
      </div>
    )
  },
)

BrewFeaturedWidget.displayName = 'BrewFeaturedWidget'
