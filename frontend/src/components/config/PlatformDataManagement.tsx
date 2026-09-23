import type { CacheInfo } from '../../services/platformTasksApi'
import type { ToastType } from '../Toast'

import type { PlatformDataPreviewHandle } from './PlatformDataPreview'
import { FaSyncAlt, FaTrash } from '@lib/icons'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  clearPlatformCache,
  fetchPlatformData,
  getPlatformCacheStatus,
  getPlatformMetadataStatus,
  submitPlatformTask,
} from '../../services/platformTasksApi'
import { resolvePlatformId } from '../../utils/platformId'
import { platformFetchDetails, refreshPlatformViews } from '../../utils/platformRefresh'
import { notifyRecentActivityUpdated } from '../../utils/recentActivity'
import { userFacingError } from '../../utils/userFacingError'
import { ButtonItem, SettingGroup, useSettingGuide } from '../settings'
import { TaskStatus } from '../TaskStatus'
import PlatformDataPreview from './PlatformDataPreview'

interface PlatformStatus {
  hasRawData: boolean
  rawDataSize: number
  rawFetchedAt: string | null
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
  const { t, format, locale } = useI18n()
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
  const [fetchResult, setFetchResult] = useState<{ partial: boolean; issues: unknown; message?: unknown } | null>(null)
  const fetchDetails = platformFetchDetails(fetchResult?.issues, t.dataManagement.fetchResult) || userFacingError(fetchResult?.message, t.dataManagement.refreshFailed)
  const fetchMessage = fetchResult ? `${fetchResult.partial ? t.dataManagement.fetchResult.partial : t.dataManagement.fetchResult.failed}\n${fetchDetails}` : ''
  const [refreshing, setRefreshing] = useState(false)
  const [processing, setProcessing] = useState(false)
  const [clearing, setClearing] = useState(false)
  const [activeTask, setActiveTask] = useState<string | null>(null)
  const loadRequestRef = useRef(0)
  const previewRef = useRef<PlatformDataPreviewHandle>(null)

  const loadStatus = useCallback(async () => {
    if (!platformId) return

    const requestId = ++loadRequestRef.current
    setStatusLoading(true)
    setRawStatusError(false)
    setCacheStatusError(false)

    const [rawResult, cacheResult] = await Promise.allSettled([
      getPlatformMetadataStatus(platformId),
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

    // one failure: keep the other; global error only if both fail
    if (rawFailed && cacheFailed) {
      showMessage(t.dataManagement.loadStatusFailed, 'error')
    }
    if (requestId === loadRequestRef.current) {
      setStatusLoading(false)
    }
  }, [
    platformId,
    platformName,
    showMessage,
    t.dataManagement.loadStatusFailed,
  ])

  useEffect(() => { setFetchResult(null) }, [platformId])

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
        format(t.dataManagement.confirmRefreshData, {
          platform: platformName,
        }),
      )
    ) {
      return
    }

    setRefreshing(true)
    setFetchResult(null)
    try {
      const data = await fetchPlatformData(platformId)

      // Both full and partial fetches may have updated persisted data.
      await refreshPlatformViews(loadStatus, async () => previewRef.current?.reload())
      notifyRecentActivityUpdated()
      const details = platformFetchDetails(data.issues, t.dataManagement.fetchResult)
      if (!data.success) {
        setFetchResult({ partial: Boolean(data.partial), issues: data.issues, message: data.message })
        const prefix = data.partial ? t.dataManagement.fetchResult.partial : t.dataManagement.fetchResult.failed
        showMessage(`${prefix}\n${details || userFacingError(data.message, t.dataManagement.refreshFailed)}`, data.partial ? 'warning' : 'error', 5000)
        return
      }
      showMessage(
        format(t.dataManagement.dataRefreshed, { platform: platformName }),
        'success',
        5000,
      )
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

    let taskId: string
    try {
      taskId = await submitPlatformTask(platformId)
    } catch (error) {
      console.error('Error submitting task:', error)
      showMessage(
        format(t.dataManagement.submitTaskFailed, { platform: platformName }),
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
        format(t.dataManagement.confirmClearCache, { platform: platformName }),
      )
    ) {
      return
    }

    setClearing(true)
    try {
      const cleared = await clearPlatformCache(platformId).then(() => true, (error) => {
        console.error('Error clearing platform cache:', error)
        return false
      })
      if (!cleared) {
        showMessage(
          format(t.dataManagement.clearCacheFailed, { platform: platformName }),
          'error',
        )
        return
      }

      showMessage(
        format(t.dataManagement.cacheCleared, { platform: platformName }),
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
    return new Date(date).toLocaleString(locale)
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
        {fetchMessage ? <p role="status" className="whitespace-pre-line">{fetchMessage}</p> : null}
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
