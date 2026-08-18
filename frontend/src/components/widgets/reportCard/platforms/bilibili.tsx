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
    const userLevel = useMemo(
      () => Number(data?.user_level) || 0,
      [data?.user_level],
    )
    const followerCount = useMemo(
      () => Number(data?.follower_count) || 0,
      [data?.follower_count],
    )
    const followingCount = useMemo(
      () => Number(data?.following_count) || 0,
      [data?.following_count],
    )
    const hasStats =
      userLevel > 0 || followerCount > 0 || followingCount > 0

    const formatNumber = (num: number) => {
      if (num >= 10000)
        return `${(num / 10000).toFixed(1)}${t.reportsPage.tenThousandSuffix}`
      if (num >= 1000) return `${(num / 1000).toFixed(1)}k`
      return num.toString()
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
            {/* 概览：展示 BE 写入的等级/粉丝/关注（弹幕之上） */}
            {hasStats && (
              <div className="absolute bottom-3 right-3 z-20 flex flex-col items-end gap-1.5 pointer-events-none">
                {userLevel > 0 && (
                  <motion.div
                    className="px-2.5 py-0.5 rounded-full text-[9px] font-bold flex items-center gap-1 shadow-lg bg-linear-to-br from-pink-50 to-pink-100 dark:from-pink-950/80 dark:to-pink-900/60 text-pink-600 dark:text-pink-300"
                    style={{ boxShadow: '0 2px 12px rgba(236, 72, 153, 0.25)' }}
                    initial={{ scale: 0.8, opacity: 0, x: 20 }}
                    animate={{ scale: 1, opacity: 1, x: 0 }}
                    transition={{ duration: 0.4, delay: 0.1 }}
                  >
                    <span className="text-[7px]">●</span>
                    <span>Lv.{userLevel}</span>
                  </motion.div>
                )}
                {(followerCount > 0 || followingCount > 0) && (
                  <motion.div
                    className="flex items-center gap-2 px-3 py-1.5 rounded-lg glass-surface glass-90 shadow-lg border border-white/30 dark:border-white/10"
                    initial={{ scale: 0.8, opacity: 0, x: 20 }}
                    animate={{ scale: 1, opacity: 1, x: 0 }}
                    transition={{ duration: 0.4, delay: 0.2 }}
                  >
                    {followerCount > 0 && (
                      <div className="flex flex-col items-end">
                        <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">
                          {formatNumber(followerCount)}
                        </span>
                        <span className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400">
                          {t.reportsPage.fans}
                        </span>
                      </div>
                    )}
                    {followerCount > 0 && followingCount > 0 && (
                      <div className="w-px h-5 bg-gray-300 dark:bg-white/20" />
                    )}
                    {followingCount > 0 && (
                      <div className="flex flex-col items-end">
                        <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">
                          {formatNumber(followingCount)}
                        </span>
                        <span className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400">
                          {t.dataManagement.previewMetric.following_count}
                        </span>
                      </div>
                    )}
                  </motion.div>
                )}
              </div>
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
