import type { ToastType } from '../Toast'

import type { PlatformDataPreviewHandle } from './PlatformDataPreview'
import { FaSyncAlt, FaTrash } from '@lib/icons'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { useBackgroundTasks } from '../../hooks/useBackgroundTasks'
import { fetchJson } from '../../utils/apiHelper'
import { getCSRFToken } from '../../utils/csrf'
import { resolvePlatformId } from '../../utils/platformId'
import { notifyRecentActivityUpdated } from '../../utils/recentActivity'
import { userFacingError } from '../../utils/userFacingError'
import { ButtonItem, SettingGroup, useSettingGuide } from '../settings'
import { TaskStatus } from '../TaskStatus'
import PlatformDataPreview from './PlatformDataPreview'

interface CacheInfo {
  platform: string
  exists: boolean
  size_bytes?: number
  modified_at?: string
  path: string
}

interface PlatformStatus {
  hasRawData: boolean
  rawDataSize: number
  rawFetchedAt: string | null
}

interface PlatformMetadataStatusResponse {
  success: boolean
  platform: string
  has_raw_data: boolean
  raw_data_size: number
  raw_fetched_at: string | null
}

export interface PlatformDataManagementProps {
  platformName: string
  showMessage: (
    message: string,
    type?: ToastType,
    duration?: number,
  ) => void
}

export default function PlatformDataManagement({
  platformName,
  showMessage,
}: PlatformDataManagementProps) {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const platformId = useMemo(
    () => resolvePlatformId(platformName),
    [platformName],
  )
  const [status, setStatus] = useState<PlatformStatus | null>(null)
  const [cache, setCache] = useState<CacheInfo | null>(null)
  const [statusLoading, setStatusLoading] = useState(false)
  const [rawStatusError, setRawStatusError] = useState(false)
  const [cacheStatusError, setCacheStatusError] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [processing, setProcessing] = useState(false)
  const [clearing, setClearing] = useState(false)
  const [activeTask, setActiveTask] = useState<string | null>(null)
  const { clearPlatformCache, getPlatformCacheStatus, submitTask } =
    useBackgroundTasks()
  const loadRequestRef = useRef(0)
  const previewRef = useRef<PlatformDataPreviewHandle>(null)

  const loadStatus = useCallback(async () => {
    if (!platformId) return

    const requestId = ++loadRequestRef.current
    setStatusLoading(true)
    setRawStatusError(false)
    setCacheStatusError(false)

    const [rawResult, cacheResult] = await Promise.allSettled([
      fetchJson<PlatformMetadataStatusResponse>(
        `${API_URL}/api/profile/metadata/status/${encodeURIComponent(platformId)}`,
        undefined,
        'Unable to load platform data status',
      ),
      getPlatformCacheStatus(platformId),
    ])

    if (requestId !== loadRequestRef.current) return

    const rawFailed =
      rawResult.status === 'rejected' || !rawResult.value.success
    const cacheFailed =
      cacheResult.status === 'rejected' || cacheResult.value === null

    if (rawFailed) {
      console.error(
        `Failed to load ${platformName} raw data status:`,
        rawResult.status === 'rejected' ? rawResult.reason : rawResult.value,
      )
      setRawStatusError(true)
    } else {
      setStatus({
        hasRawData: rawResult.value.has_raw_data,
        rawDataSize: rawResult.value.raw_data_size,
        rawFetchedAt: rawResult.value.raw_fetched_at,
      })
    }

    if (cacheFailed) {
      console.error(
        `Failed to load ${platformName} cache status:`,
        cacheResult.status === 'rejected'
          ? cacheResult.reason
          : 'empty cache status response',
      )
      setCacheStatusError(true)
    } else {
      setCache(cacheResult.value)
    }

    // 单项失败时保留另一项的有效状态并在对应行显示“暂时不可用”；
    // 只有整块都无法读取时才弹出全局失败提示。
    if (rawFailed && cacheFailed) {
      showMessage(t.dataManagement.loadStatusFailed, 'error')
    }
    if (requestId === loadRequestRef.current) {
      setStatusLoading(false)
    }
  }, [
    getPlatformCacheStatus,
    platformId,
    platformName,
    showMessage,
    t.dataManagement.loadStatusFailed,
  ])

  useEffect(() => {
    setStatus(null)
    setCache(null)
    setActiveTask(null)
    setProcessing(false)
    void loadStatus()
    return () => {
      loadRequestRef.current += 1
    }
  }, [loadStatus])

  const refreshPlatform = async () => {
    if (!platformId) return
    if (
      !window.confirm(
        t.dataManagement.confirmRefreshData.replace(
          '{platform}',
          platformName,
        ),
      )
    ) {
      return
    }

    setRefreshing(true)
    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) {
        showMessage(t.dataManagement.csrfTokenError, 'error', 5000)
        return
      }

      const response = await fetch(`${API_URL}/api/profile/fetch-platform`, {
        method: 'POST',
        credentials: 'include',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        body: JSON.stringify({ platform: platformId }),
      })
      const data = await response.json()

      if (!response.ok || !data.success) {
        throw new Error(
          data.message ||
            t.errors.httpStatus.replace('{status}', String(response.status)),
        )
      }

      notifyRecentActivityUpdated()
      showMessage(
        t.dataManagement.dataRefreshed.replace('{platform}', platformName),
        'success',
        5000,
      )
      await loadStatus()
      // 原始数据已更新，智能过滤可能仍是旧的；仍刷新预览以同步时间戳/状态
      void previewRef.current?.reload()
    } catch (error) {
      showMessage(
        userFacingError(error, t.dataManagement.refreshFailed),
        'error',
        5000,
      )
    } finally {
      setRefreshing(false)
    }
  }

  const processPlatform = async () => {
    if (!platformId) return

    const taskId = await submitTask(platformId)
    if (!taskId) {
      showMessage(
        t.dataManagement.submitTaskFailed.replace(
          '{platform}',
          platformName,
        ),
        'error',
      )
      return
    }

    setActiveTask(taskId)
    setProcessing(true)
  }

  const clearCache = async () => {
    if (!platformId) return
    if (
      !window.confirm(
        t.dataManagement.confirmClearCache.replace(
          '{platform}',
          platformName,
        ),
      )
    ) {
      return
    }

    setClearing(true)
    try {
      const success = await clearPlatformCache(platformId)
      if (!success) {
        showMessage(
          t.dataManagement.clearCacheFailed.replace(
            '{platform}',
            platformName,
          ),
          'error',
        )
        return
      }

      showMessage(
        t.dataManagement.cacheCleared.replace('{platform}', platformName),
        'success',
      )
      await loadStatus()
      void previewRef.current?.reload()
    } finally {
      setClearing(false)
    }
  }

  const handleTaskComplete = () => {
    setActiveTask(null)
    setProcessing(false)
    void loadStatus()
    void previewRef.current?.reload()
  }

  const handleTaskClose = () => {
    setActiveTask(null)
    setProcessing(false)
  }

  const formatBytes = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(2)} KB`
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
  }

  const formatDateTime = (date: string | null | undefined): string => {
    if (!date) return t.dataManagement.unknown
    return new Date(date).toLocaleString()
  }

  if (!platformId) return null

  const hasCache = Boolean(cache?.exists)
  const rawStatusText = rawStatusError
    ? t.dataManagement.statusUnavailable
    : statusLoading && !status
      ? t.common.loading
      : status?.hasRawData
        ? `${formatBytes(status.rawDataSize)} · ${formatDateTime(
            status.rawFetchedAt,
          )}`
        : t.dataManagement.noData
  const cacheStatusText = cacheStatusError
    ? t.dataManagement.statusUnavailable
    : statusLoading && !cache
      ? t.common.loading
      : hasCache
        ? `${formatBytes(cache?.size_bytes ?? 0)} · ${formatDateTime(
            cache?.modified_at,
          )}`
        : t.dataManagement.noCache

  return (
    <>
      <PlatformDataPreview ref={previewRef} platformName={platformName} />

      <SettingGroup
        title={t.dataManagement.dataManagementTitle}
        detail={t.dataManagement.dataManagementDesc}
        {...bindGuide('platforms.dataManagement', g.platforms.dataManagement)}
        className="platform-data-management"
      >
        {activeTask ? (
          <div className="platform-data-management-task">
            <TaskStatus
              taskId={activeTask}
              onComplete={handleTaskComplete}
              onClose={handleTaskClose}
            />
          </div>
        ) : null}

        <ButtonItem
          itemKey="platform-raw-refresh"
          label={t.dataManagement.rawData}
          description={rawStatusText}
          {...bindGuide('platforms.dataRefresh', g.platforms.dataRefresh)}
          buttonText={
            refreshing ? t.dataManagement.refreshing : t.dataManagement.refresh
          }
          buttonIcon={<FaSyncAlt />}
          variant="secondary"
          layout="horizontal"
          size="sm"
          loading={refreshing}
          disabled={refreshing || statusLoading}
          onClick={() => void refreshPlatform()}
        />

        <ButtonItem
          itemKey="platform-filter-process"
          label={t.dataManagement.smartFilter}
          description={cacheStatusText}
          {...bindGuide('platforms.dataReprocess', g.platforms.dataReprocess)}
          buttonText={
            processing ? t.dataManagement.processing : t.dataManagement.process
          }
          variant="primary"
          layout="horizontal"
          size="sm"
          loading={processing}
          disabled={processing || !status?.hasRawData || statusLoading}
          onClick={() => void processPlatform()}
        />

        <ButtonItem
          itemKey="platform-filter-clear"
          label={t.dataManagement.clearCache}
          description={cacheStatusText}
          {...bindGuide('platforms.dataClearCache', g.platforms.dataClearCache)}
          buttonText={
            clearing ? t.dataManagement.clearing : t.dataManagement.clear
          }
          buttonIcon={<FaTrash />}
          variant="danger"
          layout="horizontal"
          size="sm"
          loading={clearing}
          disabled={clearing || statusLoading || !hasCache}
          onClick={() => void clearCache()}
        />
      </SettingGroup>
    </>
  )
}
