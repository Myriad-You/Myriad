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
      // 组件卸载时取消所有请求。
      isMountedRef.current = false

      requestKeysRef.current.forEach((key) => {
        requestManager.cancelRequest(key)
      })
      requestKeysRef.current.clear()
    }
  }, [])

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

        requestKeysRef.current.delete(key)

        if (!isMountedRef.current) {
          return null
        }

        return result
      } catch (error) {
        requestKeysRef.current.delete(key)

        if (
          error instanceof Error &&
          // 取消错误且组件已卸载则静默。
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

  const cancelRequest = useCallback((key: string) => {
    requestManager.cancelRequest(key)
    requestKeysRef.current.delete(key)
  }, [])

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
