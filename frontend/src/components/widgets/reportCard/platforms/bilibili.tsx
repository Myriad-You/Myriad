import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useEffect, useMemo } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { useLoopAnimation } from '../../../../hooks/animation'
import {
  CONTENT_FADE_ANIMATE,
  CONTENT_FADE_EXIT,
  CONTENT_FADE_INITIAL,
  CONTENT_FADE_TRANSITION,
  CONTENT_SLIDE_ANIMATE,
  CONTENT_SLIDE_EXIT,
  CONTENT_SLIDE_INITIAL,
  CONTENT_SLIDE_TRANSITION,
  createDanmakuTransition,
  DANMAKU_ANIMATE,
  DANMAKU_INITIAL,
  LANES_ARRAY,
} from '../animations'
import { useLibraryItemRotation } from '../hooks'
import { getBilibiliProxyUrl } from '../media'

// B站组件（完整版）
export const DanmakuWidget = memo(
  ({
    data,
    allowLoop = true,
    triggerKey,
  }: {
    data?: { danmaku?: string[] }
    allowLoop?: boolean
    triggerKey?: unknown
  }) => {
    const { t } = useI18n()
    const defaultDanmaku = t.reportCard.danmakuDefault as unknown as string[]
    const texts = useMemo(
      () => data?.danmaku || defaultDanmaku,
      [data?.danmaku, defaultDanmaku],
    )

    // 🆕 使用触发式动画 - triggerKey 变化时播放一轮，完成后自动释放
    useLoopAnimation({
      duration: 11000, // 弹幕滚动约8秒 + 额外保持3秒
      trigger: triggerKey, // 状态切换时触发
      enabled: allowLoop, // 低端设备禁用
    })

    // 🆕 低性能模式：限制弹幕数量不超过3条
    // 用 useMemo 锁定：仅在 loop 状态变化时重算随机，避免每次渲染重新洗牌弹幕
    const maxDanmakuCount = useMemo(
      () =>
        allowLoop
          ? Math.random() < 0.7
            ? Math.random() < 0.5
              ? 3
              : 4
            : 5
          : 3,
      [allowLoop],
    )

    const animations = useMemo(() => {
      // 使用预生成的 LANES_ARRAY 进行洗牌
      const availableLanes = [...LANES_ARRAY]
      for (let i = availableLanes.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1))
        ;[availableLanes[i], availableLanes[j]] = [
          availableLanes[j],
          availableLanes[i],
        ]
      }
      return texts.slice(0, maxDanmakuCount).map((_, i) => ({
        duration: 6 + Math.random() * 4,
        delay: i * 0.7 + Math.random() * 0.5,
        top: `${10 + availableLanes[i] * 18}%`,
        opacity: 0.4 + Math.random() * 0.3,
      }))
    }, [texts, maxDanmakuCount])

    return (
      <div className="relative h-full w-full overflow-hidden">
        {animations.map((anim, i) => (
          <motion.div
            key={`${texts[i]}-${i}`}
            initial={DANMAKU_INITIAL}
            animate={DANMAKU_ANIMATE}
            transition={createDanmakuTransition(anim.duration, anim.delay)}
            className="absolute whitespace-nowrap text-base font-bold danmaku-text-color gpu-accelerated"
            style={{
              top: anim.top,
              opacity: anim.opacity,
            }}
          >
            {texts[i]}
          </motion.div>
        ))}
      </div>
    )
  },
)
DanmakuWidget.displayName = 'DanmakuWidget'

export const BilibiliWidget = memo(
  ({ data, showOverview, onContentChange, allowLoop = true }: any) => {
    const { t } = useI18n()
    const libraryItems = useMemo(
      () => data?.library_items || [],
      [data?.library_items],
    )
    const { currentItem, currentItemIndex } = useLibraryItemRotation(
      libraryItems,
      showOverview,
    )

    // BE card_visuals: user_level / follower_count / following_count
    const userLevel = Number(data?.user_level) || 0
    const followerCount = Number(data?.follower_count) || 0
    const followingCount = Number(data?.following_count) || 0
    const formatCount = (num: number) => {
      if (num >= 10000)
        return `${(num / 10000).toFixed(1)}${t.reportsPage.tenThousandSuffix}`
      if (num >= 1000) return `${(num / 1000).toFixed(1)}k`
      return String(num)
    }
    const stats: { key: string; value: string; label?: string }[] = []
    if (userLevel > 0) stats.push({ key: 'level', value: `Lv.${userLevel}` })
    if (followerCount > 0) {
      stats.push({
        key: 'fans',
        value: formatCount(followerCount),
        label: t.reportsPage.fans,
      })
    }
    if (followingCount > 0) {
      stats.push({
        key: 'following',
        value: formatCount(followingCount),
        label: t.dataManagement.previewMetric.following_count,
      })
    }

    useEffect(() => {
      if (!showOverview && currentItem) {
        onContentChange?.({ title: currentItem.title, type: currentItem.type })
      } else {
        onContentChange?.(null)
      }
    }, [showOverview, currentItem, onContentChange])

    return (
      <AnimatePresence mode="wait">
        {showOverview || !currentItem ? (
          <motion.div
            key="danmaku"
            initial={CONTENT_FADE_INITIAL}
            animate={CONTENT_FADE_ANIMATE}
            exit={CONTENT_FADE_EXIT}
            transition={CONTENT_FADE_TRANSITION}
            className="h-full w-full relative"
          >
            <DanmakuWidget
              data={data}
              allowLoop={allowLoop}
              triggerKey={showOverview}
            />
            {/* 概览：等级 / 粉丝 / 关注，走 .glass 表面令牌 */}
            {stats.length > 0 && (
              <motion.div
                className="absolute bottom-3 right-3 z-20 pointer-events-none"
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.35, delay: 0.12 }}
              >
                <div className="glass flex items-baseline gap-2 rounded-full px-2.5 py-1">
                  {stats.map((stat) => (
                    <span key={stat.key} className="flex items-baseline gap-0.5">
                      <span className="text-[10px] font-black tabular-nums text-gray-900 dark:text-gray-100">
                        {stat.value}
                      </span>
                      {stat.label && (
                        <span className="text-[9px] font-medium text-gray-500 dark:text-gray-400">
                          {stat.label}
                        </span>
                      )}
                    </span>
                  ))}
                </div>
              </motion.div>
            )}
          </motion.div>
        ) : (
          <motion.div
            key={`lib-${currentItemIndex}`}
            initial={CONTENT_SLIDE_INITIAL}
            animate={CONTENT_SLIDE_ANIMATE}
            exit={CONTENT_SLIDE_EXIT}
            transition={CONTENT_SLIDE_TRANSITION}
            className="h-full w-full p-1.5"
          >
            <div className="relative h-full w-full rounded-lg overflow-hidden shadow-lg bg-white dark:bg-black/90">
              <div className="absolute inset-0">
                <img
                  src={getBilibiliProxyUrl(
                    currentItem.cover,
                    currentItem.title,
                  )}
                  alt={currentItem.title}
                  className="w-full h-full object-cover"
                  loading="lazy"
                />
                <div className="absolute inset-0 bg-linear-to-t from-black/80 via-black/40 to-transparent" />
              </div>
              {/* 追番进度：extract 从 bangumi 写入的 progress */}
              {(currentItem.progress || currentItem.title) && (
                <div className="absolute inset-x-0 bottom-0 p-2.5 z-10 flex flex-col gap-0.5">
                  <span className="text-[11px] font-bold text-white line-clamp-1 drop-shadow">
                    {currentItem.title}
                  </span>
                  {currentItem.progress && (
                    <span className="text-[10px] font-medium text-pink-200/95 line-clamp-1">
                      {currentItem.progress}
                    </span>
                  )}
                </div>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    )
  },
)
