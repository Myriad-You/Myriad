/**
 * 综合报告卡片组件
 */

import type { ComprehensiveAnalysis } from './types'
import {
  FaBrain,
  FaCode,
  FaGamepad,
  FaHeart,
  FaLightbulb,
  FaMusic,
  FaPalette,
  FaRobot,
  FaRocket,
  FaTrash,
  LuPalette,
} from '@lib/icons'

import { motionShim as motion } from '@lib/motionShim'
import type { MouseEvent } from 'react'
import { memo, useCallback } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useLoopAnimation } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import {

  REPORT_CARD_FLEX_BASIS,
} from './types'

// 图标组件
const ThemeIcon = memo(
  ({
    iconImageUrl,
    iconPrompt,
    iconName,
  }: {
    iconImageUrl?: string
    iconPrompt?: string
    iconName?: string
  }) => {
    if (iconImageUrl) {
      return (
        <img
          src={iconImageUrl}
          alt="theme icon"
          className="w-24 h-24 object-contain drop-shadow-2xl"
        />
      )
    }

    if (iconPrompt) {
      return (
        <div
          className="w-20 h-20 flex items-center justify-center bg-white/20 rounded-lg text-white/60"
          title={iconPrompt}
        >
          <LuPalette size={32} />
        </div>
      )
    }

    switch (iconName) {
      case 'FaRobot':
        return <FaRobot size={64} />
      case 'FaBrain':
        return <FaBrain size={64} />
      case 'FaHeart':
        return <FaHeart size={64} />
      case 'FaMusic':
        return <FaMusic size={64} />
      case 'FaCode':
        return <FaCode size={64} />
      case 'FaGamepad':
        return <FaGamepad size={64} />
      case 'FaPalette':
        return <FaPalette size={64} />
      case 'FaRocket':
        return <FaRocket size={64} />
      default:
        return <FaLightbulb size={64} />
    }
  },
)

function getThemeIcon(
  iconImageUrl?: string,
  iconPrompt?: string,
  iconName?: string,
): React.ReactNode {
  return (
    <ThemeIcon
      iconImageUrl={iconImageUrl}
      iconPrompt={iconPrompt}
      iconName={iconName}
    />
  )
}

interface ComprehensiveReportCardProps {
  compReport: {
    id: number
    综合分析: ComprehensiveAnalysis
    created_at: string
  }
  index: number
  onOpen: (analysis: any, id: number, createdAt: string) => void
  /** When provided, shows a delete control (owner/admin only from parent). */
  onDelete?: (id: number) => void
}

export const ComprehensiveReportCard = memo<ComprehensiveReportCardProps>(
  ({ compReport, index, onOpen, onDelete }) => {
    const anim = useAnimationLevel()
    const { t } = useI18n()

    // 🆕 使用触发式动画 - 组件挂载时播放一次装饰动画
    const { isAnimating } = useLoopAnimation({
      duration: 4000, // 装饰动画约4秒周期
      trigger: 'mount', // 固定值，组件首次渲染时触发一次
      enabled: anim.loop, // 低端设备禁用
    })

    const canAnimate = anim.loop && isAnimating
    const canDelete = typeof onDelete === 'function'

    if (!compReport.综合分析) return null

    const analysis = compReport.综合分析
    const themeColor = analysis.theme_color || '#6366f1'

    const handleClick = useCallback(() => {
      onOpen(analysis, compReport.id, compReport.created_at)
    }, [analysis, compReport.id, compReport.created_at, onOpen])

    const handleDeleteClick = useCallback(
      (e: MouseEvent) => {
        e.stopPropagation()
        e.preventDefault()
        if (!onDelete) return
        if (!window.confirm(t.reportsPage.confirmDeleteReport)) return
        onDelete(compReport.id)
      },
      [onDelete, compReport.id, t.reportsPage.confirmDeleteReport],
    )

    return (
      <motion.div
        layout
        onClick={handleClick}
        whileHover={{
          y: -4,
          scale: 1.02,
          transition: { duration: 0.2, ease: 'easeOut' },
        }}
        whileTap={{
          scale: 0.98,
          transition: { duration: 0.2, ease: 'easeOut' },
        }}
        className="relative aspect-2/1 rounded-2xl overflow-hidden cursor-pointer group glass hover:shadow-xl transition-shadow shrink-0 min-w-0 snap-start"
        style={{
          // Matches home 4x2 at all breakpoints (1 / sm:2 / lg:4); see REPORT_CARD_FLEX_BASIS
          flexBasis: REPORT_CARD_FLEX_BASIS,
          willChange: 'transform, opacity',
        }}
        initial={{ opacity: 0, y: 20, scale: 0.95 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: -20, scale: 0.95 }}
        transition={{
          duration: 0.4,
          delay: index * 0.08,
          ease: [0.4, 0, 0.2, 1],
        }}
      >
        {canDelete && (
          <button
            type="button"
            onClick={handleDeleteClick}
            onPointerDown={(e) => e.stopPropagation()}
            className="absolute top-2 right-2 z-20 w-8 h-8 rounded-lg flex items-center justify-center bg-black/40 hover:bg-red-500/90 text-white/80 hover:text-white opacity-100 md:opacity-0 md:group-hover:opacity-100 focus:opacity-100 focus-visible:opacity-100 transition-all shadow-sm backdrop-blur-sm"
            aria-label={t.reportsPage.deleteReport}
            title={t.reportsPage.deleteReport}
          >
            <FaTrash size={12} />
          </button>
        )}
        <div className="absolute inset-0">
          <div
            className="absolute -right-10 -top-10 w-40 h-40 rounded-full blur-3xl opacity-20"
            style={{ background: themeColor }}
          />
          <div
            className="absolute -left-10 -bottom-10 w-32 h-32 rounded-full blur-2xl opacity-15"
            style={{ background: themeColor }}
          />

          <div className="relative z-10 h-full flex items-center justify-between px-6 py-3">
            {/* 左侧：图标区域 */}
            <div className="shrink-0 flex items-center justify-center">
              {analysis.decorative_emojis &&
                analysis.decorative_emojis.length > 0 && (
                  <>
                    {analysis.decorative_emojis
                      .slice(0, 2)
                      .map((emoji: string, i: number) => {
                        const positions = [
                          { top: '20%', left: '2%' },
                          { bottom: '20%', left: '2%' },
                        ]
                        const pos = positions[i % positions.length]

                        return (
                          <motion.span
                            key={i}
                            className="absolute text-2xl opacity-25"
                            style={{
                              ...pos,
                              willChange: canAnimate
                                ? 'transform, opacity'
                                : 'auto',
                            }}
                            animate={
                              canAnimate
                                ? {
                                    y: [0, -8, 0],
                                    opacity: [0.2, 0.3, 0.2],
                                  }
                                : { y: 0, opacity: 0.25 }
                            }
                            transition={
                              canAnimate
                                ? {
                                    duration: 4 + i * 0.3,
                                    repeat: 2, // ~10s
                                    delay: i * 0.5,
                                    ease: 'easeInOut',
                                  }
                                : { duration: 0 }
                            }
                          >
                            {emoji}
                          </motion.span>
                        )
                      })}
                  </>
                )}

              {(analysis.icon_image_url ||
                analysis.icon_prompt ||
                analysis.theme_icon) && (
                <div className="relative">
                  <motion.div
                    className="absolute inset-0 rounded-xl blur-xl"
                    style={{
                      background: themeColor,
                      opacity: 0.25,
                      willChange: canAnimate ? 'transform' : 'auto',
                    }}
                    animate={
                      canAnimate
                        ? {
                            scale: [1, 1.15, 1],
                            opacity: [0.2, 0.35, 0.2],
                          }
                        : { scale: 1, opacity: 0.25 }
                    }
                    transition={
                      canAnimate
                        ? { duration: 4, repeat: 2, ease: 'easeInOut' }
                        : { duration: 0 }
                    }
                  />
                  <motion.div
                    className="relative w-24 h-24 rounded-xl backdrop-blur-sm flex items-center justify-center shadow-2xl overflow-hidden"
                    style={{
                      background: 'var(--glass-bg)',
                      border: `2px solid ${themeColor}30`,
                      willChange: canAnimate ? 'transform' : 'auto',
                    }}
                    animate={
                      canAnimate
                        ? {
                            y: [0, -5, 0],
                          }
                        : { y: 0 }
                    }
                    transition={
                      canAnimate
                        ? { duration: 4, repeat: 2, ease: 'easeInOut' }
                        : { duration: 0 }
                    }
                  >
                    <div style={{ color: themeColor, fontSize: '56px' }}>
                      {getThemeIcon(
                        analysis.icon_image_url,
                        analysis.icon_prompt,
                        analysis.theme_icon,
                      )}
                    </div>
                  </motion.div>
                </div>
              )}
            </div>

            {/* 右侧：文本信息区域 */}
            <div className="flex-1 min-w-0 flex flex-col justify-center pl-5 pr-2">
              <motion.h2
                className="text-lg font-bold mb-1.5 truncate leading-tight"
                style={{ color: themeColor }}
                initial={{ opacity: 0, x: -10 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ delay: 0.1 }}
              >
                {analysis.visual_style || t.reportsPage.allPlatformReport}
              </motion.h2>

              {analysis.card_subtitle && (
                <motion.p
                  className="text-xs text-gray-600 dark:text-gray-400 mb-2 truncate leading-relaxed"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ delay: 0.2 }}
                >
                  {analysis.card_subtitle}
                </motion.p>
              )}

              {analysis.key_metric && (
                <motion.div
                  className="flex items-center"
                  initial={{ opacity: 0, scale: 0.95 }}
                  animate={{ opacity: 1, scale: 1 }}
                  transition={{ delay: 0.3 }}
                >
                  <div
                    className="px-3 py-1.5 rounded-full backdrop-blur-sm font-medium text-xs truncate max-w-full"
                    style={{
                      background: `${themeColor}20`,
                      color: themeColor,
                    }}
                  >
                    {analysis.key_metric}
                  </div>
                </motion.div>
              )}
            </div>

            {/* 右侧装饰emoji */}
            {analysis.decorative_emojis &&
              analysis.decorative_emojis.length > 2 && (
                <>
                  {analysis.decorative_emojis
                    .slice(2, 4)
                    .map((emoji: string, i: number) => {
                      const positions = [
                        { top: '20%', right: '3%' },
                        { bottom: '20%', right: '3%' },
                      ]
                      const pos = positions[i % positions.length]

                      return (
                        <motion.span
                          key={i + 2}
                          className="absolute text-2xl opacity-25"
                          style={{
                            ...pos,
                            willChange: canAnimate
                              ? 'transform, opacity'
                              : 'auto',
                          }}
                          animate={
                            canAnimate
                              ? {
                                  y: [0, -8, 0],
                                  opacity: [0.2, 0.3, 0.2],
                                }
                              : { y: 0, opacity: 0.25 }
                          }
                          transition={
                            canAnimate
                              ? {
                                  duration: 4 + i * 0.3,
                                  repeat: 2, // ~10s
                                  delay: (i + 2) * 0.5,
                                  ease: 'easeInOut',
                                }
                              : { duration: 0 }
                          }
                        >
                          {emoji}
                        </motion.span>
                      )
                    })}
                </>
              )}
          </div>
        </div>
      </motion.div>
    )
  },
)

ComprehensiveReportCard.displayName = 'ComprehensiveReportCard'
