/**
 * 欢迎小组件 - 4x2卡片
 * Glass风格设计，左右布局，动态引导内容
 */

import type { WidgetComponentProps } from '../WidgetGrid'
import { SiAppstore } from '@lib/icons'
import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useId, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { useLoopAnimation } from '../../hooks/animation'
import { useHomeVisibilityInterval } from '../../hooks/animation/pages/home'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { GlowBackground } from './shared/GlowBackground'

// Navigation guide type definition
interface NavigationGuide {
  title: string
  description: string
  features: string[]
  path: string
  color: string
}

export const WelcomeWidget = memo(({ config, isEditMode, isPreview }: WidgetComponentProps) => {
  // 如果是预览模式，强制 scale 为 1，因为外部容器已经进行了缩放
  const { containerRef, scale, fontScale } = useWidgetSize(config.size, isPreview ? 1 : undefined)
  const anim = useAnimationLevel()
  const uniqueId = useId()
  const { t, locale } = useI18n()

  // 非中文语言时减小标题字体（英文等语言单词较长）
  const isNonChinese = !locale.startsWith('zh')
  const titleFontScale = isNonChinese ? fontScale * 0.88 : fontScale // 标题减少约12%
  const infoFontScale = isNonChinese ? fontScale * 0.92 : fontScale // 信息类减少约8%

  // 🆕 使用触发式动画 - 组件挂载时播放一次箭头动画
  const { isAnimating } = useLoopAnimation({
    duration: 1500, // 箭头动画约1.5秒周期
    trigger: 'mount', // 固定值，组件首次渲染时触发一次
    enabled: anim.loop, // 低端设备禁用
  })

  const canAnimate = anim.loop && isAnimating

  const navigate = useNavigate()
  const [currentGuideIndex, setCurrentGuideIndex] = useState(0)
  const [greeting, setGreeting] = useState('')

  // Navigation guides with i18n
  const navigationGuides = useMemo(() => [
    {
      title: t.widgets.library,
      description: t.widgets.multiPlatformAggregation,
      features: [
        t.widgets.showPersonality,
      ],
      path: '/library',
      color: '#8b5cf6',
    },
    {
      title: t.widgets.dataReport,
      description: t.widgets.dualLayerAnalysis,
      features: [
        t.widgets.platformProfile,
      ],
      path: '/reports',
      color: '#06b6d4',
    },
    {
      title: t.widgets.brewReading,
      description: t.widgets.brewDesc,
      features: [
        t.widgets.brewFeature,
      ],
      path: '/brew',
      color: '#f97316',
    },
    {
      title: t.widgets.tappApps,
      description: t.widgets.tappDesc,
      features: [
        t.widgets.tappFeature,
      ],
      path: '/tapp',
      color: '#10b981',
    },
  ], [t])

  // 动态问候语
  useEffect(() => {
    if (isPreview) {
      setGreeting(t.greeting.welcome)
      return
    }
    const hour = new Date().getHours()
    if (hour < 6)
      setGreeting(t.greeting.lateNight)
    else if (hour < 9)
      setGreeting(t.greeting.morning)
    else if (hour < 12)
      setGreeting(t.greeting.morning)
    else if (hour < 14)
      setGreeting(t.greeting.noon)
    else if (hour < 18)
      setGreeting(t.greeting.afternoon)
    else if (hour < 22)
      setGreeting(t.greeting.evening)
    else setGreeting(t.greeting.night)
  }, [isPreview, t])

  // 🔧 使用首页原子化可见性感知定时器轮播引导卡片
  useHomeVisibilityInterval(
    () => setCurrentGuideIndex(prev => (prev + 1) % navigationGuides.length),
    5000,
    !isEditMode && !isPreview,
  )

  const currentGuide = useMemo(
    () => navigationGuides[currentGuideIndex],
    [currentGuideIndex, navigationGuides],
  )

  const handleGuideClick = useCallback(() => {
    if (!isEditMode && currentGuide) {
      navigate(currentGuide.path)
    }
  }, [isEditMode, currentGuide, navigate])

  // 日期格式化 - 提取到 useMemo 避免每次渲染都格式化
  const formattedDate = useMemo(() => {
    return new Date().toLocaleDateString(locale, {
      month: 'long',
      day: 'numeric',
      weekday: 'long',
    })
  }, [locale])

  // 判断是否为2x2布局
  const is2x2 = config.size === '2x2'

  // 渲染导航图标 - 使用 useCallback 避免重复创建，与导航岛图标保持一致
  const renderNavIcon = useCallback((guide: NavigationGuide) => {
    const iconClass = 'w-5 h-5'
    if (guide.path === '/library') {
      return (
        <svg className={iconClass} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
        </svg>
      )
    }
    if (guide.path === '/reports') {
      return (
        <svg className={iconClass} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M7 12l3-3 3 3 4-4M8 21l4-4 4 4M3 4h18M4 4h16v12a1 1 0 01-1 1H5a1 1 0 01-1-1V4z" />
        </svg>
      )
    }
    if (guide.path === '/brew') {
      return (
        <svg className={iconClass} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M18 8h1a4 4 0 010 8h-1M2 8h16v9a4 4 0 01-4 4H6a4 4 0 01-4-4V8zM6 1v3M10 1v3M14 1v3" />
        </svg>
      )
    }
    // Tapp 应用 - 直接使用 SiAppstore 组件
    return <SiAppstore className={iconClass} />
  }, [])

  // 2x2 布局 - 简化版，只显示问候语，保持左上角布局
  if (is2x2) {
    return (
      <div ref={containerRef} className="relative h-full w-full rounded-xl overflow-hidden glass">
        {/* 背景装饰 */}
        <div className="absolute inset-0 bg-gradient-to-br from-gray-50/50 to-transparent dark:from-white/[0.02] dark:to-transparent" />
        <GlowBackground
          color="var(--color-primary)"
          animLevel={anim.level}
          shouldAnimate={anim.loop}
          variant="single"
          size="md"
        />

        {/* 主内容 - 左上角布局 */}
        <div
          className="relative h-full flex flex-col justify-start"
          style={{ padding: `${16 * scale}px` }}
        >
          <motion.div
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{ duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }}
          >
            <div
              className="text-3xl mb-2"
              style={{ fontSize: `${30 * fontScale}px`, marginBottom: `${8 * scale}px` }}
            >
              👋
            </div>
            <h2
              className="text-3xl font-black text-gray-800 dark:text-gray-100 leading-none mb-1.5"
              style={{ fontSize: `${30 * titleFontScale}px`, marginBottom: `${6 * scale}px` }}
            >
              {greeting}
            </h2>
            <p
              className="text-xs text-gray-500 dark:text-gray-400"
              style={{ fontSize: `${12 * infoFontScale}px` }}
            >
              {formattedDate}
            </p>
          </motion.div>
        </div>

        {isEditMode && (
          <div className="absolute inset-0 border-2 border-dashed border-blue-400 rounded-xl pointer-events-none" />
        )}
      </div>
    )
  }

  // 4x2 布局 - 完整版
  return (
    <div ref={containerRef} className="relative h-full w-full rounded-xl overflow-hidden glass">
      {/* 背景装饰 */}
      <div className="absolute inset-0 bg-gradient-to-br from-gray-50/50 to-transparent dark:from-white/[0.02] dark:to-transparent" />
      <GlowBackground
        color="var(--color-primary)"
        animLevel={anim.level}
        shouldAnimate={anim.loop}
        variant="single"
        size="lg"
      />

      {/* 主内容 - 左右布局 */}
      <div
        className="relative h-full flex flex-row"
        style={{
          padding: `${16 * scale}px`,
          gap: `${20 * scale}px`,
        }}
      >
        {/* 左侧：固定问候区 (35%) */}
        <div className="flex flex-col justify-between" style={{ width: '35%' }}>
          <motion.div
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{ duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }}
          >
            <div className="text-3xl mb-2" style={{ fontSize: `${30 * fontScale}px`, marginBottom: `${8 * scale}px` }}>👋</div>
            <h2
              className="text-3xl font-black text-gray-800 dark:text-gray-100 leading-none mb-1.5"
              style={{ fontSize: `${30 * titleFontScale}px`, marginBottom: `${6 * scale}px` }}
            >
              {greeting}
            </h2>
            <p
              className="text-xs text-gray-500 dark:text-gray-400"
              style={{ fontSize: `${12 * infoFontScale}px` }}
            >
              {formattedDate}
            </p>
          </motion.div>

          {/* 轮播指示器 */}
          <motion.div
            className="flex gap-2"
            style={{ gap: `${8 * scale}px` }}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.5, delay: 0.2 }}
          >
            {navigationGuides.map((guide, index) => (
              <motion.div
                key={index}
                className="h-1 rounded-full"
                style={{
                  backgroundColor: index === currentGuideIndex ? 'var(--color-primary)' : '#d1d5db',
                  height: `${4 * scale}px`,
                }}
                animate={{
                  width: index === currentGuideIndex ? 28 * scale : 8 * scale,
                  opacity: index === currentGuideIndex ? 1 : 0.4,
                }}
                transition={{ duration: 0.3 }}
              />
            ))}
          </motion.div>
        </div>

        {/* 右侧：动态引导卡片 (65%) */}
        <div className="flex-1 relative">
          <AnimatePresence mode="wait">
            <motion.div
              key={currentGuideIndex}
              initial={{ opacity: 0, x: 20 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -20 }}
              transition={{ duration: 0.5, ease: [0.34, 1.56, 0.64, 1] }}
              onClick={handleGuideClick}
              className="absolute inset-0 cursor-pointer"
            >
              <div
                className="relative h-full w-full rounded-lg bg-white/60 dark:bg-white/[0.03] backdrop-blur-sm hover:bg-white/80 dark:hover:bg-white/[0.05] transition-all hover:scale-[1.02] shadow-lg overflow-hidden p-4"
                style={{ padding: `${16 * scale}px` }}
              >
                <div
                  className="relative flex items-start gap-3 mb-3"
                  style={{ gap: `${12 * scale}px`, marginBottom: `${12 * scale}px` }}
                >
                  <motion.div
                    className="w-6 h-6 flex-shrink-0 flex items-center justify-center text-gray-700 dark:text-white/60"
                    style={{ width: `${24 * scale}px`, height: `${24 * scale}px` }}
                    initial={{ scale: 0.8 }}
                    animate={{ scale: 1 }}
                    transition={{ duration: 0.4, delay: 0.1 }}
                  >
                    {renderNavIcon(currentGuide)}
                  </motion.div>

                  <div className="flex-1 min-w-0">
                    <h3
                      className="text-lg font-black mb-0.5 leading-tight text-gray-800 dark:text-gray-100"
                      style={{ fontSize: `${18 * titleFontScale}px`, marginBottom: `${2 * scale}px` }}
                    >
                      {currentGuide.title}
                    </h3>
                    <p
                      className="text-[10px] text-gray-500 dark:text-gray-400"
                      style={{ fontSize: `${10 * infoFontScale}px` }}
                    >
                      {currentGuide.description}
                    </p>
                  </div>
                </div>

                {/* 功能特性 */}
                <div className="relative space-y-1.5 mb-3" style={{ marginBottom: `${12 * scale}px` }}>
                  {currentGuide.features.map((feature, index) => (
                    <motion.div
                      key={index}
                      initial={{ opacity: 0, y: 5 }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{ duration: 0.3, delay: 0.15 + index * 0.08 }}
                      className="flex items-start gap-2 text-[10px] text-gray-600 dark:text-gray-400"
                      style={{
                        gap: `${8 * scale}px`,
                        fontSize: `${10 * fontScale}px`,
                        marginBottom: `${6 * scale}px`,
                      }}
                    >
                      <div
                        className="w-1 h-1 rounded-full mt-1 flex-shrink-0 bg-gray-400 dark:bg-white/30"
                        style={{
                          width: `${4 * scale}px`,
                          height: `${4 * scale}px`,
                          marginTop: `${4 * scale}px`,
                        }}
                      />
                      <span>{feature}</span>
                    </motion.div>
                  ))}
                </div>

                {/* 前往按钮 */}
                <motion.div
                  className="relative flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-gray-700 dark:text-white/60"
                  style={{
                    gap: `${4 * scale}px`,
                    fontSize: `${10 * fontScale}px`,
                  }}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 0.4, delay: 0.4 }}
                >
                  <span>{t.common.go}</span>
                  <motion.svg
                    className="w-3 h-3"
                    style={{ width: `${12 * scale}px`, height: `${12 * scale}px` }}
                    fill="currentColor"
                    viewBox="0 0 20 20"
                    animate={canAnimate ? { x: [0, 3, 0] } : { x: 0 }}
                    transition={canAnimate ? {
                      duration: 1.5,
                      repeat: 3, // ~4.5s
                      ease: 'easeInOut',
                    } : { duration: 0 }}
                  >
                    <path fillRule="evenodd" d="M10.293 3.293a1 1 0 011.414 0l6 6a1 1 0 010 1.414l-6 6a1 1 0 01-1.414-1.414L14.586 11H3a1 1 0 110-2h11.586l-4.293-4.293a1 1 0 010-1.414z" clipRule="evenodd" />
                  </motion.svg>
                </motion.div>
              </div>
            </motion.div>
          </AnimatePresence>
        </div>
      </div>

      {isEditMode && (
        <div className="absolute inset-0 border-2 border-dashed border-blue-400 rounded-xl pointer-events-none" />
      )}
    </div>
  )
},
)

WelcomeWidget.displayName = 'WelcomeWidget'
