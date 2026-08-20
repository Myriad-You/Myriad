/**
 * 路由调度器 Hook
 *
 * 将原子化调度器与 React Router 整合
 * 在路由变化时自动管理页面生命周期
 *
 * @example
 * ```tsx
 * function AppRoutes() {
 *   // 自动在路由切换时调用 startPage()
 *   useRouteScheduler();
 *
 *   return <Routes>...</Routes>;
 * }
 * ```
 */

import { useEffect, useRef } from 'react'
import { useLocation } from 'react-router-dom'

import {
  isPageVisible,
  onVisibility,
  pause,
  resume,
  runPageCleanup,
  startPage,
} from './core'

// 路径到页面 ID 的映射
const pathToPageId: Record<string, string> = {
  '/': 'home',
  '/library': 'library',
  '/reports': 'reports',
  '/brew': 'brew',
  '/tapp': 'tapp',
  '/config': 'config',
  '/login': 'login',
  '/details': 'details',
  '/setup': 'setup',
}

/**
 * 从路径提取页面 ID
 */
function getPageIdFromPath(pathname: string): string {
  // 精确匹配
  if (pathToPageId[pathname]) {
    return pathToPageId[pathname]
  }

  // 处理带参数的路径（如 /details/123）
  const basePath = `/${pathname.split('/')[1]}`
  if (pathToPageId[basePath]) {
    return pathToPageId[basePath]
  }

  // 未知路径使用路径本身作为 ID
  return pathname.replace(/^\//, '') || 'unknown'
}

/**
 * 路由调度器 Hook
 *
 * 在路由变化时：
 * 1. 清理旧页面的专用资源（Observer 等）
 * 2. 调用 startPage(pageId) 初始化新页面
 * 3. 页面可见性变化时暂停/恢复调度器
 *
 * 注意：这个 Hook 应该在路由组件的顶层调用，确保路由变化时立即响应
 */
export function useRouteScheduler(): void {
  const location = useLocation()
  const lastPathRef = useRef<string | null>(null)
  const lastPageIdRef = useRef<string | null>(null)

  // 路由变化时启动新页面
  useEffect(() => {
    const currentPath = location.pathname

    // 避免相同路径重复触发
    if (currentPath === lastPathRef.current) {
      return
    }

    // 清理旧页面的专用资源
    if (lastPageIdRef.current) {
      runPageCleanup(lastPageIdRef.current)
    }

    lastPathRef.current = currentPath
    const pageId = getPageIdFromPath(currentPath)
    lastPageIdRef.current = pageId

    // 调用原子化核心的 startPage
    // 这会根据页面配置初始化所需功能
    startPage(pageId)
  }, [location.pathname])

  // 首次挂载时确保调度器状态正确
  useEffect(() => {
    // 如果页面不可见，暂停调度器
    if (isPageVisible()) resume()
    else pause()

    const unsubscribeVisibility = onVisibility((visible) => {
      if (visible) resume()
      else pause()
    })

    // 组件卸载时清理最后一个页面
    return () => {
      unsubscribeVisibility()
      if (lastPageIdRef.current) {
        runPageCleanup(lastPageIdRef.current)
      }
    }
  }, [])
}

/**
 * 页面级别的调度器 Hook
 *
 * 与 useRouteScheduler 类似，但用于单个页面组件内部
 * 适合需要在页面组件内手动控制调度器生命周期的场景
 *
 * @example
 * ```tsx
 * function HomePage() {
 *   // 在组件挂载时启动页面，卸载时清理
 *   usePageScheduler('home');
 *
 *   return <div>...</div>;
 * }
 * ```
 */
export function usePageScheduler(pageId: string): void {
  useEffect(() => {
    startPage(pageId)

    // 组件卸载时清理页面专用资源
    return () => {
      runPageCleanup(pageId)
    }
  }, [pageId])
}

export default useRouteScheduler
