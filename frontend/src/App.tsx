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

import { LocaleAccountSync } from './contexts/LocaleAccountSync'
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
import { TAPP_LIST_PATH, tappRunPath } from './tapp/utils/tappPaths'
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

// 懒加载，避免其错误阻塞主应用
const TappBackgroundRunner = lazy(
  () => import('./tapp/components/TappBackgroundRunner'),
)

const Home = lazy(() => import('./views/Home.tsx'))
const Library = lazy(() => import('./views/Library.tsx'))
const Brew = lazy(() => import('./views/Brew.tsx'))
const Reports = lazy(() => import('./views/Reports.tsx'))
const Config = lazy(() => import('./views/Config.tsx'))
const Login = lazy(() => import('./views/Login.tsx'))
const Register = lazy(() => import('./views/Register.tsx'))
const Setup = lazy(() => import('./views/Setup.tsx'))

const TappList = lazy(() => import('./tapp/pages/TappListPage.tsx'))
const TappRun = lazy(() => import('./tapp/pages/TappRunPage.tsx'))
const TappDetail = lazy(() => import('./tapp/pages/TappDetailPage.tsx'))
const TappStore = lazy(() => import('./tapp/pages/TappStorePage.tsx'))
const TappPlayground = lazy(
  () => import('./tapp/pages/TappPlaygroundPage.tsx'),
)

const AgentPanel = lazy(() => import('./components/agent-panel/AgentPanel'))
const AgentEngine = lazy(() => import('./components/agent-panel/AgentEngine'))

/** 复用 AuthContext，避免路由切换再打 /api/auth/me。 */
function RequireAuth({
  children,
  requiresAdmin,
}: {
  children: React.ReactNode
  requiresAdmin?: boolean
}) {
  const { isAuthenticated, isAdmin, hasChecked } = useAuth()

  if (!hasChecked) {
    return null
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />
  }

  if (requiresAdmin && !isAdmin) {
    return <Navigate to="/" replace />
  }

  return children
}

/** 已登录去首页。解析中不要闪表单。OAuth 错误由 AppLayout toast，重定向不要再带 query。 */
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
 * 多窗未挂载时的全局 open_window。
 * TappWindowManager 的 typed handler 优先。
 */
function GlobalAgentWindowHandler() {
  const navigate = useNavigate()
  const location = useLocation()
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
          if (action.type === 'query_windows') {
            return {
              available: false,
              windows: [],
              activeWindowId: null,
              windowCount: 0,
            }
          }
          if (action.type === 'close_window') {
            const target = (
              action as {
                target?: { tappId?: string }
              }
            ).target
            const data = (action as { data?: Record<string, unknown> }).data
            const id =
              (action as { tappId?: string }).tappId ||
              target?.tappId ||
              (typeof data?.tappId === 'string' ? data.tappId : undefined)
            if (id || location.pathname.startsWith('/tapp/run')) {
              navigate(TAPP_LIST_PATH)
              return true
            }
            return false
          }
          if (action.type === 'focus_window') {
            const target = (
              action as {
                target?: { tappId?: string }
                tappId?: string
                data?: Record<string, unknown>
              }
            ).target
            const data = (action as { data?: Record<string, unknown> }).data
            const id =
              (action as { tappId?: string }).tappId ||
              target?.tappId ||
              (typeof data?.tappId === 'string' ? data.tappId : undefined)
            if (!id) return false
            navigate(tappRunPath(id))
            return true
          }
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
  }, [navigate, location.pathname])
  return null
}

/** 模块可见性 + 平台 ai_chat；无权限不渲染，不跳路由。 */
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

/** fallback null：PageLoader 与页面数据态已覆盖等待，不要再加路由级 spinner。 */
function SuspensePage({ children }: { children: React.ReactNode }) {
  return <Suspense fallback={null}>{children}</Suspense>
}

/** 策略来自 resolvePageRouteAnimation。 */
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

/** 普通页：exit 偏淡出少位移，叠在 Tapp 壳下不闪没。 */
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

/** 详情：壳层 presence 负责动效，页面层不二次淡出。 */
const detailPageVariants = {
  initial: {},
  enter: {
    transition: { duration: 0 },
  },
  exit: {
    transition: { duration: 0 },
  },
}

function AppRoutes() {
  const location = useLocation()
  const routeAnim = resolvePageRouteAnimation(location.pathname)

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
        {/* /brew/* 单路由，避免 /brew ↔ /brew/item/:id remount 丢阅读器状态。 */}
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
        {/* DEV 专用磁贴预览。lazy() 必须写在 DEV 分支里面，否则动态 import 仍会打进生产 chunk。 */}
        {import.meta.env.DEV && (
          <Route
            path="/dev/brew-tiles"
            element={
              <SuspensePage>
                {React.createElement(
                  lazy(() => import('./views/BrewTilePreview.tsx')),
                )}
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

        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </AnimatedPage>
  )
}

export function App() {
  const [_isLayoutReady, setIsLayoutReady] = useState(false)

  // 后台 Tapp 宿主延后到首屏+入场之后：会拉起整套 runtime，不与首屏抢主线程。
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

  useEffect(() => {
    let innerRafId: number | null = null
    const rafId = requestAnimationFrame(() => {
      innerRafId = requestAnimationFrame(() => {
        setIsLayoutReady(true)

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

  // 只预取资料库 / Tapp，不预取 Config。6s：过早会与首屏抢主线程。
  useEffect(() => {
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
            <LocaleAccountSync />
            <MusicPlayerProvider>
              <NavigationProvider>
                <PageContentProvider>
                  <ReadingListProvider>
                    <AgentGlobalActions />
                    {/* open_window 全局回退；多窗挂载时 typed handler 覆盖 */}
                    <GlobalAgentWindowHandler />
                    <AgentAccessGate>
                      <Suspense fallback={null}>
                        <AgentEngine />
                        <AgentPanel />
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
