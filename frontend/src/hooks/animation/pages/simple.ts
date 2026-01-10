/**
 * 简单页面通用调度器 Hooks
 *
 * 适用于：Config、Login、Setup、DataManagement、Details
 * 这些页面只需要基础的 Timeout 功能（防抖、延迟跳转等）
 *
 * @example
 * ```tsx
 * import { useSimplePageScheduler, useSimpleTimeout, useSimpleDebounce } from '@hooks/animation/pages/simple';
 *
 * function Config() {
 *   useSimplePageScheduler('config');
 *
 *   const debouncedSave = useSimpleDebounce((value) => {
 *     saveConfig(value);
 *   }, 500);
 *
 *   return <ConfigForm onChange={debouncedSave} />;
 * }
 * ```
 */

import { useCallback, useEffect, useRef } from 'react'
import { Feature, hasFeature } from '../pageFeatures'

// 支持的简单页面
type SimplePageId = 'config' | 'login' | 'setup' | 'data-management'

// ==================== 页面初始化 ====================
// 注意：startPage 由 useRouteScheduler 统一调用
// 这些简单页面的 Scheduler 保留为占位符，保持 API 一致性

export function useSimplePageScheduler(_pageId: SimplePageId): void {
  // startPage 由 useRouteScheduler 统一调用
}

// Config 专用
export function useConfigScheduler(): void {
  // startPage('config') 由 useRouteScheduler 统一调用
}

// Login 专用
export function useLoginScheduler(): void {
  // startPage('login') 由 useRouteScheduler 统一调用
}

// Setup 专用
export function useSetupScheduler(): void {
  // startPage('setup') 由 useRouteScheduler 统一调用
}

// DataManagement 专用
export function useDataManagementScheduler(): void {
  // startPage('data-management') 由 useRouteScheduler 统一调用
}

// ==================== Timeout Hooks ====================

/**
 * 简单页面延时器
 */
export function useSimpleTimeout(
  callback: () => void,
  delay: number | null,
  pageId: SimplePageId = 'config',
): void {
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!hasFeature(pageId, Feature.Timeout) || delay === null) {
      return
    }

    const id = setTimeout(() => savedCallback.current(), delay)
    return () => clearTimeout(id)
  }, [delay, pageId])
}

/**
 * 简单页面防抖
 */
export function useSimpleDebounce<T extends (...args: any[]) => void>(
  callback: T,
  delay: number,
  pageId: SimplePageId = 'config',
): T {
  const timeoutRef = useRef<number | null>(null)
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  const debounced = useCallback((...args: any[]) => {
    if (!hasFeature(pageId, Feature.Timeout)) {
      // 功能未启用，直接调用
      savedCallback.current(...args)
      return
    }

    if (timeoutRef.current !== null) {
      clearTimeout(timeoutRef.current)
    }

    timeoutRef.current = window.setTimeout(() => {
      timeoutRef.current = null
      savedCallback.current(...args)
    }, delay)
  }, [delay, pageId]) as T

  useEffect(() => {
    return () => {
      if (timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  return debounced
}

/**
 * 简单页面节流
 */
export function useSimpleThrottle<T extends (...args: any[]) => void>(
  callback: T,
  delay: number,
  pageId: SimplePageId = 'config',
): T {
  const lastRun = useRef(0)
  const timeoutRef = useRef<number | null>(null)
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  const throttled = useCallback((...args: any[]) => {
    if (!hasFeature(pageId, Feature.Timeout)) {
      savedCallback.current(...args)
      return
    }

    const now = Date.now()
    const remaining = delay - (now - lastRun.current)

    if (remaining <= 0) {
      lastRun.current = now
      savedCallback.current(...args)
    }
    else if (timeoutRef.current === null) {
      timeoutRef.current = window.setTimeout(() => {
        lastRun.current = Date.now()
        timeoutRef.current = null
        savedCallback.current(...args)
      }, remaining)
    }
  }, [delay, pageId]) as T

  useEffect(() => {
    return () => {
      if (timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  return throttled
}
