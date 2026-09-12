import type { WidgetComponentProps } from '../widgetGridTypes'
import { MyriadStoreIcon } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'

import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import {
  useHomeVisibilityInterval,
  useLoopAnimation,
} from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { ClampText, FitText } from './shared/FitText'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'

interface NavigationGuide {
  title: string
  description: string
  features: string[]
  path: string
  color: string
}

const WELCOME_ICON_ASSET = '/icons/widgets/welcome.webp'

export const WelcomeWidget = memo(
  ({ config, isEditMode, isPreview }: WidgetComponentProps) => {
    const { containerRef, scale, fontScale, height } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const anim = useAnimationLevel()
    const { t, locale } = useI18n()

    const { isAnimating } = useLoopAnimation({
      duration: 1500,
      trigger: 'mount',
      enabled: anim.loop,
    })

    const canAnimate = anim.loop && isAnimating
    const welcomeIconSize = 34 * fontScale

    const navigate = useNavigate()
    const [currentGuideIndex, setCurrentGuideIndex] = useState(0)
    const [greeting, setGreeting] = useState('')

    const navigationGuides = useMemo(
      () => [
        {
          title: t.widgets.library,
          description: t.widgets.multiPlatformAggregation,
          features: [t.widgets.libraryFeature],
          path: '/library',
          color: '#8b5cf6',
        },
        {
          title: t.widgets.dataReport,
          description: t.widgets.dualLayerAnalysis,
          features: [t.widgets.platformProfile],
          path: '/reports',
          color: '#06b6d4',
        },
        {
          title: t.widgets.brewReading,
          description: t.widgets.brewDesc,
          features: [t.widgets.brewFeature],
          path: '/brew',
          color: '#f97316',
        },
        {
          title: t.widgets.tappApps,
          description: t.widgets.tappDesc,
          features: [t.widgets.tappFeature],
          path: '/tapp',
          color: '#10b981',
        },
      ],
      [t],
    )

    useEffect(() => {
      if (isPreview) {
        setGreeting(t.greeting.welcome)
        return
      }
      const hour = new Date().getHours()
      if (hour < 6) setGreeting(t.greeting.lateNight)
      else if (hour < 9) setGreeting(t.greeting.morning)
      else if (hour < 12) setGreeting(t.greeting.morning)
      else if (hour < 14) setGreeting(t.greeting.noon)
      else if (hour < 18) setGreeting(t.greeting.afternoon)
      else if (hour < 22) setGreeting(t.greeting.evening)
      else setGreeting(t.greeting.night)
    }, [isPreview, t])

    useHomeVisibilityInterval(
      () =>
        setCurrentGuideIndex((prev) => (prev + 1) % navigationGuides.length),
      5000,
      !isEditMode && !isPreview && anim.widgetUiRotation,
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

    const renderWelcomeIcon = useCallback(
      () => (
        <img
          src={WELCOME_ICON_ASSET}
          alt=""
          aria-hidden="true"
          draggable={false}
          className="block shrink-0 object-contain drop-shadow-sm"
          style={{
            width: `${welcomeIconSize}px`,
            height: `${welcomeIconSize}px`,
          }}
        />
      ),
      [welcomeIconSize],
    )

    const formattedDate = useMemo(() => {
      return new Date().toLocaleDateString(locale, {
        month: 'long',
        day: 'numeric',
        weekday: 'long',
      })
    }, [locale])

    const is2x2 = config.size === '2x2'

    // 4x2 英语两行问候不能撑满列高，要给日期/轮播点留空。
    const greetingBoxHeight = Math.round((height || 140) * (is2x2 ? 0.44 : 0.3))

    const renderNavIcon = useCallback((guide: NavigationGuide) => {
      const iconClass = 'w-5 h-5'
      if (guide.path === '/library') {
        return (
          <svg
            className={iconClass}
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            strokeWidth={2}
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"
            />
          </svg>
        )
      }
      if (guide.path === '/reports') {
        return (
          <svg
            className={iconClass}
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            strokeWidth={2}
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M7 12l3-3 3 3 4-4M8 21l4-4 4 4M3 4h18M4 4h16v12a1 1 0 01-1 1H5a1 1 0 01-1-1V4z"
            />
          </svg>
        )
      }
      if (guide.path === '/brew') {
        return (
          <svg
            className={iconClass}
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            strokeWidth={2}
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M18 8h1a4 4 0 010 8h-1M2 8h16v9a4 4 0 01-4 4H6a4 4 0 01-4-4V8zM6 1v3M10 1v3M14 1v3"
            />
          </svg>
        )
      }
      return <MyriadStoreIcon className={iconClass} />
    }, [])

    if (is2x2) {
      return (
        <WidgetShell
          containerRef={containerRef}
          scale={scale}
          padding={16}
          contentClassName="flex flex-col justify-start"
          background={
            <GlowBackground
              color="var(--color-primary)"
              animLevel={anim.level}
              shouldAnimate={anim.loop}
              variant="single"
              size="md"
            />
          }
        >
          <motion.div
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{ duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }}
          >
            <div
              className="mb-2 flex items-center"
              style={{
                marginBottom: `${8 * scale}px`,
              }}
            >
              {renderWelcomeIcon()}
            </div>
            <FitText
              as="h2"
              className="font-black text-gray-800 dark:text-gray-100"
              max={42 * fontScale}
              min={12 * fontScale}
              maxLines={2}
              boxHeight={greetingBoxHeight}
              style={{ marginBottom: `${6 * scale}px` }}
            >
              {greeting}
            </FitText>
            <FitText
              as="p"
              className="text-gray-500 dark:text-gray-400"
              max={10 * fontScale}
              min={9 * fontScale}
            >
              {formattedDate}
            </FitText>
          </motion.div>
        </WidgetShell>
      )
    }

    return (
      <WidgetShell
        containerRef={containerRef}
        scale={scale}
        padding={16}
        contentClassName="flex flex-row"
        contentStyle={{ gap: `${20 * scale}px` }}
        background={
          <GlowBackground
            color="var(--color-primary)"
            animLevel={anim.level}
            shouldAnimate={anim.loop}
            variant="single"
            size="lg"
          />
        }
      >
        <div className="flex flex-col justify-between" style={{ width: '42%' }}>
          <motion.div
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{ duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }}
          >
            <div
              className="mb-2 flex items-center"
              style={{
                marginBottom: `${8 * scale}px`,
              }}
            >
              {renderWelcomeIcon()}
            </div>
            <FitText
              as="h2"
              className="font-black text-gray-800 dark:text-gray-100"
              max={42 * fontScale}
              min={12 * fontScale}
              maxLines={2}
              boxHeight={greetingBoxHeight}
              style={{ marginBottom: `${6 * scale}px` }}
            >
              {greeting}
            </FitText>
            <FitText
              as="p"
              className="text-gray-500 dark:text-gray-400"
              max={10 * fontScale}
              min={9 * fontScale}
            >
              {formattedDate}
            </FitText>
          </motion.div>

          <motion.div
            className="flex gap-2 shrink-0 mt-2"
            style={{ gap: `${8 * scale}px` }}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.5, delay: 0.2 }}
          >
            {navigationGuides.map((_, index) => (
              <motion.div
                key={index}
                className="h-1 rounded-full"
                style={{
                  backgroundColor:
                    index === currentGuideIndex
                      ? 'var(--color-primary)'
                      : '#d1d5db',
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
                className="relative h-full w-full rounded-lg glass-surface glass-60 transition-all hover:scale-[1.02] shadow-lg overflow-hidden p-4 flex flex-col"
                style={{ padding: `${16 * scale}px` }}
              >
                <div
                  className="relative flex items-start gap-3 mb-3 shrink-0"
                  style={{
                    gap: `${12 * scale}px`,
                    marginBottom: `${12 * scale}px`,
                  }}
                >
                  <motion.div
                    className="w-6 h-6 shrink-0 flex items-center justify-center text-gray-700 dark:text-white/60"
                    style={{
                      width: `${24 * scale}px`,
                      height: `${24 * scale}px`,
                    }}
                    initial={{ scale: 0.8 }}
                    animate={{ scale: 1 }}
                    transition={{ duration: 0.4, delay: 0.1 }}
                  >
                    {renderNavIcon(currentGuide)}
                  </motion.div>

                  <div className="flex-1 min-w-0">
                    <FitText
                      as="h3"
                      className="font-black text-gray-800 dark:text-gray-100"
                      max={17 * fontScale}
                      min={12 * fontScale}
                      style={{ marginBottom: `${2 * scale}px` }}
                    >
                      {currentGuide.title}
                    </FitText>
                    {/* 一行省略，避免特性区裁出半行残影。 */}
                    <ClampText
                      as="p"
                      className="text-gray-500 dark:text-gray-400"
                      lines={1}
                      title={currentGuide.description}
                      style={{ fontSize: `${10 * fontScale}px` }}
                    >
                      {currentGuide.description}
                    </ClampText>
                  </div>
                </div>

                <div
                  className="relative space-y-1.5 flex-1 min-h-0 overflow-hidden"
                  style={{ marginBottom: `${8 * scale}px` }}
                >
                  {currentGuide.features.map((feature, index) => (
                    <motion.div
                      key={index}
                      initial={{ opacity: 0, y: 5 }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{
                        duration: 0.3,
                        delay: 0.15 + index * 0.08,
                      }}
                      className="flex items-start gap-2 text-[10px] text-gray-600 dark:text-gray-400"
                      style={{
                        gap: `${8 * scale}px`,
                        fontSize: `${10 * fontScale}px`,
                        marginBottom: `${6 * scale}px`,
                      }}
                    >
                      <div
                        className="w-1 h-1 rounded-full mt-1 shrink-0 bg-gray-400 dark:bg-white/30"
                        style={{
                          width: `${4 * scale}px`,
                          height: `${4 * scale}px`,
                          marginTop: `${4 * scale}px`,
                        }}
                      />
                      <ClampText lines={1} title={feature}>
                        {feature}
                      </ClampText>
                    </motion.div>
                  ))}
                </div>

                <motion.div
                  className="relative flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-gray-700 dark:text-white/60 shrink-0 mt-auto"
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
                    style={{
                      width: `${12 * scale}px`,
                      height: `${12 * scale}px`,
                    }}
                    fill="currentColor"
                    viewBox="0 0 20 20"
                    animate={canAnimate ? { x: [0, 3, 0] } : { x: 0 }}
                    transition={
                      canAnimate
                        ? {
                            duration: 1.5,
                            repeat: 3,
                            ease: 'easeInOut',
                          }
                        : { duration: 0 }
                    }
                  >
                    <path
                      fillRule="evenodd"
                      d="M10.293 3.293a1 1 0 011.414 0l6 6a1 1 0 010 1.414l-6 6a1 1 0 01-1.414-1.414L14.586 11H3a1 1 0 110-2h11.586l-4.293-4.293a1 1 0 010-1.414z"
                      clipRule="evenodd"
                    />
                  </motion.svg>
                </motion.div>
              </div>
            </motion.div>
          </AnimatePresence>
        </div>
      </WidgetShell>
    )
  },
)

WelcomeWidget.displayName = 'WelcomeWidget'
