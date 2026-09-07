/**
 * 导航岛组件 - Apple Dynamic Island 风格
 *
 * 职责：
 * - 渲染一级导航（主页、资料库、Brew、报告、Tapp）
 * - 根据 NavigationContext 渲染页面声明的二级导航
 * - 处理一二级导航的切换动画
 */

import type { ModuleVisibilityKey } from '../utils/moduleVisibility'
import type { NavLayout } from '../utils/navLayout'
import { MyriadStoreIcon } from '@lib/icons'

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

/** Bottom ↔ rail crossfade timings (ms). Position only swaps while opacity≈0. */
const NAV_CHROME_OUT_MS = 200
const NAV_CHROME_IN_MS = 280

interface ModeMetrics {
  height?: number
  width?: number
}

/** 一级导航项定义 - 数据驱动渲染，避免重复 JSX */
interface PrimaryNavItem {
  id: string
  path: string
  icon: React.ReactNode
  tooltip: string
  ariaLabel: string
  /** true → active 用前缀匹配（如 /tapp 匹配 /tapp/xxx） */
  matchPrefix?: boolean
  /** 绑定到模块可见性设置；主页不设置 */
  moduleKey?: ModuleVisibilityKey
  /**
   * true → 渲染为 <a>（利于 SEO / 中键新开），点击直接 navigate；
   * false → 渲染为 <button>，点击走 handleNavToPage（可触发二级导航自动展开）
   */
  asAnchor?: boolean
}

// 常量
const MIN_ISLAND_HEIGHT = 48
const MAX_ISLAND_HEIGHT = 800

// ─── 提取 SVG 图标为模块级常量，避免每次渲染重新创建 JSX ───
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
const IconBrew = (
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
      d="M18 8h1a4 4 0 010 8h-1M2 8h16v9a4 4 0 01-4 4H6a4 4 0 01-4-4V8zM6 1v3M10 1v3M14 1v3"
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

/**
 * 设备感知的动画时序配置
 * 桌面端：从容优雅，给用户充分感知动画层次
 * 移动端 / 触控平板：敏捷紧凑，触控反馈要快
 */
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

/** Clear inline size leftovers when mobile ↔ desktop chrome swaps. */
function resetIslandChromeForLayout(
  island: HTMLElement,
  layout: NavLayout,
): void {
  if (layout === 'desktop') {
    // Desktop rail is fixed-width via CSS; never keep mobile measured width.
    island.style.removeProperty('width')
  } else {
    // Mobile bottom bar sizes to content; never keep desktop measured height.
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

/** 两帧延迟执行 - 确保浏览器完成布局后回调 */
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

/** 计算 padding（带安全检查），结果可缓存 */
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

/**
 * 导航岛 Tooltip - 使用 Portal 渲染到 body，避免被 overflow:hidden 裁剪
 * 使用事件委托：pointerover/pointerout 冒泡事件，配合 relatedTarget 实现无闪烁切换
 * memo 化：避免父组件动画状态变化时重复渲染
 */
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
        // 已经可见时直接更新内容和位置，无需延迟
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

      // 使用 pointerover/pointerout（会冒泡）实现事件委托，仅响应鼠标
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
        // 如果移向另一个 tooltip 元素，跳过 hide（让 pointerover 直接更新）
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

  /**
   * Subscribe so this component re-renders when the store flips; morph runner
   * still owns chrome via subscribeNavLayout(tryMorph). Snapshot seeds state.
   */
  useSyncExternalStore(
    subscribeNavLayout,
    getNavLayoutSnapshot,
    getServerNavLayoutSnapshot,
  )
  /**
   * Applied chrome layout (drives data-nav-layout + island CSS).
   * Lags desired during bottom↔rail crossfade so the island fades out at the
   * old anchor, swaps position while invisible, then fades in.
   */
  const [chromeLayout, setChromeLayout] = useState<NavLayout>(() =>
    getNavLayoutSnapshot(),
  )
  const [chromeSwitch, setChromeSwitch] = useState<'out' | 'in' | null>(null)
  const chromeLayoutRef = useRef<NavLayout>(chromeLayout)
  chromeLayoutRef.current = chromeLayout
  const chromeSwitchingRef = useRef(false)
  const chromeTimersRef = useRef<{ out?: number; in?: number }>({})
  /** Keep latest helpers for morph timers (avoid effect re-entry cancel). */
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
  // 缓存 padding 值，避免每次动画都触发 getComputedStyle
  const cachedPaddingRef = useRef<number | null>(null)
  // 追踪 handleTransition 内部定时器，组件卸载时清理
  const exitRafRef = useRef<number>(0)
  const exitTimerRef = useRef<number>(0)
  // 用 ref 追踪 secondaryNav，供路由变化 effect 读取（不加入 deps）
  const secondaryNavRef = useRef(secondaryNav)
  secondaryNavRef.current = secondaryNav

  // 当前是否显示二级导航（子路由也匹配，如 /brew/item/xxx 匹配 /brew）
  // 强制转为 boolean：secondaryNav 注销时若从 false 变为 undefined，
  // 会导致进入动画 effect 的 deps 变化而中断进行中的动画（导航项卡在不可见态）
  const showSecondary = Boolean(
    secondaryNav?.expanded &&
      location.pathname.startsWith(secondaryNav.routePath),
  )

  // 动画期间锁定的渲染模式
  const currentRenderMode = isAnimating
    ? renderModeRef.current
    : showSecondary
      ? 'secondary'
      : 'normal'

  // 记录是否已自动展开过（避免重复触发）
  const autoExpandedRef = useRef<string | null>(null)

  // 获取缓存的 padding，仅首次调用时触发 getComputedStyle
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

  // 路由切换时重置状态
  // 必须用 useLayoutEffect：本 effect 定义在进入动画 effect 之前，
  // 同为 layout effect 时按定义顺序先执行——先清理残留标记、解锁渲染模式，
  // 再由进入动画 effect 设置新标记。若用 useEffect（paint 之后执行），
  // 会反过来摘掉进入动画刚设置的 data-transitioning，导致呼吸动画被中途
  // 砍断、内容硬切闪屏（移动端系统返回时最明显）
  useLayoutEffect(() => {
    if (prevPathnameRef.current !== location.pathname) {
      const prevPath = prevPathnameRef.current
      prevPathnameRef.current = location.pathname

      // 中断进行中的 handleTransition — 防止退出定时器在路由切换后
      // 继续执行 renderModeRef 写入和 data-entering 设置，导致导航项残留隐藏
      cancelAnimationFrame(exitRafRef.current)
      clearTimeout(exitTimerRef.current)

      // 清除岛上可能残留的过渡标记
      const island = navContentRef.current?.closest(
        '.dynamic-island',
      ) as HTMLElement | null
      if (island) {
        island.removeAttribute('data-transitioning')
        island.removeAttribute('data-entering')
        // 桌面端：重新测量并设置正常模式高度，而非直接移除
        // 保留明确的内联高度值，为后续一级→二级过渡的 CSS transition 提供起始帧
        // （若直接 removeProperty，高度变为 auto，auto → px 无法触发 CSS 过渡动画）
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

      // 判断是否在同一二级导航组内导航（如 /brew → /brew/item/xxx）
      const nav = secondaryNavRef.current
      const stayingInSecondary =
        nav?.expanded &&
        prevPath.startsWith(nav.routePath) &&
        location.pathname.startsWith(nav.routePath)

      if (stayingInSecondary) {
        // 同组内导航：保持二级模式，不触发过渡动画
        renderModeRef.current = 'secondary'
      } else {
        // 离开二级导航组：解锁渲染模式为正常模式。
        // 注意不改写 lastModeRef —— 它表示"屏幕上当前渲染的模式"，
        // 由进入动画 effect 对比 lastModeRef 与新模式检测到 secondary → normal
        // 的切换后，播放完整的进入动画（呼吸 + 交错淡入 + 尺寸过渡），
        // 而不是内容硬切（系统返回时退出动画丢失/闪屏的根因）
        renderModeRef.current = 'normal'
      }
      setIsAnimating(false)
      // 重置自动展开标记，允许新页面自动展开
      autoExpandedRef.current = null
    }
  }, [location.pathname, renderModeRef, setIsAnimating, getCachedPadding])

  // 安全机制：防止 isAnimating 卡死，超时强制重置
  useEffect(() => {
    if (!isAnimating) return
    const safetyTimer = setTimeout(() => {
      setIsAnimating(false)
    }, 2000)
    return () => clearTimeout(safetyTimer)
  }, [isAnimating, setIsAnimating])

  // 组件卸载时清理 handleTransition 内部的定时器和 rAF
  useEffect(() => {
    return () => {
      cancelAnimationFrame(exitRafRef.current)
      clearTimeout(exitTimerRef.current)
    }
  }, [])

  // 自动展开二级导航：当进入有二级导航的页面时
  useEffect(() => {
    // 条件：有二级导航配置、当前在对应路由、尚未展开、未在动画中、还未自动展开过
    if (
      secondaryNav &&
      location.pathname.startsWith(secondaryNav.routePath) &&
      !secondaryNav.expanded &&
      !isAnimating &&
      autoExpandedRef.current !== location.pathname
    ) {
      // 标记已自动展开，避免重复触发
      autoExpandedRef.current = location.pathname
      // 延迟触发展开，等待页面初始化完成
      const timer = setTimeout(() => {
        // 触发展开事件，让页面自己处理
        window.dispatchEvent(
          new CustomEvent('nav-expand-secondary', {
            detail: { path: location.pathname },
          }),
        )
      }, 300)
      return () => clearTimeout(timer)
    }
  }, [secondaryNav, location.pathname, isAnimating])

  /**
   * 统一模式切换动画处理
   * @param targetMode - 目标模式（'secondary' = 展开, 'normal' = 收起）
   */
  const handleTransition = useCallback(
    (targetMode: 'normal' | 'secondary') => {
      if (isAnimating || !secondaryNav) return

      const content = navContentRef.current
      if (!content) {
        secondaryNav.onToggleExpand()
        return
      }

      setIsAnimating(true)
      // 锁定当前模式（退出阶段保持旧内容渲染）
      renderModeRef.current =
        targetMode === 'secondary' ? 'normal' : 'secondary'

      const groups = Array.from(content.querySelectorAll('.nav-group'))
      const island = content.closest('.dynamic-island') as HTMLElement

      // 标记过渡开始（启用 will-change、CSS 安全网）
      if (island) {
        island.setAttribute('data-transitioning', 'true')
      }

      // 将当前尺寸固定为 px 值，为 CSS 过渡提供起始帧
      if (island) {
        if (isDesktop()) {
          // 确保有明确的起始高度，避免 auto → px 无法触发 CSS 过渡
          if (!island.style.height) {
            island.style.height = `${island.offsetHeight}px`
          }
        } else {
          island.style.width = `${island.offsetWidth}px`
        }
      }

      // 退出动画 - 使用单个 rAF 循环替代多个 setTimeout，减少定时器开销
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
      // 先清除所有旧标记，然后启动 rAF 循环
      groups.forEach((group) => {
        ;(group as HTMLElement).removeAttribute('data-animation')
      })
      exitRafRef.current = requestAnimationFrame(runExitStagger)

      // 退出完成后切换内容（保持 isAnimating=true 直到进入动画结束）
      exitTimerRef.current = window.setTimeout(
        () => {
          // 标记即将加载新内容，CSS 安全网确保新 DOM 不闪现
          if (island) {
            island.setAttribute('data-entering', 'true')
          }
          // 不清理旧 DOM 的 data-animation — 它们即将被 React 卸载
          renderModeRef.current = targetMode
          secondaryNav.onToggleExpand()
          // 不在此处 setIsAnimating(false)，等进入动画完成后再解锁
        },
        groups.length * timing.exitStagger + timing.exitDuration,
      )
    },
    [isAnimating, secondaryNav, setIsAnimating, renderModeRef],
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

  // 导航到页面并展开二级导航
  const handleNavToPage = useCallback(
    (path: string) => {
      handlePrefetchPath(path)
      if (location.pathname === path) {
        // 已在目标页面
        if (secondaryNav?.routePath === path) {
          // 有二级导航，无论当前是否展开，都触发展开
          // 如果已展开则不做任何事，如果未展开则展开
          if (!secondaryNav.expanded) {
            handleExpand()
          }
        }
      } else {
        const go = () => {
          navigate(path)
          // 等待路由更新后展开
          setTimeout(() => {
            if (window.location.pathname === path) {
              // 通过事件通知页面展开二级导航
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

  // 进入动画
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

    // 直接设置 enter-initial（跳过 removeAttribute — 新 DOM 没有残留标记）
    groups.forEach((group) => {
      ;(group as HTMLElement).setAttribute('data-animation', 'enter-initial')
    })

    if (island) {
      island.setAttribute('data-transitioning', 'true')
      // 移除 entering 标记 — enter-initial 已接管可见性控制，防闪安全网可解除
      island.removeAttribute('data-entering')
    }

    // 两帧后读取尺寸并启动进入动画
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
        // 通过 force-reflow 测量新内容的自然宽度，触发 CSS 宽度过渡
        const fromWidth = currentIsland.offsetWidth
        currentIsland.style.removeProperty('width')
        const naturalWidth = currentIsland.offsetWidth // force layout，获取新内容的自然宽度
        if (naturalWidth > 0 && naturalWidth !== fromWidth) {
          // 恢复起始值，下一帧设目标值，触发 CSS 过渡
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
      // 使用单个 rAF 循环替代 N 个 setTimeout，减少定时器开销
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

      // 清理定时器：等待所有进入动画完成后解锁
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
      // 被中断时清理过渡标记
      if (island) {
        island.removeAttribute('data-transitioning')
        island.removeAttribute('data-entering')
      }
      // 同时清理组标记：进入动画被中断时若残留 enter-initial（opacity: 0），
      // 导航项会不可见直至 force-visible 兜底动画（约 2s）才恢复；
      // 移除标记后 .nav-group:not([data-animation]) 的过渡会平滑淡回可见态
      groups.forEach((group) => {
        ;(group as HTMLElement).removeAttribute('data-animation')
      })
    }
    // deps: showSecondary 是模式切换的唯一信号；不包含 secondaryNav 以避免
    // activeId 变化时触发 cleanup（会中断进行中的进入动画）。
    // lastModeRef guard 确保只在实际模式切换时执行动画。
  }, [
    showSecondary,
    isAnimating,
    setIsAnimating,
    getCachedPadding,
    updateModeMetrics,
  ])

  // 正常模式高度：挂载后 + 一级导航可见项变化时重算（见下方 primaryNavItems 之后的 effect）。
  // 不可只在 mount 量一次：鉴权/模块可见性异步生效后项数会变，否则岛高度会偏大。

  // Apply chrome layout to <html> (section padding etc.)
  useLayoutEffect(() => {
    applyNavLayoutToDocument(chromeLayout)
  }, [chromeLayout])

  /**
   * Bottom bar ↔ side rail crossfade (single owner).
   * - Fade out at old anchor
   * - Swap data-nav-layout while opacity≈0
   * - Fade in at new anchor
   * Never interpolate translateX(-50%) ↔ translateY(-50%).
   * Mid-morph desire changes are ignored until settle, then re-checked.
   */
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
        void island.offsetWidth
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
        // Let CSS data-nav-switch own opacity/transform for this sequence.
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
          // Chain if desire moved during the morph.
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

  // Chrome settled / first desktop paint: lock height for secondary transitions
  useLayoutEffect(() => {
    if (chromeSwitch !== null) return
    lastNavLayoutRef.current = chromeLayout
    const content = navContentRef.current
    const island = content?.closest('.dynamic-island') as HTMLElement | null
    if (!island || !content) return
    if (chromeLayout === 'desktop') {
      resetIslandChromeForLayout(island, 'desktop')
      void island.offsetWidth
      const padding = getCachedPadding(island)
      const height = content.scrollHeight + padding
      if (safeSetHeight(island, height)) {
        updateModeMetrics(lastModeRef.current, { height })
      }
    }
  }, [chromeLayout, chromeSwitch, getCachedPadding, updateModeMetrics])

  // 窗口大小变化时更新尺寸 - 同布局内；跨布局由 crossfade 处理
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

  // 键盘辅助：Escape 收起二级导航，提升键盘可达性
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

  // 一级导航项 - 数据驱动，仅随语言变化重建（active 在渲染时按当前路由计算）
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
          id: 'brew',
          path: '/brew',
          icon: IconBrew,
          tooltip: t.nav.brewReading,
          ariaLabel: t.nav.brewReading,
          moduleKey: 'brew',
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
          // /tapp 是已安装应用列表，商店在 /tapp/store — tip 勿用 tappStore
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

  /** 可见一级项签名：项增删时触发岛尺寸重测 */
  const primaryNavSignature = useMemo(
    () => primaryNavItems.map((item) => item.id).join('|'),
    [primaryNavItems],
  )

  // 一级导航可见集合变化后重算正常模式高度/宽度（模块可见性、鉴权完成等）
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
        // 移动端：项数变化后按内容重测自然宽度，避免沿用全量项时的缓存宽度
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
        {/*
          nav-island-scroll：移动端横向滚动放在内层，外层 dynamic-island 只做
          毛玻璃 + overflow:hidden。若把 overflow-x:auto 直接加在带
          backdrop-filter 的岛上，二级菜单项较多时滑动会产生残影。
          方向由 data-nav-layout 驱动 CSS（勿用 md:，否则平板触控带会错用侧轨样式）。
        */}
        <div className="nav-island-scroll flex items-center gap-1 relative">
          {currentRenderMode === 'secondary' && secondaryNav ? (
            /* 二级导航模式 */
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
              {/* 返回按钮 - Escape 也可收起 */}
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

              {/* 分隔符 — 横/竖由 .nav-island-divider + data-nav-layout 切换 */}
              <div className="nav-group nav-group-spaced" data-group="divider">
                <div className="nav-island-divider bg-gray-300/50 dark:bg-neutral-700/50"></div>
              </div>

              {/* 二级导航项。教程锚在上级 content，只 fit 分类钮，不圈返回。 */}
              <div className="contents">
                {secondaryNav.items.map((item) => (
                  <div
                    key={item.id}
                    className="nav-group nav-group-spaced"
                    data-group={item.id}
                  >
                    <button
                      onClick={() => {
                        secondaryNav.onChange(item.id)
                        // 在子路由（如 /brew/item/xxx）点击导航项时，返回基础路由
                        if (
                          location.pathname !== secondaryNav.routePath &&
                          location.pathname.startsWith(
                            `${secondaryNav.routePath}/`,
                          )
                        ) {
                          navigate(secondaryNav.routePath)
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
            /* 一级导航模式 */
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
