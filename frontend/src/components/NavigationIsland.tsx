import { SiAppstore } from '@lib/icons'
import { AnimatePresence, motion } from 'motion/react'
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import navBrewSvgUrl from '../assets/icons/nav-brew.svg'
import navHomeSvgUrl from '../assets/icons/nav-home.svg'
import navLibrarySvgUrl from '../assets/icons/nav-library.svg'
import navReportsSvgUrl from '../assets/icons/nav-reports.svg'
import { useI18n } from '../contexts/I18nContext'
import { useNavigation } from '../contexts/NavigationContext'
import './NavigationIsland.css'

// 常量提取，避免重复创建
const ANIMATION_STAGGER = 20
const ENTER_STAGGER = 40
const ENTER_DELAY = 50
const HEIGHT_SPRING = { type: 'spring', stiffness: 200, damping: 20, mass: 1 } as const

function MaskIcon({ src, className }: { src: string, className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={className}
      style={{
        display: 'inline-block',
        backgroundColor: 'currentColor',
        WebkitMaskImage: `url(${src})`,
        maskImage: `url(${src})`,
        WebkitMaskRepeat: 'no-repeat',
        maskRepeat: 'no-repeat',
        WebkitMaskPosition: 'center',
        maskPosition: 'center',
        WebkitMaskSize: 'contain',
        maskSize: 'contain',
      }}
    />
  )
}

const BlurPulse = memo(({ isAnimating, transitionPulse }: { isAnimating: boolean, transitionPulse: number }) => {
  return (
    <AnimatePresence initial={false}>
      {isAnimating && (
        <motion.div
          key={`blur-${transitionPulse}`}
          className="absolute inset-0 pointer-events-none"
          style={{ borderRadius: '1.5rem' }}
          initial={{ opacity: 0, backdropFilter: 'blur(20px)' }}
          animate={{
            opacity: [0, 1, 0],
            backdropFilter: ['blur(20px)', 'blur(28px)', 'blur(20px)'],
          }}
          exit={{ opacity: 0, backdropFilter: 'blur(20px)' }}
          transition={{
            duration: 0.4,
            times: [0, 0.5, 1],
            ease: [0.25, 1.3, 0.5, 1] as const,
          }}
        />
      )}
    </AnimatePresence>
  )
})

const SecondaryNavIndicator = memo(({
  icon,
  iconKey,
  expandHint,
  fallbackIcon,
  onExpand,
}: {
  icon: React.ReactNode | null
  iconKey: string | number
  expandHint?: string
  fallbackIcon: React.ReactNode | null
  onExpand: () => void
}) => {
  const iconNode = icon || fallbackIcon

  return (
    <>
      <div
        className="nav-group nav-group-spaced"
        data-group="divider"
      >
        <div className="w-px h-6 bg-gray-300/50 dark:bg-neutral-700/50 md:w-6 md:h-px md:my-0"></div>
      </div>
      <div
        className="nav-group"
        data-group="current-secondary"
      >
        <motion.button
          whileHover={{ scale: 1.08 }}
          whileTap={{ scale: 0.92 }}
          transition={{ duration: 0.15, ease: [0.25, 1.2, 0.5, 1] as const }}
          className="nav-item opacity-60 hover:opacity-100 transition-opacity"
          title={expandHint}
          onClick={onExpand}
        >
          <span className="relative inline-flex w-5 h-5 items-center justify-center">
            <AnimatePresence initial={false} mode="sync">
              {iconNode
                ? (
                    <motion.span
                      key={iconKey}
                      className="absolute inset-0 inline-flex items-center justify-center"
                      initial={{ opacity: 0, scale: 0.88, y: 6 }}
                      animate={{ opacity: 1, scale: 1, y: 0 }}
                      exit={{ opacity: 0, scale: 0.88, y: -6 }}
                      transition={{ duration: 0.2, ease: [0.25, 1.2, 0.5, 1] as const }}
                    >
                      {iconNode}
                    </motion.span>
                  )
                : null}
            </AnimatePresence>
          </span>
        </motion.button>
      </div>
    </>
  )
})

export function NavigationIsland() {
  const location = useLocation()
  const navigate = useNavigate()
  const { t } = useI18n()
  const {
    secondaryNav,
    secondaryNavRoutePath,
    isAnimating,
    setIsAnimating,
    renderModeRef,
    immersiveMode,
  } = useNavigation()

  const islandRef = useRef<HTMLDivElement>(null)
  const navContentRef = useRef<HTMLDivElement>(null)
  const lastModeRef = useRef<'normal' | 'secondary'>('normal')
  const prevPathnameRef = useRef(location.pathname)

  // 当前是否显示二级导航
  const showSecondary = secondaryNav?.expanded && secondaryNav.routePath === location.pathname

  // 动画期间锁定的渲染模式
  const currentRenderMode = isAnimating ? renderModeRef.current : (showSecondary ? 'secondary' : 'normal')

  const canShowIndicator = secondaryNavRoutePath !== null && location.pathname.startsWith(secondaryNavRoutePath)
  const shouldShowIndicator = Boolean(canShowIndicator && !secondaryNav?.expanded)
  const pendingToggleRef = useRef<'expand' | 'collapse' | null>(null)
  const [transitionPulse, setTransitionPulse] = useState(0)

  useEffect(() => {
    console.warn('secondaryNav.routePath', secondaryNav?.routePath)
  }, [secondaryNav?.routePath])

  useEffect(() => {
    console.warn('location.pathname', location.pathname)
  }, [location.pathname])

  useEffect(() => {
    console.warn('canShowIndicator', canShowIndicator)
  }, [canShowIndicator])

  // useEffect(() => {
  //   console.warn('shouldShowIndicator', shouldShowIndicator)
  // }, [shouldShowIndicator])

  const groupVariants = useMemo(() => ({
    initial: { opacity: 0, scale: 0.88, y: 8 },
    animate: {
      opacity: 1,
      scale: 1,
      y: 0,
      transition: {
        duration: 0.32,
        ease: [0.25, 1.2, 0.5, 1] as const,
      },
    },
    exit: {
      opacity: 0,
      scale: 0.88,
      y: 8,
      transition: {
        duration: 0.25,
        ease: [0.4, 0, 1, 1] as const,
      },
    },
  }), [])

  const containerVariants = useMemo(() => ({
    initial: {},
    animate: {
      scale: [1, 0.96, 1],
      transition: {
        duration: 0.4,
        times: [0, 0.5, 1],
        ease: [0.25, 1.3, 0.5, 1] as const,
        delayChildren: ENTER_DELAY / 1000,
        staggerChildren: ENTER_STAGGER / 1000,
      },
    },
    exit: {
      transition: {
        staggerChildren: ANIMATION_STAGGER / 1000,
        staggerDirection: -1,
      },
    },
  }), [])

  const handleModeAnimationComplete = useCallback((mode: 'normal' | 'secondary') => {
    if (!isAnimating)
      return
    const pending = pendingToggleRef.current
    if (pending === 'expand' && mode === 'secondary') {
      pendingToggleRef.current = null
      setIsAnimating(false)
    }
    else if (pending === 'collapse' && mode === 'normal') {
      pendingToggleRef.current = null
      setIsAnimating(false)
    }
  }, [isAnimating, setIsAnimating])

  // 路由切换时重置状态
  useEffect(() => {
    if (prevPathnameRef.current !== location.pathname) {
      prevPathnameRef.current = location.pathname
      // 路由切换时重置为正常模式
      lastModeRef.current = 'normal'
      renderModeRef.current = 'normal'
      pendingToggleRef.current = null
      setIsAnimating(false)
    }
  }, [location.pathname, renderModeRef, setIsAnimating])

  // 处理展开二级导航（带退出动画）
  const handleExpand = useCallback(() => {
    if (isAnimating || !secondaryNav)
      return

    const content = navContentRef.current
    if (!content) {
      secondaryNav.onToggleExpand()
      return
    }

    setIsAnimating(true)
    pendingToggleRef.current = 'expand'
    renderModeRef.current = 'normal'

    secondaryNav.onToggleExpand()
    requestAnimationFrame(() => {
      renderModeRef.current = 'secondary'
    })
  }, [isAnimating, secondaryNav, setIsAnimating, renderModeRef])

  // 处理收起二级导航（返回按钮）
  const handleCollapse = useCallback(() => {
    if (isAnimating || !secondaryNav)
      return

    const content = navContentRef.current
    if (!content) {
      secondaryNav.onToggleExpand()
      return
    }

    setIsAnimating(true)
    pendingToggleRef.current = 'collapse'
    renderModeRef.current = 'secondary'

    secondaryNav.onToggleExpand()
    requestAnimationFrame(() => {
      renderModeRef.current = 'normal'
    })
  }, [isAnimating, secondaryNav, setIsAnimating, renderModeRef])

  // 导航到页面并展开二级导航
  const handleNavToPage = useCallback((path: string) => {
    if (location.pathname === path) {
      // 已在目标页面
      if (secondaryNav?.routePath === path) {
        // 有二级导航，无论当前是否展开，都触发展开
        // 如果已展开则不做任何事，如果未展开则展开
        if (!secondaryNav.expanded) {
          handleExpand()
        }
      }
    }
    else {
      // 导航到目标页面
      navigate(path)
      // 等待路由更新后展开
      if (window.location.pathname === path) {
        // 通过事件通知页面展开二级导航
        window.dispatchEvent(new CustomEvent('nav-expand-secondary', { detail: { path } }))
      }
    }
  }, [location.pathname, secondaryNav, handleExpand, navigate])

  // 进入动画
  useLayoutEffect(() => {
    const content = navContentRef.current
    if (!content)
      return

    const currentMode: 'normal' | 'secondary' = currentRenderMode
    if (lastModeRef.current === currentMode)
      return

    lastModeRef.current = currentMode
    setTransitionPulse(v => v + 1)
  }, [currentRenderMode])

  // 获取当前选中的二级导航项图标 - 使用 useMemo 缓存
  const activeSecondaryIcon = useMemo(() => {
    if (!secondaryNav)
      return null
    const activeItem = secondaryNav.items.find(item => item.id === secondaryNav.activeId)
    return activeItem?.icon || null
  }, [secondaryNav])

  const fallbackSecondaryIcon = useMemo(() => {
    if (secondaryNavRoutePath === '/library')
      return <MaskIcon src={navLibrarySvgUrl.src} className="w-5 h-5" />
    if (secondaryNavRoutePath === '/brew')
      return <MaskIcon src={navBrewSvgUrl.src} className="w-5 h-5" />
    if (secondaryNavRoutePath === '/reports')
      return <MaskIcon src={navReportsSvgUrl.src} className="w-5 h-5" />
    return null
  }, [secondaryNavRoutePath])

  const primaryNavItems = useMemo(() => ([
    {
      id: 'main',
      path: '/',
      title: t.nav.home,
      ariaLabel: t.nav.backToHome,
      icon: <MaskIcon src={navHomeSvgUrl.src} className="w-5 h-5" />,
      active: location.pathname === '/',
      spaced: false,
    },
    {
      id: 'library',
      path: '/library',
      title: t.nav.library,
      ariaLabel: t.nav.library,
      icon: <MaskIcon src={navLibrarySvgUrl.src} className="w-5 h-5" />,
      active: location.pathname === '/library',
      spaced: true,
    },
    {
      id: 'brew',
      path: '/brew',
      title: t.nav.brewReading,
      ariaLabel: t.nav.brewReading,
      icon: <MaskIcon src={navBrewSvgUrl.src} className="w-5 h-5" />,
      active: location.pathname === '/brew',
      spaced: true,
    },
    {
      id: 'reports',
      path: '/reports',
      title: t.nav.reports,
      ariaLabel: t.nav.reports,
      icon: <MaskIcon src={navReportsSvgUrl.src} className="w-5 h-5" />,
      active: location.pathname === '/reports',
      spaced: true,
    },
    {
      id: 'tapp',
      path: '/tapp',
      title: t.nav.tappStore,
      ariaLabel: t.nav.openTappStore,
      icon: <SiAppstore className="w-5 h-5" />,
      active: location.pathname === '/tapp' || location.pathname.startsWith('/tapp/'),
      spaced: true,
    },
  ]), [
    location.pathname,
    t.nav.backToHome,
    t.nav.brewReading,
    t.nav.home,
    t.nav.library,
    t.nav.openTappStore,
    t.nav.reports,
    t.nav.tappStore,
  ])

  return (
    <nav
      className={`nav-container ${immersiveMode ? 'immersive' : ''}`}
      aria-label={t.nav.mainNavigation}
      {...(immersiveMode && { 'aria-hidden': 'true' })}
    >
      <motion.div
        ref={islandRef}
        className="dynamic-island shadow-2xl relative overflow-hidden"
        role="navigation"
        initial={false}
        layout
        transition={{ layout: HEIGHT_SPRING }}
      >
        <BlurPulse isAnimating={isAnimating} transitionPulse={transitionPulse} />
        <div className="flex flex-row md:flex-col items-center gap-1 relative">
          <AnimatePresence mode="sync" initial={false}>
            <motion.div
              key={currentRenderMode === 'secondary' && secondaryNav ? 'secondary-mode' : 'primary-mode'}
              className="flex flex-row md:flex-col"
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={{ height: HEIGHT_SPRING, opacity: { duration: 0.2 } }}
              layout
            >
              {currentRenderMode === 'secondary' && secondaryNav
                ? (
                    <motion.div
                      ref={navContentRef}
                      className="nav-island-content flex flex-row md:flex-col items-center gap-1"
                      data-nav-mode="secondary"
                      variants={containerVariants}
                      initial="initial"
                      animate="animate"
                      exit="exit"
                      onAnimationComplete={() => handleModeAnimationComplete('secondary')}
                      layout
                    >
                      <motion.div className="nav-group" data-group="back" variants={groupVariants} layout>
                        <motion.button
                          whileHover={{ scale: 1.08 }}
                          whileTap={{ scale: 0.92 }}
                          transition={{ duration: 0.15, ease: [0.25, 1.2, 0.5, 1] as const }}
                          onClick={handleCollapse}
                          className="nav-item"
                          title={t.nav.back}
                          aria-label={t.nav.backToNav}
                        >
                          <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 19l-7-7m0 0l7-7m-7 7h18" />
                          </svg>
                        </motion.button>
                      </motion.div>

                      <motion.div className="nav-group nav-group-spaced" data-group="divider" variants={groupVariants} layout>
                        <div className="w-px h-6 bg-gray-300/50 dark:bg-neutral-700/50 md:w-6 md:h-px md:my-0"></div>
                      </motion.div>

                      {secondaryNav.items.map(item => (
                        <motion.div key={item.id} className="nav-group nav-group-spaced" data-group={item.id} variants={groupVariants} layout>
                          <motion.button
                            whileHover={{ scale: 1.08 }}
                            whileTap={{ scale: 0.92 }}
                            transition={{ duration: 0.15, ease: [0.25, 1.2, 0.5, 1] as const }}
                            onClick={() => secondaryNav.onChange(item.id)}
                            className={`nav-item ${secondaryNav.activeId === item.id ? 'active-secondary' : ''}`}
                            title={item.title || item.label}
                            aria-label={item.ariaLabel || item.label}
                          >
                            {item.icon}
                          </motion.button>
                        </motion.div>
                      ))}
                    </motion.div>
                  )
                : (
                    <motion.div
                      ref={navContentRef}
                      className="nav-island-content flex flex-row md:flex-col items-center gap-1"
                      data-nav-mode="normal"
                      variants={containerVariants}
                      initial="initial"
                      animate="animate"
                      exit="exit"
                      onAnimationComplete={() => handleModeAnimationComplete('normal')}
                      layout
                    >
                      {primaryNavItems.map(item => (
                        <motion.div
                          key={item.id}
                          className={`nav-group${item.spaced ? ' nav-group-spaced' : ''}`}
                          data-group={item.id}
                          variants={groupVariants}
                          layout
                        >
                          <motion.button
                            whileHover={{ scale: 1.08 }}
                            whileTap={{ scale: 0.92 }}
                            transition={{ duration: 0.15, ease: [0.25, 1.2, 0.5, 1] as const }}
                            type="button"
                            className={`nav-item ${item.active ? 'active' : ''}`}
                            title={item.title}
                            aria-label={item.ariaLabel}
                            onClick={() => handleNavToPage(item.path)}
                          >
                            {item.icon}
                          </motion.button>
                        </motion.div>
                      ))}
                    </motion.div>
                  )}
            </motion.div>
          </AnimatePresence>
          <AnimatePresence initial={false}>
            {shouldShowIndicator
              ? (
                  <motion.div
                    key="secondary-indicator"
                    className="flex flex-row md:flex-col items-center gap-1"
                    style={{ overflow: 'hidden' }}
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: 'auto', opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ height: HEIGHT_SPRING, opacity: { duration: 0.2 } }}
                    layout
                  >
                    <SecondaryNavIndicator
                      icon={activeSecondaryIcon}
                      iconKey={secondaryNav?.activeId ?? secondaryNavRoutePath ?? 'fallback'}
                      expandHint={secondaryNav?.expandHint || t.nav.expandFilters}
                      fallbackIcon={fallbackSecondaryIcon}
                      onExpand={handleExpand}
                    />
                  </motion.div>
                )
              : null}
          </AnimatePresence>
        </div>
      </motion.div>
    </nav>
  )
}

export default NavigationIsland
