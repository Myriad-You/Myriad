/**
 * 后台任务状态显示组件
 *
 * 功能：
 * 1. 实时轮询任务状态
 * 2. 显示处理进度条
 * 3. 错误提示
 * 4. 完成通知
 */

import {
  FaCheckCircle,
  FaExclamationCircle,
  FaSpinner,
  FaTimes,
} from '@lib/icons'
import { useCallback, useEffect, useState } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { useManagedFetch } from '../hooks/useManagedFetch'
import { Spinner } from './Spinner'

export interface Task {
  id: string
  platform: string
  status: 'Pending' | 'Processing' | 'Completed' | 'Failed'
  progress: number
  error?: string
  created_at: string
  updated_at: string
  completed_at?: string
}

interface TaskStatusProps {
  taskId: string
  onComplete?: (task: Task) => void
  onError?: (task: Task) => void
  onClose?: () => void
  autoClose?: boolean // 完成后自动关闭
  autoCloseDelay?: number // 自动关闭延迟（毫秒）
}

export function TaskStatus({
  taskId,
  onComplete,
  onError,
  onClose,
  autoClose = true,
  autoCloseDelay = 3000,
}: TaskStatusProps) {
  const [task, setTask] = useState<Task | null>(null)
  const [isPolling, setIsPolling] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [pollCount, setPollCount] = useState(0)
  const { fetch: managedFetch } = useManagedFetch()
  const { t, locale } = useI18n()

  const fetchTaskStatus = useCallback(async () => {
    try {
      const data = await managedFetch<{
        success: boolean
        task?: Task
        error?: string
      }>(
        `/api/tasks/${taskId}`,
        {
          credentials: 'include',
        },
        {
          key: `task-status-${taskId}`,
          priority: 1, // 任务状态查询有较高优先级
        },
      )

      if (!data) {
        // 请求被取消或组件已卸载
        return
      }

      if (data.success && data.task) {
        const updatedTask = data.task as Task
        setTask(updatedTask)
        setPollCount((prev) => prev + 1)

        // 任务完成或失败时停止轮询
        if (updatedTask.status === 'Completed') {
          setIsPolling(false)
          onComplete?.(updatedTask)

          if (autoClose) {
            setTimeout(() => {
              onClose?.()
            }, autoCloseDelay)
          }
        } else if (updatedTask.status === 'Failed') {
          setIsPolling(false)
          onError?.(updatedTask)
        }
      } else {
        throw new Error(data.error || 'Failed to fetch task status')
      }
    } catch (err) {
      // 静默处理取消错误
      if (err instanceof Error && err.message.includes('cancelled')) {
        return
      }

      console.error('Error fetching task status:', err)
      setError(err instanceof Error ? err.message : 'Unknown error')
      setIsPolling(false)
    }
  }, [
    taskId,
    onComplete,
    onError,
    onClose,
    autoClose,
    autoCloseDelay,
    managedFetch,
  ])

  // 智能轮询间隔：根据轮询次数和任务状态动态调整
  const getPollingInterval = useCallback(() => {
    if (!task) return 1000 // 初始：1秒

    // 根据任务状态调整
    if (task.status === 'Processing') {
      // 处理中：根据进度调整频率
      if (task.progress < 10) return 1000 // 刚开始：1秒
      if (task.progress < 50) return 1500 // 进行中：1.5秒
      if (task.progress < 90) return 2000 // 快完成：2秒
      return 1000 // 即将完成：1秒（加快检测）
    } else if (task.status === 'Pending') {
      // 等待中：逐渐降低频率避免过多请求
      if (pollCount < 5) return 1000 // 前5次：1秒
      if (pollCount < 15) return 2000 // 6-15次：2秒
      return 3000 // 15次后：3秒
    }

    return 1000 // 默认1秒
  }, [task, pollCount])

  useEffect(() => {
    // 立即执行一次
    fetchTaskStatus()

    if (!isPolling) return

    // 使用动态间隔轮询
    const interval = setInterval(fetchTaskStatus, getPollingInterval())

    return () => clearInterval(interval)
  }, [fetchTaskStatus, isPolling, getPollingInterval])

  if (error) {
    return (
      <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-4">
        <div className="flex items-start justify-between">
          <div className="flex items-start gap-3">
            <FaExclamationCircle className="w-5 h-5 text-red-500 shrink-0 mt-0.5" />
            <div>
              <h3 className="font-medium text-red-900 dark:text-red-100">
                {t.task.fetchFailed}
              </h3>
              <p className="text-sm text-red-700 dark:text-red-300 mt-1">
                {error}
              </p>
            </div>
          </div>
          {onClose && (
            <button
              onClick={onClose}
              className="text-red-400 hover:text-red-600 transition-colors"
              aria-label={t.task.closeError}
              title={t.common.close}
            >
              <FaTimes className="w-5 h-5" />
            </button>
          )}
        </div>
      </div>
    )
  }

  if (!task) {
    return (
      <div className="bg-gray-50 dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-lg p-4">
        <div className="flex items-center justify-center py-1">
          <Spinner size="sm" />
        </div>
      </div>
    )
  }

  const statusConfig = {
    Pending: {
      icon: FaSpinner,
      color: 'text-blue-500',
      bg: 'bg-blue-50 dark:bg-blue-900/20',
      border: 'border-blue-200 dark:border-blue-800',
      label: t.task.pending,
    },
    Processing: {
      icon: FaSpinner,
      color: 'text-yellow-500',
      bg: 'bg-yellow-50 dark:bg-yellow-900/20',
      border: 'border-yellow-200 dark:border-yellow-800',
      label: t.task.processing,
    },
    Completed: {
      icon: FaCheckCircle,
      color: 'text-green-500',
      bg: 'bg-green-50 dark:bg-green-900/20',
      border: 'border-green-200 dark:border-green-800',
      label: t.task.completed,
    },
    Failed: {
      icon: FaExclamationCircle,
      color: 'text-red-500',
      bg: 'bg-red-50 dark:bg-red-900/20',
      border: 'border-red-200 dark:border-red-800',
      label: t.task.failed,
    },
  }

  const config = statusConfig[task.status]
  const Icon = config.icon
  const shouldAnimate =
    task.status === 'Pending' || task.status === 'Processing'

  return (
    <div
      className={`${config.bg} border ${config.border} rounded-lg p-4 transition-all`}
    >
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-start gap-3">
          {shouldAnimate ? (
            <Spinner
              size="sm"
              color="current"
              className={`${config.color} shrink-0 mt-0.5`}
            />
          ) : (
            <Icon className={`w-5 h-5 ${config.color} shrink-0 mt-0.5`} />
          )}
          <div>
            <h3 className="font-medium text-gray-900 dark:text-gray-100">
              {task.platform} -{config.label}
            </h3>
            {task.error && (
              <p className="text-sm text-red-600 dark:text-red-400 mt-1">
                {task.error}
              </p>
            )}
          </div>
        </div>
        {onClose &&
          (task.status === 'Completed' || task.status === 'Failed') && (
            <button
              onClick={onClose}
              className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
              aria-label={t.task.closeTask}
              title={t.common.close}
            >
              <FaTimes className="w-5 h-5" />
            </button>
          )}
      </div>

      {/* 进度条 */}
      {(task.status === 'Processing' || task.status === 'Pending') && (
        <div className="space-y-1">
          <div className="flex items-center justify-between text-xs text-gray-600 dark:text-gray-400">
            <span>{t.task.progress}</span>
            <span>{task.progress.toFixed(0)}%</span>
          </div>
          <div className="w-full bg-gray-200 dark:bg-neutral-800 rounded-full h-2 overflow-hidden">
            <div
              className="bg-blue-500 h-full rounded-full transition-all duration-300 ease-out"
              style={{ width: `${task.progress}%` }}
            />
          </div>
        </div>
      )}

      {/* 时间信息 */}
      <div className="mt-3 text-xs text-gray-500 dark:text-gray-400 space-y-1">
        <div>
          {t.task.createdTime}:{' '}
          {new Date(task.created_at).toLocaleString(locale)}
        </div>
        {task.completed_at && (
          <div>
            {t.task.completedTime}:{' '}
            {new Date(task.completed_at).toLocaleString(locale)}
          </div>
        )}
      </div>
    </div>
  )
}
