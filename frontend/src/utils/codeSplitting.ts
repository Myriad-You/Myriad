/**
 * 代码分割配置
 * React.lazy() 动态导入优化
 */

import type { ComponentType } from 'react'
import { lazy } from 'react'

/**
 * 带加载状态的懒加载组件
 */
export function lazyWithPreload<T extends ComponentType<any>>(
  factory: () => Promise<{ default: T }>,
) {
  const Component = lazy(factory)

  // 添加preload方法
  ;(Component as any).preload = factory

  return Component
}

/**
 * 预加载多个路由组件
 */
export function preloadRoutes(routes: string[]): void {
  const connection = (
    navigator as Navigator & {
      connection?: { saveData?: boolean; effectiveType?: string }
    }
  ).connection

  // PageSpeed 的移动端基准会模拟慢网络。省流量或 2G 下不用
  // 非当前路由占用带宽，正常网络仍保留原有预取体验。
  if (
    connection?.saveData ||
    connection?.effectiveType === 'slow-2g' ||
    connection?.effectiveType === '2g'
  ) {
    return
  }

  const run = () => {
    routes.forEach((route) => {
      const component = routeComponents[route as keyof typeof routeComponents]
      if (component && (component as any).preload) {
        ;(component as any).preload()
      }
    })
  }

  // 空闲时预加载；无 requestIdleCallback 时退回 macrotask（否则 hover 预取永不执行）
  if ('requestIdleCallback' in window) {
    requestIdleCallback(run)
  } else {
    setTimeout(run, 0)
  }
}

/**
 * 路由组件懒加载配置
 */
export const routeComponents = {
  // 首页 - 关键路径,预加载
  home: lazyWithPreload(() => import('../views/Home')),

  // Library - 数据展示页
  library: lazyWithPreload(() => import('../views/Library')),

  // Config - 配置页
  config: lazyWithPreload(() => import('../views/Config')),

  // Setup - 设置向导
  setup: lazyWithPreload(() => import('../views/Setup')),

  // Login - 登录页
  login: lazyWithPreload(() => import('../views/Login')),

  // Tapp 主路径（列表 / 商店 / 详情 / 运行）— 须与 App.tsx lazy() 的 import 路径一致，
  // 否则 Vite 会拆成另一份 chunk，预取无效。
  tapp: lazyWithPreload(() => import('../tapp/pages/TappListPage.tsx')),
  tappStore: lazyWithPreload(() => import('../tapp/pages/TappStorePage.tsx')),
  tappDetail: lazyWithPreload(() => import('../tapp/pages/TappDetailPage.tsx')),
  tappRun: lazyWithPreload(() => import('../tapp/pages/TappRunPage.tsx')),
}

/** Home idle prefetch. Config / tapp detail / run stay on intent (hover or navigate). */
export const CRITICAL_PRELOAD_ROUTES = ['library', 'tapp', 'tappStore'] as const

/**
 * 预加载关键路由
 */
export function preloadCriticalRoutes(): void {
  preloadRoutes([...CRITICAL_PRELOAD_ROUTES])
}

/** Prefetch Tapp chunks on intent (nav hover / focus). */
export function preloadTappRoutes(): void {
  preloadRoutes(['tapp', 'tappStore', 'tappDetail', 'tappRun'])
}
