/**
 * 后台任务管理 Hook
 *
 * 提供任务提交、状态查询、批量管理等功能
 */

import type { Task } from '../components/TaskStatus'
import { useCallback, useState } from 'react'
import { getCSRFToken } from '../utils/csrf'

interface SubmitTaskResponse {
  success: boolean
  task_id?: string
  platform?: string
  status?: string
  message?: string
  error?: string
}

interface TaskStatusResponse {
  success: boolean
  task?: Task
  error?: string
}

interface CacheInfo {
  platform: string
  exists: boolean
  size_bytes?: number
  modified_at?: string
  path: string
}

interface CacheStatusResponse {
  success: boolean
  caches?: CacheInfo[]
  cache?: CacheInfo
  total_size_bytes?: number
  total_size_mb?: string
  error?: string
}

export function useBackgroundTasks() {
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  /**
   * 提交平台数据处理任务
   */
  const submitTask = useCallback(async (platform: string): Promise<string | null> => {
    setIsSubmitting(true)
    setError(null)

    try {
      const csrfToken = await getCSRFToken()
      if (!csrfToken) {
        throw new Error('无法获取 CSRF Token')
      }

      const response = await fetch('/api/tasks', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({ platform }),
      })

      const data: SubmitTaskResponse = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(data.error || `HTTP ${response.status}`)
      }

      return data.task_id || null
    }
    catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to submit task'
      setError(errorMessage)
      console.error('Error submitting task:', err)
      return null
    }
    finally {
      setIsSubmitting(false)
    }
  }, [])

  /**
   * 获取任务状态
   */
  const getTaskStatus = useCallback(async (taskId: string): Promise<Task | null> => {
    try {
      const response = await fetch(`/api/tasks/${taskId}`, {
        credentials: 'include',
      })

      const data: TaskStatusResponse = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(data.error || `HTTP ${response.status}`)
      }

      return data.task || null
    }
    catch (err) {
      console.error('Error fetching task status:', err)
      return null
    }
  }, [])

  /**
   * 获取平台当前任务
   */
  const getPlatformTask = useCallback(async (platform: string): Promise<Task | null> => {
    try {
      const response = await fetch(`/api/tasks/platform/${platform}`, {
        credentials: 'include',
      })

      const data: TaskStatusResponse = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(data.error || `HTTP ${response.status}`)
      }

      return data.task || null
    }
    catch (err) {
      console.error('Error fetching platform task:', err)
      return null
    }
  }, [])

  /**
   * 获取缓存状态
   */
  const getCacheStatus = useCallback(async (): Promise<CacheStatusResponse | null> => {
    try {
      const response = await fetch('/api/cache/status', {
        credentials: 'include',
      })

      const data: CacheStatusResponse = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(data.error || `HTTP ${response.status}`)
      }

      return data
    }
    catch (err) {
      console.error('Error fetching cache status:', err)
      return null
    }
  }, [])

  /**
   * 获取平台缓存状态
   */
  const getPlatformCacheStatus = useCallback(async (platform: string): Promise<CacheInfo | null> => {
    try {
      const response = await fetch(`/api/cache/status/${platform}`, {
        credentials: 'include',
      })

      const data: CacheStatusResponse = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(data.error || `HTTP ${response.status}`)
      }

      return data.cache || null
    }
    catch (err) {
      console.error('Error fetching platform cache status:', err)
      return null
    }
  }, [])

  /**
   * 清除平台缓存
   */
  const clearPlatformCache = useCallback(async (platform: string): Promise<boolean> => {
    try {
      const csrfToken = await getCSRFToken()
      if (!csrfToken) {
        console.error('Failed to get CSRF token')
        return false
      }

      const response = await fetch(`/api/cache/${platform}`, {
        method: 'DELETE',
        headers: {
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
      })

      const data = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(data.error || `HTTP ${response.status}`)
      }

      return true
    }
    catch (err) {
      console.error('Error clearing platform cache:', err)
      return false
    }
  }, [])

  /**
   * 批量清除缓存
   */
  const clearCaches = useCallback(async (platforms?: string[]): Promise<boolean> => {
    try {
      const csrfToken = await getCSRFToken()
      if (!csrfToken) {
        console.error('Failed to get CSRF token')
        return false
      }

      const response = await fetch('/api/cache/clear', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({ platforms }),
      })

      const data = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(data.error || `HTTP ${response.status}`)
      }

      return true
    }
    catch (err) {
      console.error('Error clearing caches:', err)
      return false
    }
  }, [])

  /**
   * 清除所有缓存
   */
  const clearAllCaches = useCallback(async (): Promise<boolean> => {
    try {
      const csrfToken = await getCSRFToken()
      if (!csrfToken) {
        console.error('Failed to get CSRF token')
        return false
      }

      const response = await fetch('/api/cache/all', {
        method: 'DELETE',
        headers: {
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
      })

      const data = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(data.error || `HTTP ${response.status}`)
      }

      return true
    }
    catch (err) {
      console.error('Error clearing all caches:', err)
      return false
    }
  }, [])

  return {
    // 任务管理
    submitTask,
    getTaskStatus,
    getPlatformTask,
    isSubmitting,

    // 缓存管理
    getCacheStatus,
    getPlatformCacheStatus,
    clearPlatformCache,
    clearCaches,
    clearAllCaches,

    // 错误状态
    error,
    clearError: () => setError(null),
  }
}
