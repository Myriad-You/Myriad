import type { ComponentType } from 'react'
import { lazy } from 'react'
import { yieldToMain } from './yieldToMain'

export function lazyWithPreload<T extends ComponentType<any>>(
  factory: () => Promise<{ default: T }>,
) {
  const Component = lazy(factory)

  ;(Component as any).preload = factory

  return Component
}

export function preloadRoutes(routes: string[]): void {
  const connection = (
    navigator as Navigator & {
      connection?: { saveData?: boolean; effectiveType?: string }
    }
  ).connection

  if (
    connection?.saveData ||
    connection?.effectiveType === 'slow-2g' ||
    connection?.effectiveType === '2g'
  ) {
    return
  }

  const run = async () => {
    for (const route of routes) {
      const component = routeComponents[route as keyof typeof routeComponents]
      if (component && (component as any).preload) {
        await (component as any).preload()
        await yieldToMain()
      }
    }
  }

  // No requestIdleCallback: fall back to macrotask or hover prefetch never runs.
  if ('requestIdleCallback' in window) {
    requestIdleCallback(() => {
      void run()
    })
  } else {
    setTimeout(() => {
      void run()
    }, 0)
  }
}

export const routeComponents = {
  home: lazyWithPreload(() => import('../views/Home')),

  library: lazyWithPreload(() => import('../views/Library')),

  config: lazyWithPreload(() => import('../views/Config')),

  setup: lazyWithPreload(() => import('../views/Setup')),

  login: lazyWithPreload(() => import('../views/Login')),

  tapp: lazyWithPreload(() => import('../tapp/pages/TappListPage.tsx')),
  tappStore: lazyWithPreload(() => import('../tapp/pages/TappStorePage.tsx')),
  tappDetail: lazyWithPreload(() => import('../tapp/pages/TappDetailPage.tsx')),
  tappRun: lazyWithPreload(() => import('../tapp/pages/TappRunPage.tsx')),
}

export const CRITICAL_PRELOAD_ROUTES = ['library', 'tapp', 'tappStore'] as const

export function preloadCriticalRoutes(): void {
  preloadRoutes(Iterator.from(CRITICAL_PRELOAD_ROUTES).toArray())
}

export function preloadTappRoutes(): void {
  preloadRoutes(['tapp', 'tappStore', 'tappDetail', 'tappRun'])
}
