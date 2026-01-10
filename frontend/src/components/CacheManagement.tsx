/**
 * 缓存管理面板组件
 *
 * 功能：
 * 1. 显示所有平台缓存状态
 * 2. 显示缓存大小和修改时间
 * 3. 清除单个或批量缓存
 * 4. 触发后台处理任务
 */

import { FaClock, FaExclamationCircle, FaHdd, FaSyncAlt, FaTrash } from '@lib/icons'
import React, { useEffect, useState } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { useBackgroundTasks } from '../hooks/useBackgroundTasks'
import { TaskStatus } from './TaskStatus'

interface CacheInfo {
  platform: string
  exists: boolean
  size_bytes?: number
  modified_at?: string
  path: string
}

export function CacheManagement() {
  const [caches, setCaches] = useState<CacheInfo[]>([])
  const [totalSize, setTotalSize] = useState<string>('0.00')
  const [isLoading, setIsLoading] = useState(true)
  const [activeTask, setActiveTask] = useState<string | null>(null)
  const [processingPlatform, setProcessingPlatform] = useState<string | null>(null)
  const { t, locale } = useI18n()

  const {
    getCacheStatus,
    clearPlatformCache,
    clearAllCaches,
    submitTask,
    isSubmitting,
  } = useBackgroundTasks()

  const loadCacheStatus = async () => {
    setIsLoading(true)
    const status = await getCacheStatus()

    if (status && status.caches) {
      setCaches(status.caches)
      setTotalSize(status.total_size_mb || '0.00')
    }

    setIsLoading(false)
  }

  useEffect(() => {
    loadCacheStatus()
  }, [])

  const handleClearCache = async (platform: string) => {
    if (!confirm(t.cache.clearPlatformConfirm.replace('{platform}', platform))) {
      return
    }

    const success = await clearPlatformCache(platform)

    if (success) {
      await loadCacheStatus()
    }
    else {
      alert(t.cache.clearFailed.replace('{platform}', platform))
    }
  }

  const handleClearAll = async () => {
    if (!confirm(t.cache.clearAllConfirm)) {
      return
    }

    const success = await clearAllCaches()

    if (success) {
      await loadCacheStatus()
    }
    else {
      alert(t.cache.clearFailed)
    }
  }

  const handleProcessPlatform = async (platform: string) => {
    const taskId = await submitTask(platform)

    if (taskId) {
      setActiveTask(taskId)
      setProcessingPlatform(platform)
    }
    else {
      alert(t.cache.submitTaskFailed.replace('{platform}', platform))
    }
  }

  const handleTaskComplete = () => {
    setActiveTask(null)
    setProcessingPlatform(null)
    loadCacheStatus() // 重新加载缓存状态
  }

  const handleTaskClose = () => {
    setActiveTask(null)
    setProcessingPlatform(null)
  }

  const formatSize = (bytes?: number): string => {
    if (!bytes)
      return '-'
    const mb = bytes / 1024 / 1024
    return `${mb.toFixed(2)} MB`
  }

  const formatDate = (dateString?: string): string => {
    if (!dateString)
      return '-'
    return new Date(dateString).toLocaleString(locale)
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <FaSyncAlt className="w-6 h-6 animate-spin text-gray-400" />
        <span className="ml-2 text-gray-600 dark:text-gray-400">{t.cache.loading}</span>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* 任务状态显示 */}
      {activeTask && (
        <div className="mb-4">
          <TaskStatus
            taskId={activeTask}
            onComplete={handleTaskComplete}
            onError={handleTaskComplete}
            onClose={handleTaskClose}
            autoClose={true}
            autoCloseDelay={3000}
          />
        </div>
      )}

      {/* 总览 */}
      <div className="bg-white dark:bg-neutral-900 rounded-lg shadow-sm border border-gray-200 dark:border-neutral-700 p-6">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-xl font-bold text-gray-900 dark:text-gray-100">{t.cache.title}</h2>
          <div className="flex items-center gap-4">
            <div className="text-sm text-gray-600 dark:text-gray-400">
              {t.cache.totalSize}
              :
              <span className="font-semibold text-gray-900 dark:text-gray-100">
                {totalSize}
                {' '}
                MB
              </span>
            </div>
            <button
              onClick={loadCacheStatus}
              className="p-2 text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-gray-100 transition-colors"
              title={t.common.refresh}
            >
              <FaSyncAlt className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* 批量操作 */}
        <div className="flex gap-2 mb-6">
          <button
            onClick={handleClearAll}
            className="px-4 py-2 bg-red-500 hover:bg-red-600 text-white rounded-lg transition-colors flex items-center gap-2"
          >
            <FaTrash className="w-4 h-4" />
            {t.cache.clearAll}
          </button>
        </div>

        {/* 缓存列表 */}
        <div className="space-y-3">
          {caches.map(cache => (
            <div
              key={cache.platform}
              className={`border rounded-lg p-4 transition-all ${
                cache.exists
                  ? 'border-gray-300 dark:border-neutral-600 bg-gray-50 dark:bg-neutral-800/50'
                  : 'border-gray-200 dark:border-neutral-700 bg-white dark:bg-neutral-900'
              }`}
            >
              <div className="flex items-start justify-between">
                <div className="flex-1">
                  <div className="flex items-center gap-3 mb-2">
                    <h3 className="font-medium text-gray-900 dark:text-gray-100 capitalize">
                      {cache.platform}
                    </h3>
                    {cache.exists
                      ? (
                          <span className="px-2 py-1 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 text-xs rounded-full">
                            {t.cache.cached}
                          </span>
                        )
                      : (
                          <span className="px-2 py-1 bg-gray-100 dark:bg-neutral-800 text-gray-600 dark:text-gray-400 text-xs rounded-full">
                            {t.cache.notCached}
                          </span>
                        )}
                  </div>

                  {cache.exists && (
                    <div className="space-y-1 text-sm text-gray-600 dark:text-gray-400">
                      <div className="flex items-center gap-2">
                        <FaHdd className="w-4 h-4" />
                        <span>
                          {t.cache.size}
                          :
                          {' '}
                          {formatSize(cache.size_bytes)}
                        </span>
                      </div>
                      <div className="flex items-center gap-2">
                        <FaClock className="w-4 h-4" />
                        <span>
                          {t.cache.modifiedTime}
                          :
                          {' '}
                          {formatDate(cache.modified_at)}
                        </span>
                      </div>
                    </div>
                  )}

                  {!cache.exists && (
                    <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400">
                      <FaExclamationCircle className="w-4 h-4" />
                      <span>{t.cache.notProcessed}</span>
                    </div>
                  )}
                </div>

                <div className="flex items-center gap-2">
                  {cache.exists && (
                    <button
                      onClick={() => handleClearCache(cache.platform)}
                      className="p-2 text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors"
                      title={t.cache.clearCache}
                    >
                      <FaTrash className="w-4 h-4" />
                    </button>
                  )}
                  <button
                    onClick={() => handleProcessPlatform(cache.platform)}
                    disabled={isSubmitting || processingPlatform === cache.platform}
                    className="px-3 py-2 bg-blue-500 hover:bg-blue-600 disabled:bg-gray-300 dark:disabled:bg-gray-700 text-white rounded-lg transition-colors flex items-center gap-2 text-sm"
                    title={cache.exists ? t.cache.reprocess : t.cache.process}
                  >
                    <FaSyncAlt
                      className={`w-4 h-4 ${
                        processingPlatform === cache.platform ? 'animate-spin' : ''
                      }`}
                    />
                    {cache.exists ? t.cache.reprocess : t.cache.process}
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* 说明 */}
      <div className="bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-lg p-4">
        <div className="flex items-start gap-3">
          <FaExclamationCircle className="w-5 h-5 text-blue-500 flex-shrink-0 mt-0.5" />
          <div className="text-sm text-blue-900 dark:text-blue-100">
            <p className="font-medium mb-2">{t.cache.aboutCaching}</p>
            <ul className="space-y-1 text-blue-800 dark:text-blue-200">
              <li>
                •
                {t.cache.cacheHint1}
              </li>
              <li>
                •
                {t.cache.cacheHint2}
              </li>
              <li>
                •
                {t.cache.cacheHint3}
              </li>
              <li>
                •
                {t.cache.cacheHint4}
              </li>
            </ul>
          </div>
        </div>
      </div>
    </div>
  )
}
