/**
 * 主应用入口
 * 集成路由器和布局,构建 SPA 核心
 * 优化: 代码分割 + 预加载 + 性能监控
 */

import type { ModuleVisibilityKey } from './utils/moduleVisibility'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import React, { lazy, Suspense, useEffect, useRef, useState } from 'react'
import {
  BrowserRouter,
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from 'react-router-dom'
import CustomScrollbar from './components/CustomScrollbar'
import RouteLoader from './components/RouteLoader'
import { AgentGlobalActions } from './contexts/AgentGlobalActions'
import { AnimationPreferenceProvider } from './contexts/AnimationPreferenceContext'
import { AuthProvider, useAuth } from './contexts/AuthContext'

import { I18nProvider } from './contexts/I18nContext'
import { MusicPlayerProvider } from './contexts/MusicPlayerContext'
import { NavigationProvider } from './contexts/NavigationContext'
import { PageContentProvider } from './contexts/PageContentContext'
import { ReadingListProvider } from './contexts/ReadingListContext'
import { useRouteScheduler } from './hooks/animation/useRouteScheduler'
import { isExlight, useAnimationLevel } from './hooks/useAnimationLevel'
import { AppLayout } from './layouts/AppLayout'
import { recordNavigation } from './router/navigationHistory'
import { TappDataExchangeConsentHost } from './tapp/components/TappDataExchangeConsentHost'
import { resolvePageRouteAnimation } from './tapp/routing/tappRouteMeta'
import { tappRunPath } from './tapp/utils/tappPaths'
import { preloadCriticalRoutes } from './utils/codeSplitting'
import {
  canAccessModuleVisibility,
  canUseAgent,
  useModuleVisibilityPreferences,
} from './utils/moduleVisibility'
import './styles/fonts.css'
import './styles/theme.css'
import './styles/animations.css'
/* 设置页动效系统：令牌需全局可见——设置原语（SettingItem / ManagedList 等）
   在设置页之外也会被渲染，令牌缺席会让它们的过渡整条失效 */
import './components/settings/settings-motion.css'
import './styles/page-transitions.css'
import './styles/navigation-island.css'
import './styles/utility.css'
import './styles/modals.css'
import './styles/overrides.css'
import './styles/performance.css'

// TappBackgroundRunner 懒加载，避免其错误阻塞主应用
const TappBackgroundRunner = lazy(
  () => import('./tapp/components/TappBackgroundRunner'),
) // 性能优化 CSS

// 懒加载视图组件 - 使用代码分割
const Home = lazy(() => import('./views/Home.tsx'))
const Library = lazy(() => import('./views/Library.tsx'))
const Brew = lazy(() => import('./views/Brew.tsx'))
// DEV 专用磁贴预览。仓库没有视觉回归，磁贴靠这一页人工扫；
// 路由本身包在 import.meta.env.DEV 里，生产构建不会打进这个 chunk。
const BrewTilePreview = lazy(() => import('./views/BrewTilePreview.tsx'))
const Reports = lazy(() => import('./views/Reports.tsx'))
const Config = lazy(() => import('./views/Config.tsx'))
const Login = lazy(() => import('./views/Login.tsx'))
const Register = lazy(() => import('./views/Register.tsx'))
const Setup = lazy(() => import('./views/Setup.tsx'))

// Tapp 页面（直连 pages，不再经 views 薄包装）
const TappList = lazy(() => import('./tapp/pages/TappListPage.tsx'))
const TappRun = lazy(() => import('./tapp/pages/TappRunPage.tsx'))
const TappDetail = lazy(() => import('./tapp/pages/TappDetailPage.tsx'))
const TappStore = lazy(() => import('./tapp/pages/TappStorePage.tsx'))
const TappPlayground = lazy(
  () => import('./tapp/pages/TappPlaygroundPage.tsx'),
)

// Arael AI 助手浮动面板
const AraelPanel = lazy(() => import('./components/agent/AraelPanel'))
const AraelFaceOverlay = lazy(
  () => import('./features/digital-life-companion/AraelFaceOverlay'),
)

/**
 * 路由守卫：复用全局 AuthContext 认证状态
 * 避免每次路由切换都重新发起 /api/auth/me 请求
 */
function RequireAuth({
  children,
  requiresAdmin,
}: {
  children: React.ReactNode
  requiresAdmin?: boolean
}) {
  const { isAuthenticated, isAdmin, hasChecked } = useAuth()

  // AuthContext 尚未完成首次检查
  if (!hasChecked) {
    return null
  }

  // 未认证
  if (!isAuthenticated) {
    return <Navigate to="/login" replace />
  }

  // 需要管理员权限但不是管理员
  if (requiresAdmin && !isAdmin) {
    return <Navigate to="/" replace />
  }

  return children
}

/**
 * Guest-only routes (/login, /register): authenticated users go home.
 * While auth is still resolving, show a spinner — never flash the form.
 *
 * OAuth errors (`?oauth_error=`) are toasted by useAuthUrlFeedback on
 * AppLayout while this spinner is shown (URL still has the query), so we
 * intentionally do not re-attach query params on redirect (avoids double-toast).
 */
function GuestOnly({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, hasChecked } = useAuth()

  if (!hasChecked) {
    return null
  }

  if (isAuthenticated) {
    return <Navigate to="/" replace />
  }

  return children
}

function ModuleVisibilityGuard({
  moduleKey,
  children,
}: {
  moduleKey: ModuleVisibilityKey
  children: React.ReactNode
}) {
  const { isAuthenticated, isAdmin, hasChecked } = useAuth()
  const { preferences, isLoading } = useModuleVisibilityPreferences()
  const visibility = preferences.modules[moduleKey]

  if (!hasChecked || isLoading) {
    return null
  }

  if (
    canAccessModuleVisibility(visibility, {
      isAuthenticated,
      isAdmin,
    })
  ) {
    return children
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />
  }

  return <Navigate to="/" replace />
}

/**
 * Agent（Arael AI 助手）访问门禁 - 悬浮面板，不做路由跳转，
 * 无权限时直接不渲染面板。
 * 门禁：页面可见性 + Tapp ai:chat（与权限页 Agent 预设同一真相源）。
 */
/**
 * Global Agent open_window fallback when multi-window is not mounted.
 * Typed handlers (TappWindowManager) take priority via registerActionHandler(type, …);
 * this global handler covers open_window from Arael on any route.
 */
function GlobalAgentWindowHandler() {
  const navigate = useNavigate()
  useEffect(() => {
    let cancelled = false
    let unregister: (() => void) | undefined
    void import('./services/agent').then(
      ({ registerActionHandler, unregisterActionHandler }) => {
        if (cancelled) return
        const handler = async (action: {
          type: string
          tappId?: string
          data?: Record<string, unknown>
        }) => {
          if (
            action.type !== 'open_window' &&
            action.type !== 'agent_interaction'
          ) {
            return false
          }
          const data = action.data
          const id =
            action.tappId ||
            (typeof data?.tappId === 'string' ? data.tappId : undefined) ||
            (typeof data?.tapp_id === 'string' ? data.tapp_id : undefined)
          if (!id) return false
          navigate(tappRunPath(id))
          return true
        }
        registerActionHandler(handler as never)
        unregister = () => unregisterActionHandler(handler as never)
      },
    )
    return () => {
      cancelled = true
      unregister?.()
    }
  }, [navigate])
  return null
}

function AgentAccessGate({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isAdmin, hasChecked } = useAuth()
  const { preferences, isLoading } = useModuleVisibilityPreferences()
  const [elevatedAiChat, setElevatedAiChat] = useState<
    { user: boolean; guest: boolean } | undefined
  >(undefined)
  const [permLoaded, setPermLoaded] = useState(false)

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const { fetchPermissionsConfig } = await import('./lib/api')
        const response = await fetchPermissionsConfig()
        if (cancelled) return
        if (response?.success && response.config) {
          setElevatedAiChat({
            user: !!response.config.user?.ai_chat,
            guest: !!response.config.guest?.ai_chat,
          })
        } else {
          setElevatedAiChat(undefined)
        }
      } catch {
        if (!cancelled) setElevatedAiChat(undefined)
      } finally {
        if (!cancelled) setPermLoaded(true)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  if (!hasChecked || isLoading || !permLoaded) {
    return null
  }

  if (
    !canUseAgent(
      preferences,
      {
        isAuthenticated,
        isAdmin,
      },
      elevatedAiChat,
    )
  ) {
    return null
  }

  return children
}

/**
 * 带 Suspense 的懒加载页面包装器
 * 确保每个页面独立处理加载状态，避免切换时闪屏
 *
 * fallback 为 null：引导加载器（PageLoader.astro）与各页面自己的数据
 * 加载态（如 Home 的 dashboard 配置 Spinner）已覆盖真实等待场景；
 * 这中间曾插入一个路由级小环，只在「引导屏刚谢幕、下一路由代码块
 * 还没取到」的窄缝里出现，观感上是无意义的第三次闪烁，故移除。
 */
function SuspensePage({ children }: { children: React.ReactNode }) {
  return <Suspense fallback={null}>{children}</Suspense>
}

/**
 * 带动画的页面包装器
 * 策略来自 resolvePageRouteAnimation（tappRouteMeta）
 */
function AnimatedPage({
  children,
  animationKey,
  variant,
  style,
}: {
  children: React.ReactNode
  animationKey: string
  variant: 'page' | 'fixed' | 'detail'
  style: 'normal' | 'fixed'
}) {
  const isFixed = style === 'fixed'
  const isDetail = variant === 'detail'
  const animationConfig = useAnimationLevel()
  const animationsEnabled = !isExlight(animationConfig)

  // 离开 fixed 的那一帧仍用 sync，避免 run→list/detail 先白屏再进场
  const wasFixedRef = useRef(isFixed)
  const presenceMode =
    isFixed || wasFixedRef.current ? ('sync' as const) : ('wait' as const)
  useEffect(() => {
    wasFixedRef.current = isFixed
  }, [isFixed])

  const variants =
    variant === 'fixed'
      ? fixedPageVariants
      : variant === 'detail'
        ? detailPageVariants
        : pageVariants
  const wrapperStyle = isFixed
    ? ({
        position: 'absolute' as const,
        inset: 0,
        zIndex: 20,
        // 详情是可滚动设置页；run/store 自管 overflow
        ...(isDetail ? { overflow: 'auto' as const } : {}),
      } as const)
    : ({ width: '100%', position: 'relative' as const } as const)

  return (
    <AnimatePresence mode={presenceMode} initial={false}>
      <motion.div
        key={animationKey}
        variants={animationsEnabled ? variants : undefined}
        // fixed / detail 进场：不播页面级 initial（壳层自管）
        initial={
          animationsEnabled && !isFixed && !isDetail ? 'initial' : false
        }
        animate={animationsEnabled ? 'enter' : undefined}
        exit={animationsEnabled ? 'exit' : undefined}
        style={wrapperStyle}
      >
        {children}
      </motion.div>
    </AnimatePresence>
  )
}

/**
 * 页面动画配置 - 普通页面（带 transform）
 * exit 偏淡出、少位移，叠在 Tapp 壳下时不显得「闪一下没了」
 */
const pageVariants = {
  initial: {
    opacity: 0,
    y: 20,
    scale: 0.98,
  },
  enter: {
    opacity: 1,
    y: 0,
    scale: 1,
    transition: {
      duration: 0.35,
      ease: [0.22, 1, 0.36, 1],
    },
  },
  exit: {
    opacity: 0,
    y: 8,
    scale: 0.99,
    transition: {
      duration: 0.32,
      ease: [0.4, 0, 0.2, 1],
    },
  },
}

/**
 * Fixed 全屏壳：进退场由页内 useTappShellPresence 主责。
 * 页面层 exit duration 0，避免「壳退完再淡一帧」双退。
 * 禁止进场 opacity（WebKit + iframe）；禁止 transform（破坏子树 fixed）。
 */
const fixedPageVariants = {
  initial: {},
  enter: {
    transition: { duration: 0 },
  },
  exit: {
    transition: { duration: 0 },
  },
}

/**
 * 详情：同上，壳层 presence 负责动效；页面层不二次淡出。
 */
const detailPageVariants = {
  initial: {},
  enter: {
    transition: { duration: 0 },
  },
  exit: {
    transition: { duration: 0 },
  },
}

/**
 * 路由内容组件
 */
function AppRoutes() {
  const location = useLocation()
  const routeAnim = resolvePageRouteAnimation(location.pathname)

  // 原子化调度器：在路由变化时自动管理页面生命周期
  useRouteScheduler()

  useEffect(() => {
    recordNavigation(location.pathname)
  }, [location.pathname])

  // 路由切换时恢复到顶部（fixed 叠化目的页 skipScroll）
  useEffect(() => {
    if (routeAnim.skipScroll) return
    window.scrollTo(0, 0)
  }, [location.pathname, routeAnim.skipScroll])

  // fixed 叠化时给 main 撑 min-height；离开后短延迟再摘，避免 exit 帧 main 塌缩
  useEffect(() => {
    const root = document.documentElement
    if (routeAnim.style === 'fixed') {
      root.setAttribute('data-tapp-overlay', '')
      return
    }
    const timer = window.setTimeout(() => {
      root.removeAttribute('data-tapp-overlay')
    }, 420)
    return () => window.clearTimeout(timer)
  }, [routeAnim.style])

  return (
    <AnimatedPage
      animationKey={routeAnim.key}
      style={routeAnim.style}
      variant={routeAnim.variant}
    >
      <Routes location={location}>
        <Route
          path="/"
          element={
            <SuspensePage>
              <Home />
            </SuspensePage>
          }
        />
        <Route
          path="/library"
          element={
            <ModuleVisibilityGuard moduleKey="library">
              <SuspensePage>
                <Library />
              </SuspensePage>
            </ModuleVisibilityGuard>
          }
        />
        {/* Brew 页面允许游客访问（只读），登录用户可使用已读/收藏，管理员可管理。
            使用 /brew/* 单路由，避免 /brew ↔ /brew/item/:id 切换时 remount 丢失阅读器状态。
            自有文章规范路径 /brew/item/:id 由 Brew 内部 match。 */}
        <Route
          path="/brew/*"
          element={
            <ModuleVisibilityGuard moduleKey="brew">
              <SuspensePage>
                <Brew />
              </SuspensePage>
            </ModuleVisibilityGuard>
          }
        />
        {import.meta.env.DEV && (
          <Route
            path="/dev/brew-tiles"
            element={
              <SuspensePage>
                <BrewTilePreview />
              </SuspensePage>
            }
          />
        )}
        <Route
          path="/reports"
          element={
            <ModuleVisibilityGuard moduleKey="reports">
              <SuspensePage>
                <Reports />
              </SuspensePage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/config"
          element={
            <RequireAuth requiresAdmin>
              <SuspensePage>
                <Config />
              </SuspensePage>
            </RequireAuth>
          }
        />
        <Route
          path="/login"
          element={
            <GuestOnly>
              <SuspensePage>
                <Login />
              </SuspensePage>
            </GuestOnly>
          }
        />
        <Route
          path="/register"
          element={
            <GuestOnly>
              <SuspensePage>
                <Register />
              </SuspensePage>
            </GuestOnly>
          }
        />
        <Route
          path="/setup"
          element={
            <SuspensePage>
              <Setup />
            </SuspensePage>
          }
        />

        {/* Tapp 路由 */}
        <Route
          path="/tapp"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <SuspensePage>
                <TappList />
              </SuspensePage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/run"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <SuspensePage>
                <TappRun />
              </SuspensePage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/run/:id"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <SuspensePage>
                <TappRun />
              </SuspensePage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/detail/:id"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <SuspensePage>
                <TappDetail />
              </SuspensePage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/store"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <SuspensePage>
                <TappStore />
              </SuspensePage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/playground"
          element={
            <RequireAuth requiresAdmin>
              <SuspensePage>
                <TappPlayground />
              </SuspensePage>
            </RequireAuth>
          }
        />

        {/* 404 页面 - 重定向到首页 */}
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </AnimatedPage>
  )
}

/**
 * 主应用组件
 */
export function App() {
  const [_isLayoutReady, setIsLayoutReady] = useState(false)

  // 后台 Tapp 宿主延后到首屏渲染 + 入场动画之后再挂载：
  // 它会拉起整个 tapp runtime（含沙箱/SDK 代码），不应与首屏抢主线程。
  // 后台 Tapp 本身无 UI，晚几秒启动对用户不可见。
  const [backgroundTappsReady, setBackgroundTappsReady] = useState(false)
  useEffect(() => {
    let idleId: number | null = null
    const start = () => setBackgroundTappsReady(true)
    const timerId = window.setTimeout(() => {
      if ('requestIdleCallback' in window) {
        idleId = requestIdleCallback(start, { timeout: 4000 })
      } else {
        start()
      }
    }, 3000)
    return () => {
      window.clearTimeout(timerId)
      if (idleId !== null && 'cancelIdleCallback' in window) {
        cancelIdleCallback(idleId)
      }
    }
  }, [])

  // 在 React 应用挂载完成后标记就绪状态
  // 注意：这只是通知基本框架已加载，各个组件会独立控制自己的淡入显示
  useEffect(() => {
    // 使用双帧延迟确保基础布局已渲染
    let innerRafId: number | null = null
    const rafId = requestAnimationFrame(() => {
      innerRafId = requestAnimationFrame(() => {
        setIsLayoutReady(true)

        // 通知 PageLoader 应用已就绪
        if ((window as any).pageLoader) {
          ;(window as any).pageLoader.markAppReady()
        }
      })
    })

    return () => {
      cancelAnimationFrame(rafId)
      if (innerRafId !== null) cancelAnimationFrame(innerRafId)
    }
  }, [])

  // 预加载关键路由 - 在空闲时加载Library和Config
  useEffect(() => {
    // 延迟6秒后预加载：低端设备上首屏渲染 + 小组件数据请求 + 入场动画
    // 可持续数秒，过早预取会与首屏抢主线程（preloadRoutes 内部还有
    // requestIdleCallback 二次让路）
    const timer = setTimeout(() => {
      preloadCriticalRoutes()
    }, 6000)

    return () => clearTimeout(timer)
  }, [])

  return (
    <BrowserRouter>
      <I18nProvider>
        <AnimationPreferenceProvider>
          <AuthProvider>
            <MusicPlayerProvider>
              <NavigationProvider>
                <PageContentProvider>
                  <ReadingListProvider>
                    {/* Agent 全局动作处理器 - 处理路由导航和页面元素交互 */}
                    <AgentGlobalActions />
                    {/* open_window 全局回退（多窗挂载时由 typed handler 覆盖） */}
                    <GlobalAgentWindowHandler />
                    {/* Arael AI 助手浮动面板 - 长按触发 */}
                    <AgentAccessGate>
                      <Suspense fallback={null}>
                        <AraelPanel />
                        <AraelFaceOverlay />
                      </Suspense>
                    </AgentAccessGate>
                    <RouteLoader />
                    <CustomScrollbar />
                    {backgroundTappsReady && (
                      <Suspense fallback={null}>
                        <TappBackgroundRunner />
                      </Suspense>
                    )}
                    <TappDataExchangeConsentHost />
                    <AppLayout>
                      <AppRoutes />
                    </AppLayout>
                    {/* 开发环境下显示合并的性能监控工具 */}
                    {import.meta.env.DEV && (
                      <Suspense fallback={null}>
                        {React.createElement(
                          lazy(() => import('./components/PerformanceMonitor')),
                        )}
                      </Suspense>
                    )}
                  </ReadingListProvider>
                </PageContentProvider>
              </NavigationProvider>
            </MusicPlayerProvider>
          </AuthProvider>
        </AnimationPreferenceProvider>
      </I18nProvider>
    </BrowserRouter>
  )
}

export default App
