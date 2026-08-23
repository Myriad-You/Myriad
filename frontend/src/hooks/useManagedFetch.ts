/**
 * 受控的 Fetch Hook
 *
 * 特性：
 * 1. 自动取消过期请求
 * 2. 组件卸载时自动清理
 * 3. 并发控制
 * 4. 支持优先级
 */

import { useCallback, useEffect, useRef } from 'react'
import { ApiError } from '../services/api'
import { requestManager } from '../utils/concurrentRequestManager'
import { httpStatusMessage } from '../utils/userFacingError'

interface UseManagedFetchOptions {
  priority?: number
  timeout?: number
  cancelOnUnmount?: boolean
}

export function useManagedFetch() {
  const requestKeysRef = useRef<Set<string>>(new Set())
  const isMountedRef = useRef(true)

  useEffect(() => {
    isMountedRef.current = true

    return () => {
      isMountedRef.current = false
      // 组件卸载时取消所有请求
      requestKeysRef.current.forEach((key) => {
        requestManager.cancelRequest(key)
      })
      requestKeysRef.current.clear()
    }
  }, [])

  /**
   * 执行受控的 fetch 请求
   */
  const fetch = useCallback(
    async <T = any>(
      url: string,
      options: RequestInit = {},
      config: UseManagedFetchOptions & { key?: string } = {},
    ): Promise<T | null> => {
      const key = config.key || `${url}-${Date.now()}`
      requestKeysRef.current.add(key)

      try {
        const result = await requestManager.fetch<T>(
          key,
          async (signal) => {
            const response = await globalThis.fetch(url, {
              ...options,
              signal,
            })

            if (!response.ok) {
              throw new ApiError(
                httpStatusMessage(response.status),
                response.status,
              )
            }

            return response.json()
          },
          config.priority || 0,
          config.timeout,
        )

        // 请求完成后从集合中移除
        requestKeysRef.current.delete(key)

        // 检查组件是否仍然挂载
        if (!isMountedRef.current) {
          return null
        }

        return result
      } catch (error) {
        requestKeysRef.current.delete(key)

        // 如果是取消错误且组件已卸载，静默处理
        if (
          error instanceof Error &&
          error.message.includes('cancelled') &&
          !isMountedRef.current
        ) {
          return null
        }

        throw error
      }
    },
    [],
  )

  /**
   * 取消指定的请求
   */
  const cancelRequest = useCallback((key: string) => {
    requestManager.cancelRequest(key)
    requestKeysRef.current.delete(key)
  }, [])

  /**
   * 取消所有该组件发起的请求
   */
  const cancelAll = useCallback(() => {
    requestKeysRef.current.forEach((key) => {
      requestManager.cancelRequest(key)
    })
    requestKeysRef.current.clear()
  }, [])

  return {
    fetch,
    cancelRequest,
    cancelAll,
  }
}
