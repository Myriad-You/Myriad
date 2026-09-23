import type { Task } from '../services/platformTasksApi'
import {
  FaCheckCircle,
  FaExclamationCircle,
  FaSpinner,
  FaTimes,
} from '@lib/icons'
import { useEffect, useRef, useState } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { getTask } from '../services/platformTasksApi'
import { reportUserFacingError } from '../utils/reportError'
import { userFacingError } from '../utils/userFacingError'
import { Spinner } from './Spinner'

export type { Task }

interface TaskStatusProps {
  taskId: string
  onComplete?: (task: Task) => void
  onError?: (task: Task) => void
  onClose?: () => void
  autoClose?: boolean
  autoCloseDelay?: number
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
  const [error, setError] = useState<string | null>(null)
  const { t, locale } = useI18n()
  const latest = useRef({
    onComplete, onError, onClose, autoClose, autoCloseDelay,
    fetchFailed: t.task.fetchFailed,
  })

  useEffect(() => {
    latest.current = {
      onComplete, onError, onClose, autoClose, autoCloseDelay,
      fetchFailed: t.task.fetchFailed,
    }
  }, [onComplete, onError, onClose, autoClose, autoCloseDelay, t.task.fetchFailed])

  useEffect(() => {
    const controller = new AbortController()
    let pollCount = 0
    let currentTask: Task | null = null
    let timer: ReturnType<typeof setTimeout> | undefined
    let closeTimer: ReturnType<typeof setTimeout> | undefined
    setTask(null)
    setError(null)

    const getPollingInterval = () => {
      if (currentTask?.status === 'Processing') {
        if (currentTask.progress < 10) return 1000
        if (currentTask.progress < 50) return 1500
        if (currentTask.progress < 90) return 2000
        return 1000
      }
      if (currentTask?.status === 'Pending') {
        if (pollCount < 5) return 1000
        if (pollCount < 15) return 2000
        return 3000
      }
      return 1000
    }

    const fetchTaskStatus = async () => {
      try {
        const data = await getTask(taskId, controller.signal)
        if (controller.signal.aborted) return

        if (data) {
          if (!data.success || !data.task) {
            throw new Error(data.error || latest.current.fetchFailed)
          }
          currentTask = data.task
          setTask(currentTask)
          pollCount++

          if (currentTask.status === 'Completed') {
            if (latest.current.autoClose) {
              closeTimer = setTimeout(() => {
                if (!controller.signal.aborted) latest.current.onClose?.()
              }, latest.current.autoCloseDelay)
            }
            latest.current.onComplete?.(currentTask)
            return
          }
          if (currentTask.status === 'Failed') {
            latest.current.onError?.(currentTask)
            return
          }
        }
      } catch (err) {
        if (controller.signal.aborted) return
        console.error('Error fetching task status:', err)
        setError(userFacingError(err, latest.current.fetchFailed))
        return
      }
      // Schedule only after settlement so slow requests never overlap.
      if (!controller.signal.aborted) timer = setTimeout(fetchTaskStatus, getPollingInterval())
    }

    void fetchTaskStatus()
    return () => {
      controller.abort()
      clearTimeout(timer)
      clearTimeout(closeTimer)
    }
  }, [taskId])

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
                {reportUserFacingError(
                  task.error,
                  t.reportsPage.generateNeedData,
                  t.reportsPage,
                )}
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
