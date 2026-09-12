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

function getPageIdFromPath(pathname: string): string {
  if (pathToPageId[pathname]) {
    return pathToPageId[pathname]
  }

  const basePath = `/${pathname.split('/')[1]}`
  if (pathToPageId[basePath]) {
    return pathToPageId[basePath]
  }

  return pathname.replaceAll(/^\//g, '') || 'unknown'
}

/** 路由顶层调用：切页时清旧页资源并 startPage。 */
export function useRouteScheduler(): void {
  const location = useLocation()
  const lastPathRef = useRef<string | null>(null)
  const lastPageIdRef = useRef<string | null>(null)

  useEffect(() => {
    const currentPath = location.pathname

    if (currentPath === lastPathRef.current) {
      return
    }

    if (lastPageIdRef.current) {
      runPageCleanup(lastPageIdRef.current)
    }

    lastPathRef.current = currentPath
    const pageId = getPageIdFromPath(currentPath)
    lastPageIdRef.current = pageId

    startPage(pageId)
  }, [location.pathname])

  useEffect(() => {
    if (isPageVisible()) resume()
    else pause()

    const unsubscribeVisibility = onVisibility((visible) => {
      if (visible) resume()
      else pause()
    })

    return () => {
      unsubscribeVisibility()
      if (lastPageIdRef.current) {
        runPageCleanup(lastPageIdRef.current)
      }
    }
  }, [])
}

export function usePageScheduler(pageId: string): void {
  useEffect(() => {
    startPage(pageId)

    return () => {
      runPageCleanup(pageId)
    }
  }, [pageId])
}

export default useRouteScheduler
