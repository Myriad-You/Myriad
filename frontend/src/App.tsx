import type { ShellNamespace } from './i18n'
import type { ModuleVisibilityKey } from './utils/moduleVisibility'
import { useLazyMotion } from '@lib/lazyMotion'
import React, {
  lazy,
  Suspense,
  useEffect,
  useRef,
  useSyncExternalStore,
} from 'react'
import {
  BrowserRouter,
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from 'react-router-dom'
import {
  clearQueuedAgentOpens,
  isAgentPanelAttached,
  subscribeAgentOpenQueue,
} from './components/agent-panel/agentPanelEvents'
import {
  AgentOpenIntentCapture,
  AgentSessionHost,
} from './components/agent-panel/AgentSessionHost'
import { BackgroundTappHost, RouteWarmup } from './components/ApplicationStartup'
import CustomScrollbar from './components/CustomScrollbar'
import { DocumentReady } from './components/DocumentReady'
import { RenderErrorBoundary } from './components/RenderErrorBoundary'
import RouteLoader from './components/RouteLoader'
import { AnimationPreferenceProvider } from './contexts/AnimationPreferenceContext'
import { AuthProvider, useAuth } from './contexts/AuthContext'
import { I18nNamespace, I18nProvider, useI18n } from './contexts/I18nContext'
import { LocaleAccountSync } from './contexts/LocaleAccountSync'

import { MusicPlayerProvider } from './contexts/MusicPlayerContext'
import { NavigationProvider } from './contexts/NavigationContext'
import { PageContentProvider } from './contexts/PageContentContext'
import { ReadingListProvider } from './contexts/ReadingListContext'
import { AgentPresenceHost } from './features/merope/AgentPresenceHost'
import { useRouteScheduler } from './hooks/animation/useRouteScheduler'
import { isExlight, useAnimationLevel } from './hooks/useAnimationLevel'
import { AppLayout } from './layouts/AppLayout'
import { recordNavigation } from './router/navigationHistory'
import { resolvePageRouteAnimation } from './tapp/routing/tappRouteMeta'
import {
  getDataExchangeConsentSnapshot,
  subscribeDataExchangeConsent,
} from './tapp/runtime/DataExchangeConsent'

import { TAPP_LIST_PATH, tappRunPath } from './tapp/utils/tappPaths'
import { routeComponents } from './utils/codeSplitting'
import {
  canAccessModuleVisibility,
  canUseAgent,
  useModuleVisibilityPreferences,
} from './utils/moduleVisibility'
import { isDocumentReady } from './utils/pageLoader'
import { usePermissionConfig } from './utils/permissionConfig'
import './styles/fonts.css'
import './styles/theme.css'
import './styles/animations.css'
import './styles/page-transitions.css'
import './styles/navigation-island.css'
import './styles/utility.css'
import './styles/overrides.css'
import './styles/performance.css'

const {
  home: Home,
  library: Library,
  phantasi: Phantasi,
  reports: Reports,
  config: Config,
  agentSettings: AgentSettings,
  login: Login,
  register: Register,
  setup: Setup,
  tapp: TappList,
  tappRun: TappRun,
  tappDetail: TappDetail,
  tappStore: TappStore,
  tappPlayground: TappPlayground,
} = routeComponents

const PerformanceMonitor = import.meta.env.DEV
  ? lazy(() => import('./components/PerformanceMonitor'))
  : () => null

const AgentPanel = lazy(() => import('./components/agent-panel/AgentPanel'))
/** Handlers for agent-issued page actions; no one can issue one before first paint. */
const AgentGlobalActions = lazy(() =>
  import('./contexts/AgentGlobalActions').then(m => ({ default: m.AgentGlobalActions })),
)
const AgentEngine = lazy(() => import('./components/agent-panel/AgentEngine'))
const TappDataExchangeConsentHost = lazy(
  () => import('./tapp/components/TappDataExchangeConsentHost'),
)

/** 弹窗 chunk 只在真有同意请求时下载；首屏访客不为它付费。 */
function TappDataExchangeConsentGate() {
  const { current } = useSyncExternalStore(
    subscribeDataExchangeConsent,
    getDataExchangeConsentSnapshot,
    getDataExchangeConsentSnapshot,
  )
  if (!current) return null
  return (
    <Suspense fallback={null}>
      <TappDataExchangeConsentHost />
    </Suspense>
  )
}

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
    void import('./services/agent/frontendActions').then(
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
  const { elevatedAiChat, loaded: permLoaded } = usePermissionConfig()

  const pending = !hasChecked || isLoading || !permLoaded
  const allowed =
    !pending &&
    canUseAgent(preferences, { isAuthenticated, isAdmin }, elevatedAiChat)

  const panelAttached = useSyncExternalStore(
    subscribeAgentOpenQueue,
    isAgentPanelAttached,
    isAgentPanelAttached,
  )

  useEffect(() => {
    if (!pending && !allowed) clearQueuedAgentOpens()
  }, [pending, allowed])

  // 面板 attach 前（判定、语言包、chunk）的长按/打开请求先排队；
  // capture 固定在第一个槽位，判定完成时不会重挂而打断进行中的长按。
  return (
    <>
      {!panelAttached && (pending || allowed) ? <AgentOpenIntentCapture /> : null}
      {allowed ? children : null}
    </>
  )
}

/** fallback null：PageLoader 与页面数据态已覆盖等待，不要再加路由级 spinner。 */
function SuspensePage({ children }: { children: React.ReactNode }) {
  return (
    <Suspense fallback={null}>
      <DocumentReady>{children}</DocumentReady>
    </Suspense>
  )
}

function NamespacedPage({
  names,
  children,
}: {
  names: readonly ShellNamespace[]
  children: React.ReactNode
}) {
  return (
    <I18nNamespace names={names}>
      <SuspensePage>{children}</SuspensePage>
    </I18nNamespace>
  )
}

/** 路由级兜底：页面渲染抛错时保留 AppLayout 外壳，不再整站白屏。 */
function RouteErrorBoundary({ children }: { children: React.ReactNode }) {
  const { t } = useI18n()
  const location = useLocation()
  return (
    <RenderErrorBoundary
      source="route"
      resetKey={location.pathname}
      fallback={({ error, reset }) => (
        <DocumentReady>
          <div
            role="alert"
            className="flex min-h-[60vh] flex-col items-center justify-center gap-3 px-6 text-center"
          >
            <p className="text-sm text-gray-500 dark:text-gray-400">
              {t.errors.pageRenderFailed}
            </p>
            {import.meta.env.DEV && error ? (
              <p className="max-w-lg truncate font-mono text-xs text-gray-400">
                {error.message}
              </p>
            ) : null}
            <button
              type="button"
              onClick={reset}
              className="rounded-lg bg-black/5 px-3 py-1.5 text-xs font-medium text-gray-700 transition-colors hover:bg-black/10 dark:bg-white/10 dark:text-gray-200 dark:hover:bg-white/15"
            >
              {t.common.retry}
            </button>
          </div>
        </DocumentReady>
      )}
    >
      {children}
    </RenderErrorBoundary>
  )
}

/**
 * Motion 按需加载。它到达时若当前路由已提交，原地把 div 换成 motion.div 会让
 * 整棵路由子树卸载重挂（慢网首访时首页会先以 variants.initial 隐形、再被重建）。
 * 因此已提交的路由保持静态终态，下一次路由切换才接入 motion。
 */
function useRouteMotion(animationKey: string) {
  const { motion, AnimatePresence } = useLazyMotion(true)
  const ready = motion && AnimatePresence ? { motion, AnimatePresence } : null
  const pin = useRef<{ key: string, isStatic: boolean } | null>(null)
  const firstKey = useRef(animationKey)
  if (pin.current?.key !== animationKey) {
    pin.current = { key: animationKey, isStatic: !ready }
  } else if (pin.current.isStatic && ready && !isDocumentReady()) {
    // 路由内容尚未提交（Suspense / 守卫仍为空），此时换类型没有代价。
    pin.current = { key: animationKey, isStatic: false }
  }
  return {
    fm: pin.current.isStatic ? null : ready,
    // 首路由沿用 initial={false}；静态首路由之后才挂上的 AnimatePresence 要放行进场。
    presenceInitial: animationKey !== firstKey.current,
  }
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
  const { fm, presenceInitial } = useRouteMotion(animationKey)

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

  if (!fm) {
    // 与 initial={false} 下 motion 的首帧一致：直接处于 enter 终态。
    return (
      <div key={animationKey} style={wrapperStyle}>
        {children}
      </div>
    )
  }

  const { AnimatePresence, motion } = fm
  return (
    <AnimatePresence mode={presenceMode} initial={presenceInitial}>
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
        {/* /journal/* 单路由，避免列表 ↔ 文章 remount 丢阅读器状态。 */}
        <Route
          path="/journal/*"
          element={
            <ModuleVisibilityGuard moduleKey="phantasi">
              <NamespacedPage names={['phantasi']}>
                <Phantasi />
              </NamespacedPage>
            </ModuleVisibilityGuard>
          }
        />
        {/* DEV 专用磁贴预览。lazy() 必须写在 DEV 分支里面，否则动态 import 仍会打进生产 chunk。 */}
        {import.meta.env.DEV && (
          <Route
            path="/dev/phantasi-tiles"
            element={
              <NamespacedPage names={['phantasi']}>
                {React.createElement(
                  lazy(() => import('./views/PhantasiTilePreview.tsx')),
                )}
              </NamespacedPage>
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
              <NamespacedPage names={['tapp', 'phantasi', 'merope', 'agentCaps']}>
                <Config />
              </NamespacedPage>
            </RequireAuth>
          }
        />
        <Route
          path="/agent/settings"
          element={
            <RequireAuth requiresAdmin>
              <NamespacedPage names={['merope', 'agentCaps']}>
                <AgentSettings />
              </NamespacedPage>
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
              <NamespacedPage names={['tapp']}>
                <TappList />
              </NamespacedPage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/run"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <NamespacedPage names={['tapp']}>
                <TappRun />
              </NamespacedPage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/run/:id"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <NamespacedPage names={['tapp']}>
                <TappRun />
              </NamespacedPage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/detail/:id"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <NamespacedPage names={['tapp']}>
                <TappDetail />
              </NamespacedPage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/store"
          element={
            <ModuleVisibilityGuard moduleKey="tapp">
              <NamespacedPage names={['tapp']}>
                <TappStore />
              </NamespacedPage>
            </ModuleVisibilityGuard>
          }
        />
        <Route
          path="/tapp/playground"
          element={
            <RequireAuth requiresAdmin>
              <NamespacedPage names={['tapp']}>
                <TappPlayground />
              </NamespacedPage>
            </RequireAuth>
          }
        />

        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </AnimatedPage>
  )
}

export function App() {
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
                    <Suspense fallback={null}>
                      <AgentGlobalActions />
                    </Suspense>
                    {/* open_window 全局回退；多窗挂载时 typed handler 覆盖 */}
                    <GlobalAgentWindowHandler />
                    <AgentAccessGate>
                      <AgentPresenceHost />
                      <I18nNamespace names={['merope', 'agentCaps']}>
                        <AgentSessionHost>
                          <Suspense fallback={null}>
                            <AgentEngine />
                            <AgentPanel />
                          </Suspense>
                        </AgentSessionHost>
                      </I18nNamespace>
                    </AgentAccessGate>
                    <RouteLoader />
                    <CustomScrollbar />
                    <BackgroundTappHost />
                    <RouteWarmup />
                    <TappDataExchangeConsentGate />
                    <AppLayout>
                      <RouteErrorBoundary>
                        <AppRoutes />
                      </RouteErrorBoundary>
                    </AppLayout>
                    {import.meta.env.DEV && (
                      <Suspense fallback={null}>
                        <PerformanceMonitor />
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
