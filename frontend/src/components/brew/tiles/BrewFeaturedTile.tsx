/**
 * 首页「精选」磁贴：不绑源，内部跑一遍 smart 取头名。
 *
 * 数据只走 `getSources()`（cache key `brew:sources`，`requestCache` 会合并
 * inflight），不新开 REST 资源：源预览里已经带了 `topic`，主题聚类直接在
 * 前端做完。
 *
 * 拿到的头名可能是一个源、也可能是一个主题：主题优先（跨源聚合的信息量更大），
 * 没有成卡的主题时退回最高分的源。两种都没有 → 引导去 /brew。
 */

import type { BrewSource } from '../../../types/brew'
import type { WidgetComponentProps } from '../../WidgetGrid'
import type { BrewTileSize } from '../logic/layout'
import type { BrewTopic } from '../logic/topics'

import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { useAuth } from '../../../contexts/AuthContext'
import { useI18n } from '../../../contexts/I18nContext'
import { useHomeVisibilityInterval } from '../../../hooks/animation'
import { useWidgetSize } from '../../../hooks/useWidgetSize'
import { getSources } from '../../../services/brewApi'
import { DEFAULT_THEME_COLOR } from '../constants'
import { downgradeForBand } from '../logic/layout'
import { roleFromAuth, sortByScore } from '../logic/score'
import { clusterTopics, previewsToTopicItems } from '../logic/topics'
import { BrewSourceTile } from './BrewSourceTile'
import { BrewTopicTile } from './BrewTopicTile'
import { TileShell } from './TileShell'
import { fs, T_MINOR } from './tokens'

/** 与 FriendLinksWidget 一致的首页轮询间隔。 */
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
  /** 一条都没有时的空态文案 */
  emptyHint?: string
}

/** 纯展示：从传入的源列表里挑头名并渲染对应磁贴。 */
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

/**
 * 首页 widget 包装：自己拉数据、自己定时刷新、点击跳 `/brew`。
 *
 * 首页永远不在原地打开阅读器 —— 首页的目的是「扫一眼」。
 */
export const BrewFeaturedWidget = memo(
  ({ config, isEditMode, isPreview }: WidgetComponentProps) => {
    const { t } = useI18n()
    const navigate = useNavigate()
    const { containerRef, scale, fontScale, viewportBand } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const [sources, setSources] = useState<BrewSource[]>([])
    const mountedRef = useRef(true)
    // 会话内冻结：布局与构图不因为「过了一分钟」而重排
    const [now] = useState(() => Date.now())

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
        // 失败保留上一次的数据；首页磁贴不该因为一次网络抖动变空
        console.error('[BrewFeaturedWidget] failed to load sources:', error)
      }
    }, [isPreview])

    useEffect(() => {
      void load()
    }, [load])

    useHomeVisibilityInterval(load, REFRESH_INTERVAL, !isPreview)

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
