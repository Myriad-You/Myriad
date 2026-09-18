import type { ModuleVisibilityKey } from '../utils/moduleVisibility'
import type { NavLayout } from '../utils/navLayout'
import { MyriadStoreIcon } from '@lib/brandIcons'

import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { createPortal } from 'react-dom'
import { useLocation, useNavigate } from 'react-router-dom'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useNavigation } from '../contexts/NavigationContext'
import { preloadTappRoutes } from '../utils/codeSplitting'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../utils/moduleVisibility'
import {
  applyNavLayoutToDocument,
  getNavLayoutSnapshot,
  getServerNavLayoutSnapshot,
  isDesktopNavLayout,
  NAV_CHROME_SETTLED_EVENT,

  subscribeNavLayout,
} from '../utils/navLayout'
import { navigateAfterStageLeave } from './stageLeaveGate'

// 底栏↔侧轨交叉淡入时长；只在 opacity≈0 时换位置。
const NAV_CHROME_OUT_MS = 200
const NAV_CHROME_IN_MS = 280

interface ModeMetrics {
  height?: number
  width?: number
}

interface PrimaryNavItem {
  id: string
  path: string
  icon: React.ReactNode
  tooltip: string
  ariaLabel: string
  // prefix：/tapp 匹配 /tapp/xxx。
  matchPrefix?: boolean
  moduleKey?: ModuleVisibilityKey
  // seo: <a> 直接 navigate；否则 <button> 走 handleNavToPage（可展开二级）。
  asAnchor?: boolean
}

const MIN_ISLAND_HEIGHT = 48
const MAX_ISLAND_HEIGHT = 800

const IconBack = (
  <svg
    className="w-5 h-5"
    fill="none"
    stroke="currentColor"
    viewBox="0 0 24 24"
  >
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={2}
      d="M10 19l-7-7m0 0l7-7m-7 7h18"
    />
  </svg>
)
const IconHome = (
  <svg
    className="w-5 h-5"
    fill="none"
    stroke="currentColor"
    viewBox="0 0 24 24"
  >
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6"
    />
  </svg>
)
const IconLibrary = (
  <svg
    className="w-5 h-5"
    fill="none"
    stroke="currentColor"
    viewBox="0 0 24 24"
  >
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"
    />
  </svg>
)
const IconPhantasi = (
  <svg
    className="w-5 h-5"
    fill="none"
    stroke="currentColor"
    viewBox="0 0 24 24"
  >
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      d="M6 4h12a2 2 0 012 2v12a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2z"
    />
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      d="M9 4v16M12 8h5M12 12h5"
    />
  </svg>
)
const IconReports = (
  <svg
    className="w-5 h-5"
    fill="none"
    stroke="currentColor"
    viewBox="0 0 24 24"
  >
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      d="M7 12l3-3 3 3 4-4M8 21l4-4 4 4M3 4h18M4 4h16v12a1 1 0 01-1 1H5a1 1 0 01-1-1V4z"
    />
  </svg>
)

function getAnimationTiming(layout: NavLayout = getNavLayoutSnapshot()) {
  const isMobile = layout === 'mobile'
  return {
    exitStagger: isMobile ? 25 : 40,
    exitDuration: isMobile ? 240 : 380,
    enterStagger: isMobile ? 30 : 50,
    enterDelay: isMobile ? 40 : 70,
  }
}

const getVariant = (): NavLayout => getNavLayoutSnapshot()
const isDesktop = () => isDesktopNavLayout()

// 底栏↔侧轨切换时清掉内联宽高残留。
function resetIslandChromeForLayout(
  island: HTMLElement,
  layout: NavLayout,
): void {
  if (layout === 'desktop') {
    // 侧轨固定宽，勿保留移动端测得的宽；底栏按内容，勿保留桌面测得的高。
    island.style.removeProperty('width')
  } else {
    island.style.removeProperty('width')
    island.style.removeProperty('height')
  }
}

function validateHeight(height: number | null | undefined): number | null {
  if (height === null || height === undefined || !Number.isFinite(height))
    return null
  return Math.round(
    Math.min(MAX_ISLAND_HEIGHT, Math.max(MIN_ISLAND_HEIGHT, height)),
  )
}

function safeSetHeight(
  island: HTMLElement,
  height: number | null | undefined,
): boolean {
  const validHeight = validateHeight(height)
  if (validHeight !== null) {
    island.style.height = `${validHeight}px`
    return true
  }
  return false
}

function doubleRaf(callback: () => void): () => void {
  let id2: number
  const id1 = requestAnimationFrame(() => {
    id2 = requestAnimationFrame(callback)
  })
  return () => {
    cancelAnimationFrame(id1)
    cancelAnimationFrame(id2)
  }
}

function getPaddingVertical(island: HTMLElement | null): number {
  if (!island) return 0
  try {
    const styles = getComputedStyle(island)
    const paddingTop = Number.parseFloat(styles.paddingTop)
    const paddingBottom = Number.parseFloat(styles.paddingBottom)
    const result =
      (Number.isFinite(paddingTop) ? paddingTop : 0) +
      (Number.isFinite(paddingBottom) ? paddingBottom : 0)
    return Number.isFinite(result) ? result : 0
  } catch {
    return 0
  }
}

// Tooltip 用 Portal，避免 overflow:hidden 裁剪；pointerover/out 委托，relatedTarget 防闪。
const NavIslandTooltip = memo(
  ({ containerRef }: { containerRef: React.RefObject<HTMLElement | null> }) => {
    const [tooltip, setTooltip] = useState<{
      text: string
      rect: DOMRect
    } | null>(null)
    const [visible, setVisible] = useState(false)
    const showTimerRef = useRef<number>(0)
    const hideTimerRef = useRef<number>(0)
    const clearTimerRef = useRef<number>(0)
    const activeTargetRef = useRef<HTMLElement | null>(null)

    useEffect(() => {
      const container = containerRef.current
      if (!container) return

      const cancelAllTimers = () => {
        clearTimeout(showTimerRef.current)
        clearTimeout(hideTimerRef.current)
        clearTimeout(clearTimerRef.current)
      }

      const show = (target: HTMLElement) => {
        const text = target.getAttribute('data-tooltip')
        if (!text) return
        cancelAllTimers()
        const rect = target.getBoundingClientRect()
        const wasVisible = activeTargetRef.current !== null
        activeTargetRef.current = target
        setTooltip({ text, rect })
        if (wasVisible) {
          setVisible(true)
        } else {
          showTimerRef.current = window.setTimeout(setVisible, 120, true)
        }
      }

      const hide = () => {
        cancelAllTimers()
        hideTimerRef.current = window.setTimeout(() => {
          activeTargetRef.current = null
          setVisible(false)
          clearTimerRef.current = window.setTimeout(setTooltip, 120, null)
        }, 60)
      }

      const handlePointerOver = (e: PointerEvent) => {
        if (e.pointerType !== 'mouse') return
        const target = (e.target as HTMLElement)?.closest?.(
          '[data-tooltip]',
        ) as HTMLElement | null
        if (target && container.contains(target)) {
          show(target)
        }
      }

      const handlePointerOut = (e: PointerEvent) => {
        if (e.pointerType !== 'mouse') return
        const from = (e.target as HTMLElement)?.closest?.(
          '[data-tooltip]',
        ) as HTMLElement | null
        if (!from) return
        // 移向另一个 tooltip 时跳过 hide，让 pointerover 直接更新。
        const to = (e.relatedTarget as HTMLElement)?.closest?.(
          '[data-tooltip]',
        ) as HTMLElement | null
        if (to && container.contains(to)) return
        hide()
      }

      const handleLeaveContainer = () => hide()

      container.addEventListener('pointerover', handlePointerOver)
      container.addEventListener('pointerout', handlePointerOut)
      container.addEventListener('mouseleave', handleLeaveContainer)
      return () => {
        container.removeEventListener('pointerover', handlePointerOver)
        container.removeEventListener('pointerout', handlePointerOut)
        container.removeEventListener('mouseleave', handleLeaveContainer)
        cancelAllTimers()
      }
    }, [containerRef])

    if (!tooltip) return null

    const mobile = !isDesktopNavLayout()
    const style: React.CSSProperties = {
      position: 'fixed',
      zIndex: 9999,
      pointerEvents: 'none',
      whiteSpace: 'nowrap',
      fontSize: '0.75rem',
      lineHeight: 1,
      padding: '6px 10px',
      borderRadius: '8px',
      background: 'var(--bg-secondary)',
      color: 'var(--text-primary)',
      border: '1px solid var(--border-color)',
      boxShadow: '0 2px 8px var(--shadow-color)',
      opacity: visible ? 1 : 0,
      transition: 'opacity 0.12s ease',
      ...(mobile
        ? {
            left: tooltip.rect.left + tooltip.rect.width / 2,
            top: tooltip.rect.top - 10,
            transform: 'translate(-50%, -100%)',
          }
        : {
            left: tooltip.rect.right + 10,
            top: tooltip.rect.top + tooltip.rect.height / 2,
            transform: 'translateY(-50%)',
          }),
    }

    return createPortal(<div style={style}>{tooltip.text}</div>, document.body)
  },
)

export function NavigationIsland() {
  const location = useLocation()
  const navigate = useNavigate()
  const { t } = useI18n()
  const { isAuthenticated, isAdmin, hasChecked } = useAuth()
  const { preferences: moduleVisibilityPreferences } =
    useModuleVisibilityPreferences()
  const {
    secondaryNav,
    isAnimating,
    setIsAnimating,
    renderModeRef,
    immersiveMode,
  } = useNavigation()

  // subscribe 以便 store 翻转时重绘；chrome 仍由 morph runner 经 subscribeNavLayout 拥有。
  useSyncExternalStore(
    subscribeNavLayout,
    getNavLayoutSnapshot,
    getServerNavLayoutSnapshot,
  )
  // 应用中的 chrome 落后于 desired，交叉淡入时先在旧锚点淡出、不可见时换位、再淡入。
  const [chromeLayout, setChromeLayout] = useState<NavLayout>(() =>
    getNavLayoutSnapshot(),
  )
  const [chromeSwitch, setChromeSwitch] = useState<'out' | 'in' | null>(null)
  const chromeLayoutRef = useRef<NavLayout>(chromeLayout)
  chromeLayoutRef.current = chromeLayout
  const chromeSwitchingRef = useRef(false)
  const chromeTimersRef = useRef<{ out?: number; in?: number }>({})
  // 给 morph 定时器最新 helpers，避免 effect 重入取消。
  const chromeHelpersRef = useRef<{
    getCachedPadding: (island: HTMLElement) => number
    updateModeMetrics: (
      mode: 'normal' | 'secondary',
      metrics: ModeMetrics,
    ) => void
  }>({
    getCachedPadding: () => 0,
    updateModeMetrics: () => {},
  })

  const navContentRef = useRef<HTMLDivElement>(null)
  const navContainerRef = useRef<HTMLElement>(null)
  const lastModeRef = useRef<'normal' | 'secondary'>('normal')
  const lastNavLayoutRef = useRef<NavLayout>(chromeLayout)
  const islandMetricsRef = useRef<Record<string, ModeMetrics>>({})
  const prevPathnameRef = useRef(location.pathname)
  const cachedPaddingRef = useRef<number | null>(null)
  const exitRafRef = useRef<number>(0)
  const exitTimerRef = useRef<number>(0)
  // secondaryNav 用 ref 给路由 effect 读，不入 deps。
  const secondaryNavRef = useRef(secondaryNav)
  secondaryNavRef.current = secondaryNav

  // showSecondary 必须是 boolean：false→undefined 会改进入动画 deps，导航项会卡在不可见。
  const showSecondary = Boolean(
    secondaryNav?.expanded &&
      location.pathname.startsWith(secondaryNav.routePath),
  )

  const currentRenderMode = isAnimating
    ? renderModeRef.current
    : showSecondary
      ? 'secondary'
      : 'normal'

  const autoExpandedRef = useRef<string | null>(null)

  const getCachedPadding = useCallback((island: HTMLElement): number => {
    if (cachedPaddingRef.current !== null) return cachedPaddingRef.current
    const padding = getPaddingVertical(island)
    cachedPaddingRef.current = padding
    return padding
  }, [])

  const buildMetricsKey = useCallback(
    (mode: 'normal' | 'secondary', variant: 'desktop' | 'mobile') =>
      `${mode}-${variant}-${location.pathname}`,
    [location.pathname],
  )

  const updateModeMetrics = useCallback(
    (mode: 'normal' | 'secondary', metrics: ModeMetrics) => {
      const key = buildMetricsKey(mode, getVariant())
      islandMetricsRef.current[key] = {
        ...islandMetricsRef.current[key],
        ...metrics,
      }
    },
    [buildMetricsKey],
  )

  chromeHelpersRef.current = { getCachedPadding, updateModeMetrics }

  const applyModeMetrics = useCallback(
    (mode: 'normal' | 'secondary', island: HTMLElement) => {
      if (!island) return

      const key = buildMetricsKey(mode, getVariant())
      const metrics = islandMetricsRef.current[key]

      if (isDesktop()) {
        if (metrics?.height) {
          if (!safeSetHeight(island, metrics.height)) {
            const content = island.querySelector(
              '.nav-island-content',
            ) as HTMLElement
            if (content) {
              const fallbackHeight =
                content.scrollHeight + getCachedPadding(island)
              safeSetHeight(island, fallbackHeight)
            }
          }
        }
        island.style.removeProperty('width')
      } else {
        if (metrics?.width) {
          island.style.width = `${metrics.width}px`
        } else {
          island.style.removeProperty('width')
        }
        island.style.removeProperty('height')
      }
    },
    [buildMetricsKey, getCachedPadding],
  )

  // 路由重置必须 useLayoutEffect 且排在进入动画 effect 之前；useEffect 会摘掉刚设的 data-transitioning。
  useLayoutEffect(() => {
    if (prevPathnameRef.current !== location.pathname) {
      const prevPath = prevPathnameRef.current
      prevPathnameRef.current = location.pathname

      // 中断 handleTransition，防止退出定时器在切路由后继续藏导航项。
      cancelAnimationFrame(exitRafRef.current)
      clearTimeout(exitTimerRef.current)

      const island = navContentRef.current?.closest(
        '.dynamic-island',
      ) as HTMLElement | null
      if (island) {
        island.removeAttribute('data-transitioning')
        island.removeAttribute('data-entering')
        // 桌面保留明确内联高度作过渡起点；removeProperty 变 auto 后 auto→px 不会过渡。
        if (isDesktop()) {
          const content = navContentRef.current
          if (content) {
            const padding = getCachedPadding(island)
            const height = validateHeight(content.scrollHeight + padding)
            if (height !== null) {
              island.style.height = `${height}px`
            }
          } else {
            island.style.removeProperty('height')
          }
        }
      }

      const nav = secondaryNavRef.current
      const stayingInSecondary =
        nav?.expanded &&
        prevPath.startsWith(nav.routePath) &&
        location.pathname.startsWith(nav.routePath)

      if (stayingInSecondary) {
        renderModeRef.current = 'secondary'
      } else {
        // 离组只解锁渲染模式，不改 lastModeRef（它表示屏上模式）；由进入动画对比后播完整过渡，避免系统返回硬切。
        renderModeRef.current = 'normal'
      }
      setIsAnimating(false)
      autoExpandedRef.current = null
    }
  }, [location.pathname, renderModeRef, setIsAnimating, getCachedPadding])

  // isAnimating 超时强制重置，防止卡死。
  useEffect(() => {
    if (!isAnimating) return
    const safetyTimer = setTimeout(() => {
      setIsAnimating(false)
    }, 2000)
    return () => clearTimeout(safetyTimer)
  }, [isAnimating, setIsAnimating])

  useEffect(() => {
    return () => {
      cancelAnimationFrame(exitRafRef.current)
      clearTimeout(exitTimerRef.current)
    }
  }, [])

  useEffect(() => {
    if (
      secondaryNav &&
      location.pathname.startsWith(secondaryNav.routePath) &&
      !secondaryNav.expanded &&
      !isAnimating &&
      autoExpandedRef.current !== location.pathname
    ) {
      autoExpandedRef.current = location.pathname
      const timer = setTimeout(() => {
        window.dispatchEvent(
          new CustomEvent('nav-expand-secondary', {
            detail: { path: location.pathname },
          }),
        )
      }, 300)
      return () => clearTimeout(timer)
    }
  }, [secondaryNav, location.pathname, isAnimating])

  const handleTransition = useCallback(
    (targetMode: 'normal' | 'secondary') => {
      if (isAnimating || !secondaryNav) return

      // 主动返回一级后，本次路由的自动展开已消费，不能在退场结束后再展开。
      if (targetMode === 'normal') {
        autoExpandedRef.current = location.pathname
      }

      const content = navContentRef.current
      if (!content) {
        secondaryNav.onToggleExpand()
        return
      }

      setIsAnimating(true)
      // 退出阶段锁定旧模式，保持旧内容。
      renderModeRef.current =
        targetMode === 'secondary' ? 'normal' : 'secondary'

      const groups = Iterator.from(content.querySelectorAll('.nav-group')).toArray()
      const island = content.closest('.dynamic-island') as HTMLElement

      if (island) {
        island.setAttribute('data-transitioning', 'true')
      }

      if (island) {
        if (isDesktop()) {
          // 起始高度必须是 px；auto→px 不会走 CSS 过渡。
          if (!island.style.height) {
            island.style.height = `${island.offsetHeight}px`
          }
        } else {
          island.style.width = `${island.offsetWidth}px`
        }
      }

      const timing = getAnimationTiming()
      const exitStartTime = performance.now()

      const runExitStagger = (now: number) => {
        const elapsed = now - exitStartTime
        let allDone = true
        for (let i = 0; i < groups.length; i++) {
          const el = groups[i] as HTMLElement
          if (elapsed >= i * timing.exitStagger) {
            if (!el.getAttribute('data-animation')) {
              el.setAttribute('data-animation', 'exit')
            }
          } else {
            allDone = false
          }
        }
        if (!allDone) {
          exitRafRef.current = requestAnimationFrame(runExitStagger)
        }
      }
      groups.forEach((group) => {
        ;(group as HTMLElement).removeAttribute('data-animation')
      })
      exitRafRef.current = requestAnimationFrame(runExitStagger)

      // 退出后切内容但保持 isAnimating，直到进入动画结束。
      exitTimerRef.current = window.setTimeout(
        () => {
          // data-entering：新 DOM 不闪现。
          if (island) {
            island.setAttribute('data-entering', 'true')
          }
          // 不清理旧 DOM 的 data-animation，它们即将被卸载。
          renderModeRef.current = targetMode
          secondaryNav.onToggleExpand()
          // 不在此处 setIsAnimating(false)，等进入动画完成再解锁。
        },
        groups.length * timing.exitStagger + timing.exitDuration,
      )
    },
    [isAnimating, secondaryNav, setIsAnimating, renderModeRef, location.pathname],
  )

  const handleExpand = useCallback(
    () => handleTransition('secondary'),
    [handleTransition],
  )
  const handleCollapse = useCallback(
    () => handleTransition('normal'),
    [handleTransition],
  )

  const handlePrefetchPath = useCallback((path: string) => {
    if (path === '/tapp' || path.startsWith('/tapp/')) {
      preloadTappRoutes()
    }
  }, [])

  const handleNavToPage = useCallback(
    (path: string) => {
      handlePrefetchPath(path)
      if (location.pathname === path) {
        if (secondaryNav?.routePath === path) {
          if (!secondaryNav.expanded) {
            handleExpand()
          }
        }
      } else {
        const go = () => {
          navigate(path)
          setTimeout(() => {
            if (window.location.pathname === path) {
              window.dispatchEvent(
                new CustomEvent('nav-expand-secondary', { detail: { path } }),
              )
            }
          }, 150)
        }
        if (!navigateAfterStageLeave(go)) go()
      }
    },
    [location.pathname, secondaryNav, handleExpand, navigate, handlePrefetchPath],
  )

  useLayoutEffect(() => {
    const content = navContentRef.current
    if (!content) return

    const currentMode: 'normal' | 'secondary' = showSecondary
      ? 'secondary'
      : 'normal'
    if (lastModeRef.current === currentMode) return

    lastModeRef.current = currentMode

    const groups = content.querySelectorAll('.nav-group')
    const island = content.closest('.dynamic-island') as HTMLElement

    // 新 DOM 无残留标记，直接设 enter-initial。
    groups.forEach((group) => {
      ;(group as HTMLElement).setAttribute('data-animation', 'enter-initial')
    })

    if (island) {
      island.setAttribute('data-transitioning', 'true')
      // enter-initial 接管可见性后即可摘 entering。
      island.removeAttribute('data-entering')
    }

    let sizeWidthRaf = 0
    const cancelSizeRaf = doubleRaf(() => {
      const currentContent = navContentRef.current
      const currentIsland = currentContent?.closest(
        '.dynamic-island',
      ) as HTMLElement
      if (!currentContent || !currentIsland) return

      const padding = getCachedPadding(currentIsland)
      const scrollHeight = currentContent.scrollHeight

      if (isDesktop() && Number.isFinite(scrollHeight) && scrollHeight > 0) {
        const calculatedHeight = scrollHeight + padding
        const validHeight = validateHeight(calculatedHeight)
        if (safeSetHeight(currentIsland, validHeight)) {
          updateModeMetrics(currentMode, { height: validHeight ?? undefined })
        } else {
          currentIsland.style.removeProperty('height')
        }
      } else if (!isDesktop()) {
        const fromWidth = currentIsland.offsetWidth
        currentIsland.style.removeProperty('width')
        const naturalWidth = currentIsland.offsetWidth
        if (naturalWidth > 0 && naturalWidth !== fromWidth) {
          currentIsland.style.width = `${fromWidth}px`
          sizeWidthRaf = requestAnimationFrame(() => {
            sizeWidthRaf = 0
            currentIsland.style.width = `${naturalWidth}px`
            updateModeMetrics(currentMode, { width: naturalWidth })
          })
        } else if (naturalWidth > 0) {
          currentIsland.style.width = `${naturalWidth}px`
          updateModeMetrics(currentMode, { width: naturalWidth })
        }
        currentIsland.style.removeProperty('height')
      }
    })

    const timing = getAnimationTiming()
    let enterRafId: number
    let enterCleanupTimer: number

    const cancelEnterRaf = doubleRaf(() => {
      const enterStartTime = performance.now()
      const totalEnterDuration =
        groups.length * timing.enterStagger + timing.enterDelay

      const runEnterStagger = (now: number) => {
        const elapsed = now - enterStartTime
        let allDone = true
        for (let i = 0; i < groups.length; i++) {
          const threshold = i * timing.enterStagger + timing.enterDelay
          if (elapsed >= threshold) {
            const el = groups[i] as HTMLElement
            if (el.hasAttribute('data-animation')) {
              el.removeAttribute('data-animation')
            }
          } else {
            allDone = false
          }
        }
        if (!allDone) {
          enterRafId = requestAnimationFrame(runEnterStagger)
        }
      }
      enterRafId = requestAnimationFrame(runEnterStagger)

      enterCleanupTimer = window.setTimeout(() => {
        if (island) {
          island.removeAttribute('data-transitioning')
          island.removeAttribute('data-entering')
        }
        groups.forEach((group) => {
          ;(group as HTMLElement).removeAttribute('data-animation')
        })
        setIsAnimating(false)
      }, totalEnterDuration + 150)
    })

    return () => {
      cancelSizeRaf()
      if (sizeWidthRaf) cancelAnimationFrame(sizeWidthRaf)
      cancelEnterRaf()
      cancelAnimationFrame(enterRafId)
      if (enterCleanupTimer) clearTimeout(enterCleanupTimer)
      if (island) {
        island.removeAttribute('data-transitioning')
        island.removeAttribute('data-entering')
      }
      // 中断时清组标记：残留 enter-initial（opacity:0）会让导航项不可见直到 force-visible。
      groups.forEach((group) => {
        ;(group as HTMLElement).removeAttribute('data-animation')
      })
    }
    // deps 只有 showSecondary；含 secondaryNav 会在 activeId 变化时 cleanup 打断进入动画。
  }, [
    showSecondary,
    isAnimating,
    setIsAnimating,
    getCachedPadding,
    updateModeMetrics,
  ])

  useLayoutEffect(() => {
    applyNavLayoutToDocument(chromeLayout)
  }, [chromeLayout])

  // 底栏↔侧轨交叉淡入唯一入口：不可见时才换 data-nav-layout；勿插值 translateX(-50%)↔translateY(-50%)。中途 desire 变化等 settle 再检。
  useEffect(() => {
    const clearTimers = () => {
      if (chromeTimersRef.current.out) {
        clearTimeout(chromeTimersRef.current.out)
        chromeTimersRef.current.out = undefined
      }
      if (chromeTimersRef.current.in) {
        clearTimeout(chromeTimersRef.current.in)
        chromeTimersRef.current.in = undefined
      }
    }

    const remeasureFor = (target: NavLayout) => {
      const content = navContentRef.current
      const island = content?.closest('.dynamic-island') as HTMLElement | null
      if (!island) return
      const { getCachedPadding: pad, updateModeMetrics: metrics } =
        chromeHelpersRef.current
      cachedPaddingRef.current = null
      island.removeAttribute('data-transitioning')
      island.removeAttribute('data-entering')
      resetIslandChromeForLayout(island, target)
      if (target === 'desktop' && content) {
        const padding = pad(island)
        const height = content.scrollHeight + padding
        if (safeSetHeight(island, height)) {
          metrics(lastModeRef.current, { height })
        }
      }
    }

    const tryMorph = () => {
      const desired = getNavLayoutSnapshot()
      if (desired === chromeLayoutRef.current) return
      if (chromeSwitchingRef.current) return

      const nav = navContainerRef.current
      chromeSwitchingRef.current = true
      setChromeSwitch('out')

      if (nav) {
        // 这段 opacity/transform 交给 CSS data-nav-switch。
        nav.style.removeProperty('opacity')
        nav.style.removeProperty('transform')
        nav.style.removeProperty('pointer-events')
        nav.style.removeProperty('transition')
      }

      clearTimers()
      chromeTimersRef.current.out = window.setTimeout(() => {
        const target = getNavLayoutSnapshot()
        chromeLayoutRef.current = target
        setChromeLayout(target)
        lastNavLayoutRef.current = target
        applyNavLayoutToDocument(target)
        remeasureFor(target)

        setChromeSwitch('in')
        chromeTimersRef.current.in = window.setTimeout(() => {
          setChromeSwitch(null)
          chromeSwitchingRef.current = false
          if (nav) {
            nav.dispatchEvent(new Event(NAV_CHROME_SETTLED_EVENT))
          }
          // morph 中 desire 变了，settle 后再跟一次。
          requestAnimationFrame(() => tryMorph())
        }, NAV_CHROME_IN_MS)
      }, NAV_CHROME_OUT_MS)
    }

    tryMorph()
    const unsub = subscribeNavLayout(tryMorph)

    return () => {
      unsub()
      clearTimers()
      chromeSwitchingRef.current = false
    }
  }, [])

  useLayoutEffect(() => {
    if (chromeSwitch !== null) return
    lastNavLayoutRef.current = chromeLayout
    const content = navContentRef.current
    const island = content?.closest('.dynamic-island') as HTMLElement | null
    if (!island || !content) return
    if (chromeLayout === 'desktop') {
      resetIslandChromeForLayout(island, 'desktop')
      const padding = getCachedPadding(island)
      const height = content.scrollHeight + padding
      if (safeSetHeight(island, height)) {
        updateModeMetrics(lastModeRef.current, { height })
      }
    }
  }, [chromeLayout, chromeSwitch, getCachedPadding, updateModeMetrics])

  // 同布局内随窗口改尺寸；跨布局走交叉淡入。
  useEffect(() => {
    let timeoutId: number | null = null

    const handleResize = () => {
      if (timeoutId) {
        clearTimeout(timeoutId)
      }
      timeoutId = window.setTimeout(() => {
        if (chromeSwitchingRef.current) return
        const content = navContentRef.current
        const island = content?.closest('.dynamic-island') as HTMLElement | null
        if (!island) return
        if (getNavLayoutSnapshot() !== chromeLayoutRef.current) return

        resetIslandChromeForLayout(island, chromeLayoutRef.current)
        applyModeMetrics(lastModeRef.current, island)
      }, 100)
    }

    window.addEventListener('resize', handleResize, { passive: true })
    return () => {
      window.removeEventListener('resize', handleResize)
      if (timeoutId) {
        clearTimeout(timeoutId)
      }
    }
  }, [applyModeMetrics])

  useEffect(() => {
    if (currentRenderMode !== 'secondary') return
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !isAnimating) {
        handleCollapse()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [currentRenderMode, isAnimating, handleCollapse])

  const primaryNavItems = useMemo<PrimaryNavItem[]>(
    () => {
      const items: PrimaryNavItem[] = [
        {
          id: 'main',
          path: '/',
          icon: IconHome,
          tooltip: t.nav.home,
          ariaLabel: t.nav.backToHome,
          asAnchor: true,
        },
        {
          id: 'library',
          path: '/library',
          icon: IconLibrary,
          tooltip: t.nav.library,
          ariaLabel: t.nav.library,
          moduleKey: 'library',
        },
        {
          id: 'phantasi',
          path: '/journal',
          icon: IconPhantasi,
          tooltip: t.nav.phantasiReading,
          ariaLabel: t.nav.phantasiReading,
          moduleKey: 'phantasi',
        },
        {
          id: 'reports',
          path: '/reports',
          icon: IconReports,
          tooltip: t.nav.reports,
          ariaLabel: t.nav.reports,
          moduleKey: 'reports',
        },
        {
          id: 'tapp',
          path: '/tapp',
          icon: <MyriadStoreIcon className="w-5 h-5" />,
          // /tapp 是已装列表，商店在 /tapp/store — tip 勿用 tappStore。
          tooltip: t.nav.tapp,
          ariaLabel: t.nav.openTapp,
          matchPrefix: true,
          asAnchor: true,
          moduleKey: 'tapp',
        },
      ]

      return items.filter((item) => {
        if (!item.moduleKey || !hasChecked) return true
        return canAccessModuleVisibility(
          moduleVisibilityPreferences.modules[item.moduleKey],
          {
            isAuthenticated,
            isAdmin,
          },
        )
      })
    },
    [
      t,
      hasChecked,
      isAuthenticated,
      isAdmin,
      moduleVisibilityPreferences,
    ],
  )

  const primaryNavSignature = useMemo(
    () => primaryNavItems.map((item) => item.id).join('|'),
    [primaryNavItems],
  )

  useLayoutEffect(() => {
    if (isAnimating || currentRenderMode !== 'normal') return

    let rafId = 0
    let retryCount = 0
    const maxRetries = 8

    const measure = () => {
      const content = navContentRef.current
      const island = content?.closest('.dynamic-island') as HTMLElement | null
      if (!content || !island) return

      if (isDesktop()) {
        const padding = getCachedPadding(island)
        const scrollHeight = content.scrollHeight
        if (
          (!Number.isFinite(scrollHeight) ||
            scrollHeight < MIN_ISLAND_HEIGHT) &&
          retryCount < maxRetries
        ) {
          retryCount++
          rafId = requestAnimationFrame(measure)
          return
        }
        const validHeight = validateHeight(scrollHeight + padding)
        if (validHeight !== null) {
          safeSetHeight(island, validHeight)
          updateModeMetrics('normal', { height: validHeight })
        }
      } else {
        // 移动端项数变化后按内容重测自然宽，避免沿用全量项缓存宽。
        island.style.removeProperty('height')
        const fromWidth = island.offsetWidth
        island.style.removeProperty('width')
        const naturalWidth = island.offsetWidth
        if (naturalWidth > 0) {
          if (fromWidth > 0 && fromWidth !== naturalWidth) {
            island.style.width = `${fromWidth}px`
            rafId = requestAnimationFrame(() => {
              island.style.width = `${naturalWidth}px`
              updateModeMetrics('normal', { width: naturalWidth })
            })
          } else {
            island.style.width = `${naturalWidth}px`
            updateModeMetrics('normal', { width: naturalWidth })
          }
        }
      }
    }

    rafId = requestAnimationFrame(measure)
    return () => cancelAnimationFrame(rafId)
  }, [
    primaryNavSignature,
    currentRenderMode,
    isAnimating,
    getCachedPadding,
    updateModeMetrics,
  ])

  return (
    <nav
      ref={navContainerRef}
      className={`nav-container ${immersiveMode ? 'immersive' : ''}`}
      data-nav-layout={chromeLayout}
      data-nav-switch={chromeSwitch ?? undefined}
      aria-label={t.nav.mainNavigation}
      {...(immersiveMode && { 'aria-hidden': 'true' })}
      {...(chromeSwitch ? { 'aria-busy': 'true' } : {})}
    >
      <div className="dynamic-island" data-tour="nav">
        {/* 横向滚动放内层；overflow-x:auto 加在带 backdrop-filter 的岛上会残影。方向用 data-nav-layout，勿用 md:。 */}
        <div className="nav-island-scroll flex items-center gap-1 relative">
          {currentRenderMode === 'secondary' && secondaryNav ? (
            <div
              ref={navContentRef}
              className="nav-island-content flex items-center gap-1"
              key="secondary-mode"
              role="toolbar"
              aria-label={secondaryNav.expandHint || t.nav.mainNavigation}
              data-tour={
                secondaryNav.routePath === '/library'
                  ? 'library-filters'
                  : undefined
              }
              data-tour-fit={
                secondaryNav.routePath === '/library'
                  ? '.nav-group:not([data-group="back"]):not([data-group="divider"]) .nav-item'
                  : undefined
              }
            >
              <div className="nav-group" data-group="back">
                <button
                  onClick={handleCollapse}
                  className="nav-item"
                  data-tooltip={`${t.nav.back} (Esc)`}
                  aria-label={t.nav.backToNav}
                >
                  {IconBack}
                </button>
              </div>

              <div className="nav-group nav-group-spaced" data-group="divider">
                <div className="nav-island-divider bg-gray-300/50 dark:bg-neutral-700/50"></div>
              </div>

              {/* 教程锚在上级 content，只 fit 分类钮，不圈返回。 */}
              <div className="contents">
                {secondaryNav.items.map((item) => (
                  <div
                    key={item.id}
                    className="nav-group nav-group-spaced"
                    data-group={item.id}
                  >
                    <button
                      onClick={() => {
                        if (secondaryNav.activeId !== item.id) {
                          secondaryNav.onChange(item.id)
                          return
                        }
                        const home = item.path ?? secondaryNav.routePath
                        if (
                          location.pathname !== home &&
                          location.pathname.startsWith(`${home}/`)
                        ) {
                          navigate(home)
                        }
                      }}
                      className={`nav-item ${secondaryNav.activeId === item.id ? 'active-secondary' : ''}`}
                      data-tooltip={item.title || item.label}
                      aria-label={item.ariaLabel || item.label}
                    >
                      {item.icon}
                    </button>
                  </div>
                ))}
              </div>
            </div>
          ) : (
            <div
              ref={navContentRef}
              className="nav-island-content flex items-center gap-1"
              key="primary-mode"
              role="toolbar"
              aria-label={t.nav.mainNavigation}
            >
              {primaryNavItems.map((item, i) => {
                const active = item.matchPrefix
                  ? location.pathname === item.path ||
                    location.pathname.startsWith(`${item.path}/`)
                  : location.pathname === item.path
                const className = `nav-item ${active ? 'active' : ''}`
                const ariaCurrent = active ? 'page' : undefined
                return (
                  <div
                    key={item.id}
                    className={`nav-group${i > 0 ? ' nav-group-spaced' : ''}`}
                    data-group={item.id}
                  >
                    {item.asAnchor ? (
                      <a
                        href={item.path}
                        className={className}
                        data-tooltip={item.tooltip}
                        aria-label={item.ariaLabel}
                        aria-current={ariaCurrent}
                        onPointerEnter={() => handlePrefetchPath(item.path)}
                        onFocus={() => handlePrefetchPath(item.path)}
                        onClick={(e) => {
                          e.preventDefault()
                          handlePrefetchPath(item.path)
                          const go = () => navigate(item.path)
                          if (!navigateAfterStageLeave(go)) go()
                        }}
                      >
                        {item.icon}
                      </a>
                    ) : (
                      <button
                        className={className}
                        data-tooltip={item.tooltip}
                        aria-label={item.ariaLabel}
                        aria-current={ariaCurrent}
                        onPointerEnter={() => handlePrefetchPath(item.path)}
                        onFocus={() => handlePrefetchPath(item.path)}
                        onClick={() => handleNavToPage(item.path)}
                      >
                        {item.icon}
                      </button>
                    )}
                  </div>
                )
              })}
            </div>
          )}
        </div>
      </div>
      <NavIslandTooltip containerRef={navContainerRef} />
    </nav>
  )
}

export default NavigationIsland
