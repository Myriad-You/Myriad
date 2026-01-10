/**
 * 主应用入口
 * 集成路由器和布局,构建 SPA 核心
 * 优化: 代码分割 + 预加载 + 性能监控
 */

import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import React, { lazy, Suspense, useEffect, useState } from 'react'
import { BrowserRouter, Navigate, Route, Routes, useLocation } from 'react-router-dom'
import CustomScrollbar from './components/CustomScrollbar'
import RouteLoader from './components/RouteLoader'
import { AnimationPreferenceProvider } from './contexts/AnimationPreferenceContext'
import { AuthProvider } from './contexts/AuthContext'
import { I18nProvider } from './contexts/I18nContext'
import { MusicPlayerProvider } from './contexts/MusicPlayerContext'
import { NavigationProvider } from './contexts/NavigationContext'
import { NotificationProvider } from './contexts/NotificationContext'
import { useRouteScheduler } from './hooks/animation'
import { AppLayout } from './layouts/AppLayout'
import { recordNavigation } from './router/navigationHistory'
import { preloadCriticalRoutes } from './utils/codeSplitting'
import './styles/fonts.css'
import './styles/theme.css'
import './styles/animations.css'
import './styles/page-transitions.css'
import './styles/navigation-island.css'
import './styles/utility.css'
import './styles/modals.css'
import './styles/overrides.css'
import './styles/performance.css'
// TappBackgroundRunner 懒加载，避免其错误阻塞主应用
const TappBackgroundRunner = lazy(() => import('./tapp/components/TappBackgroundRunner')) // 🔧 性能优化 CSS

// 懒加载视图组件 - 使用代码分割
const Home = lazy(() => import('./views/Home.tsx'))
const Library = lazy(() => import('./views/Library.tsx'))
const Brew = lazy(() => import('./views/Brew.tsx'))
const Reports = lazy(() => import('./views/Reports.tsx'))
const Config = lazy(() => import('./views/Config.tsx'))
const DataManagement = lazy(() => import('./views/DataManagement.tsx'))
const Login = lazy(() => import('./views/Login.tsx'))
const Setup = lazy(() => import('./views/Setup.tsx'))

// Tapp 页面
const TappList = lazy(() => import('./tapp/pages/TappListPage.tsx'))
const TappRun = lazy(() => import('./views/TappRunView.tsx'))
const TappDetail = lazy(() => import('./views/TappDetailView.tsx'))

/**
 * 路由守卫：检查认证状态
 * ✅ 使用 API 验证（HttpOnly Cookie 无法被 JS 读取）
 */
function RequireAuth({ children, requiresAdmin }: { children: JSX.Element, requiresAdmin?: boolean }) {
  const [isAuthenticated, setIsAuthenticated] = useState<boolean | null>(null)
  const [isAdmin, setIsAdmin] = useState(false)

  useEffect(() => {
    async function checkAuth() {
      try {
        const response = await fetch('/api/auth/me', {
          credentials: 'include',
        })

        if (response.ok) {
          const userData = await response.json()
          setIsAuthenticated(true)
          setIsAdmin(userData.is_admin || false)
        }
        else {
          // 401 是正常的未登录状态，静默处理
          setIsAuthenticated(false)
        }
      }
      catch {
        // 网络错误时静默处理
        setIsAuthenticated(false)
      }
    }

    checkAuth()
  }, [])

  // 加载中
  if (isAuthenticated === null) {
    return <LoadingFallback />
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
 * 加载指示器 - 纯光效
 * 无背景遮罩，只有优雅的光
 * 包装在 AnimatedView 中以参与页面切换动画
 */
function LoadingFallback() {
  return (
    <div className="fixed inset-0 z-[9999] pointer-events-none flex items-center justify-center">
      {/* 纯光效 - 跟随壁纸色 */}
      <div className="loading-fallback-light" />
    </div>
  )
}

/**
 * 带 Suspense 的懒加载页面包装器
 * 确保每个页面独立处理加载状态，避免切换时闪屏
 */
function SuspensePage({ children }: { children: React.ReactNode }) {
  return (
    <Suspense fallback={<LoadingFallback />}>
      {children}
    </Suspense>
  )
}

/**
 * 带动画的页面包装器
 * 确保 AnimatePresence 直接包裹 motion 组件
 */
function AnimatedPage({ children, useFixedWrapper = false }: { children: React.ReactNode, useFixedWrapper?: boolean }) {
  const location = useLocation()

  // 🎯 根据页面类型选择不同的动画配置
  const variants = useFixedWrapper ? fixedPageVariants : pageVariants
  const wrapperStyle = useFixedWrapper
    ? { position: 'absolute' as const, inset: 0 }
    : { width: '100%' }

  return (
    <AnimatePresence mode="wait">
      <motion.div
        key={location.pathname}
        variants={variants}
        initial="initial"
        animate="enter"
        exit="exit"
        style={wrapperStyle}
      >
        {children}
      </motion.div>
    </AnimatePresence>
  )
}

/**
 * 页面动画配置 - 普通页面（带 transform）
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
    y: -15,
    scale: 0.98,
    transition: {
      duration: 0.25,
      ease: [0.4, 0, 0.6, 1],
    },
  },
}

/**
 * 🎯 Fixed 布局页面动画配置 - 只用 opacity，不用 transform
 * transform 会破坏 fixed 定位（fixed 元素会相对于有 transform 的祖先定位）
 */
const fixedPageVariants = {
  initial: {
    opacity: 0,
  },
  enter: {
    opacity: 1,
    transition: {
      duration: 0.3,
      ease: [0.22, 1, 0.36, 1],
    },
  },
  exit: {
    opacity: 0,
    transition: {
      duration: 0.2,
      ease: [0.4, 0, 0.6, 1],
    },
  },
}

/**
 * 路由内容组件
 */
function AppRoutes() {
  const location = useLocation()

  // 🎯 判断是否是 fixed 布局页面（如 TappRunPage、多任务模式）
  const isFixedLayoutPage = location.pathname.startsWith('/tapp/run/') || location.pathname === '/tapp/run'

  // 🔧 原子化调度器：在路由变化时自动管理页面生命周期
  // 这会在路由切换时清理旧页面的订阅并初始化新页面
  useRouteScheduler()

  // 记录每次路由变化
  // 页面动画状态由 AnimatedView 中的 usePageTransition 自动管理
  useEffect(() => {
    recordNavigation(location.pathname)
  }, [location.pathname])

  // 路由切换时恢复到顶部
  useEffect(() => {
    window.scrollTo(0, 0)
  }, [location.pathname])

  return (
    <AnimatedPage useFixedWrapper={isFixedLayoutPage}>
      <Routes location={location}>
        <Route path="/" element={<SuspensePage><Home /></SuspensePage>} />
        <Route path="/library" element={<SuspensePage><Library /></SuspensePage>} />
        {/* Brew 页面允许游客访问（只读），登录用户可使用已读/收藏，管理员可管理 */}
        <Route path="/brew" element={<SuspensePage><Brew /></SuspensePage>} />
        <Route path="/reports" element={<SuspensePage><Reports /></SuspensePage>} />
        <Route
          path="/config"
          element={(
            <RequireAuth requiresAdmin>
              <SuspensePage><Config /></SuspensePage>
            </RequireAuth>
          )}
        />
        <Route
          path="/data-management"
          element={(
            <RequireAuth requiresAdmin>
              <SuspensePage><DataManagement /></SuspensePage>
            </RequireAuth>
          )}
        />
        <Route path="/login" element={<SuspensePage><Login /></SuspensePage>} />
        <Route path="/setup" element={<SuspensePage><Setup /></SuspensePage>} />

        {/* Tapp 路由 */}
        <Route path="/tapp" element={<SuspensePage><TappList /></SuspensePage>} />
        <Route path="/tapp/run" element={<SuspensePage><TappRun /></SuspensePage>} />
        <Route path="/tapp/run/:id" element={<SuspensePage><TappRun /></SuspensePage>} />
        <Route path="/tapp/detail/:id" element={<SuspensePage><TappDetail /></SuspensePage>} />

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
  console.debug('[App] App component rendering...')
  const [isLayoutReady, setIsLayoutReady] = useState(false)

  // 在 React 应用挂载完成后标记就绪状态
  // 注意：这只是通知基本框架已加载，各个组件会独立控制自己的淡入显示
  useEffect(() => {
    console.debug('[App] App useEffect running...')
    // 使用双帧延迟确保基础布局已渲染
    const rafId = requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        setIsLayoutReady(true)

        // 通知 PageLoader 应用已就绪
        if ((window as any).pageLoader) {
          (window as any).pageLoader.markAppReady()
        }
      })
    })

    return () => cancelAnimationFrame(rafId)
  }, [])

  // 预加载关键路由 - 在空闲时加载Library和Config
  useEffect(() => {
    // 延迟2秒后预加载,确保首屏已渲染完成
    const timer = setTimeout(() => {
      preloadCriticalRoutes()
    }, 2000)

    return () => clearTimeout(timer)
  }, [])

  return (
    <BrowserRouter>
      <I18nProvider>
        <AnimationPreferenceProvider>
          <AuthProvider>
            <NotificationProvider>
              <MusicPlayerProvider>
                <NavigationProvider>
                  <RouteLoader />
                  <CustomScrollbar />
                  <Suspense fallback={null}>
                    <TappBackgroundRunner />
                  </Suspense>
                  <AppLayout>
                    <AppRoutes />
                  </AppLayout>
                  {/* 开发环境下显示合并的性能监控工具 */}
                  {import.meta.env.DEV && (
                    <Suspense fallback={null}>
                      {React.createElement(lazy(() => import('./components/PerformanceMonitor')))}
                    </Suspense>
                  )}
                </NavigationProvider>
              </MusicPlayerProvider>
            </NotificationProvider>
          </AuthProvider>
        </AnimationPreferenceProvider>
      </I18nProvider>
    </BrowserRouter>
  )
}

export default App
