/**
 * 数据管理视图组件
 * 统一管理平台数据缓存和智能过滤
 */

import { FaGithub, FaSteam, FaSyncAlt, SiBilibili, SiNeteasecloudmusic } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import AnimatedView from '../components/AnimatedView'
import { ButtonSpinner } from '../components/Spinner'
import { TaskStatus } from '../components/TaskStatus'
import Toast from '../components/Toast'
import { API_URL } from '../config'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { usePageReady } from '../hooks/animation'
import { useDataManagementScheduler } from '../hooks/animation/pages/simple'
import { useBackgroundTasks } from '../hooks/useBackgroundTasks'
import { getCSRFToken } from '../utils/csrf'
import { hasSessionHint } from '../utils/sessionDetection'
import '../components/ConfigForm.css'

// 平台定义
const PLATFORMS = [
  { id: 'bilibili', name: 'Bilibili', icon: SiBilibili, color: '#00A1D6' },
  { id: 'github', name: 'GitHub', icon: FaGithub, color: '#181717' },
  { id: 'netease', name: 'NetEase Music', nameKey: 'neteaseMusic', icon: SiNeteasecloudmusic, color: '#C20C0C' },
  { id: 'steam', name: 'Steam', icon: FaSteam, color: '#00ADEE' },
]

interface CacheInfo {
  platform: string
  exists: boolean
  size_bytes?: number
  modified_at?: string
  path: string
}

interface PlatformStatus {
  platform_id: string
  platform_name: string
  has_raw_data: boolean
  raw_data_size: number
  raw_fetched_at: string | null
  has_filtered_cache: boolean
  filtered_cache_size: number
  filtered_modified_at: string | null
}

export default function DataManagement() {
  // 🆕 初始化页面级调度器
  useDataManagementScheduler()

  const navigate = useNavigate()
  const { isAdmin: authIsAdmin, isAuthenticated, checkAuth } = useAuth()
  const { t } = useI18n()
  const isPageReady = usePageReady()
  const [loading, setLoading] = useState(true)
  const [isAdmin, setIsAdmin] = useState(false)
  const [message, setMessage] = useState('')

  const [platformStatuses, setPlatformStatuses] = useState<PlatformStatus[]>([])
  const [cacheStatuses, setCacheStatuses] = useState<CacheInfo[]>([])
  const [statusLoading, setStatusLoading] = useState(false)
  const [refreshingPlatform, setRefreshingPlatform] = useState<string | null>(null)
  const [activeTask, setActiveTask] = useState<string | null>(null)
  const [processingPlatform, setProcessingPlatform] = useState<string | null>(null)
  const [clearingPlatform, setClearingPlatform] = useState<string | null>(null)

  const {
    getCacheStatus,
    clearPlatformCache,
    submitTask,
    isSubmitting,
  } = useBackgroundTasks()

  // 使用 AuthContext 检查管理员权限
  useEffect(() => {
    if (!isAuthenticated) {
      // 智能检测：检查是否有登录迹象
      if (hasSessionHint()) {
        // 有登录迹象，触发认证检查
        checkAuth()
      }
      else {
        // 无登录迹象，直接重定向到登录页
        navigate('/login', { replace: true })
      }
    }
    else {
      if (!authIsAdmin) {
        navigate('/', { replace: true })
        return
      }

      // 预加载 CSRF Token
      getCSRFToken(true).then(() => {
        setIsAdmin(true)
        setLoading(false)
      })
    }
  }, [authIsAdmin, isAuthenticated, checkAuth, navigate])

  // 加载所有状态
  const loadAllStatuses = async () => {
    setStatusLoading(true)
    try {
      // 加载原始数据状态
      const metadataResponse = await fetch(`${API_URL}/api/profile/metadata`, {
        credentials: 'include',
      })
      const metadataData = await metadataResponse.json()

      // 加载过滤缓存状态
      const cacheData = await getCacheStatus()

      const statuses: PlatformStatus[] = PLATFORMS.map((platform) => {
        const platformData = metadataData.success && metadataData.data
          ? metadataData.data[platform.id]
          : null

        const cacheInfo = cacheData?.caches?.find((c: CacheInfo) => c.platform === platform.id)

        return {
          platform_id: platform.id,
          platform_name: platform.name,
          has_raw_data: !!platformData,
          raw_data_size: platformData ? JSON.stringify(platformData).length : 0,
          raw_fetched_at: metadataData.fetched_at || null,
          has_filtered_cache: cacheInfo?.exists || false,
          filtered_cache_size: cacheInfo?.size_bytes || 0,
          filtered_modified_at: cacheInfo?.modified_at || null,
        }
      })

      setPlatformStatuses(statuses)
      if (cacheData?.caches) {
        setCacheStatuses(cacheData.caches)
      }
    }
    catch (error) {
      setMessage(t.dataManagement.loadStatusFailed)
      setTimeout(() => setMessage(''), 3000)
    }
    finally {
      setStatusLoading(false)
    }
  }

  // 刷新单个平台原始数据
  const refreshSinglePlatform = async (platformId: string, platformName: string) => {
    if (!confirm(t.dataManagement.confirmRefreshData.replace('{platform}', platformName)))
      return

    setRefreshingPlatform(platformId)
    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) {
        setMessage(t.dataManagement.csrfTokenError)
        setTimeout(() => setMessage(''), 5000)
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

      if (data.success) {
        setMessage(t.dataManagement.dataRefreshed.replace('{platform}', platformName))
        setTimeout(() => loadAllStatuses(), 500)
      }
      else {
        setMessage(t.dataManagement.refreshFailed + (data.message ? `: ${data.message}` : ''))
      }
    }
    catch (error: any) {
      setMessage(t.dataManagement.refreshFailed + (error.message ? `: ${error.message}` : ''))
    }
    finally {
      setRefreshingPlatform(null)
      setTimeout(() => setMessage(''), 5000)
    }
  }

  // 处理平台智能过滤
  const handleProcessPlatform = async (platformId: string, platformName: string) => {
    const taskId = await submitTask(platformId)

    if (taskId) {
      setActiveTask(taskId)
      setProcessingPlatform(platformId)
    }
    else {
      setMessage(t.dataManagement.submitTaskFailed.replace('{platform}', platformName))
      setTimeout(() => setMessage(''), 3000)
    }
  }

  // 清除平台过滤缓存
  const handleClearCache = async (platformId: string, platformName: string) => {
    if (!confirm(t.dataManagement.confirmClearCache.replace('{platform}', platformName)))
      return

    setClearingPlatform(platformId)
    try {
      const success = await clearPlatformCache(platformId)

      if (success) {
        setMessage(t.dataManagement.cacheCleared.replace('{platform}', platformName))
        await loadAllStatuses()
      }
      else {
        setMessage(t.dataManagement.clearCacheFailed.replace('{platform}', platformName))
      }
    }
    finally {
      setClearingPlatform(null)
      setTimeout(() => setMessage(''), 3000)
    }
  }

  const handleTaskComplete = () => {
    setActiveTask(null)
    setProcessingPlatform(null)
    loadAllStatuses()
  }

  const handleTaskClose = () => {
    setActiveTask(null)
    setProcessingPlatform(null)
  }

  // 辅助函数：格式化字节大小
  const formatBytes = (bytes: number): string => {
    if (bytes < 1024)
      return `${bytes} B`
    if (bytes < 1024 * 1024)
      return `${(bytes / 1024).toFixed(2)} KB`
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
  }

  // 辅助函数：格式化日期时间
  const formatDateTime = (dateStr: string | null): string => {
    if (!dateStr)
      return t.dataManagement.unknown
    return new Date(dateStr).toLocaleString('zh-CN')
  }

  useEffect(() => {
    if (isAdmin) {
      loadAllStatuses()
    }
  }, [isAdmin])

  if (loading || !isAdmin) {
    return null
  }

  return (
    <AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
      {message && <Toast message={message} />}

      <div className="modern-config-container">
        {/* 返回按钮 */}
        <button
          type="button"
          onClick={() => navigate('/config')}
          className="btn-base btn-secondary back-to-config-button"
          title={t.dataManagement.backToConfig}
          aria-label={t.dataManagement.backToConfig}
        >
          <svg width="16" height="16" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" d="M10 19l-7-7m0 0l7-7m-7 7h18" />
          </svg>
          <span>{t.dataManagement.backToConfig}</span>
        </button>

        {/* 页面标题 */}
        <div className="card-header" style={{ marginBottom: '1.25rem' }}>
          <h2 style={{ fontSize: '1.5rem', marginBottom: '0.5rem' }}>{t.dataManagement.dataManagementTitle}</h2>
          <p>{t.dataManagement.dataManagementDesc}</p>
        </div>

        {/* 后台任务状态显示 */}
        {activeTask && (
          <div style={{ marginBottom: '1rem' }}>
            <TaskStatus
              taskId={activeTask}
              onComplete={handleTaskComplete}
              onClose={handleTaskClose}
            />
          </div>
        )}

        {/* 平台数据管理 */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {PLATFORMS.map((platform, index) => {
            const status = platformStatuses.find(s => s.platform_id === platform.id)
            const cache = cacheStatuses.find(c => c.platform === platform.id)
            const IconComponent = platform.icon
            const isProcessing = processingPlatform === platform.id

            return (
              <motion.div
                key={platform.id}
                className="config-section"
                style={{ marginBottom: 0 }}
                initial={{ opacity: 0, y: 20 }}
                animate={isPageReady ? { opacity: 1, y: 0 } : { opacity: 0, y: 20 }}
                transition={{ delay: isPageReady ? 0.1 + index * 0.08 : 0, duration: 0.3 }}
              >
                {/* 平台标题 */}
                <div className="section-header" style={{ marginBottom: '0.75rem' }}>
                  <div className="section-header-left">
                    <div style={{
                      width: '2.5rem',
                      height: '2.5rem',
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'center',
                      flexShrink: 0,
                    }}
                    >
                      <IconComponent
                        style={{
                          color: '#1f2937',
                          fontSize: '1.75rem',
                        }}
                        className="dark:text-gray-100"
                      />
                    </div>
                    <div>
                      <h3
                        style={{
                          fontSize: '1rem',
                          fontWeight: 600,
                          marginBottom: 0,
                          color: '#1f2937',
                        }}
                        className="dark:text-gray-100"
                      >
                        {(platform as any).nameKey ? t.dataManagement[(platform as any).nameKey as keyof typeof t.dataManagement] : platform.name}
                      </h3>
                    </div>
                  </div>
                </div>

                {/* 原始数据 - 横向布局 */}
                <div className="config-field-row" style={{ marginBottom: '0.5rem' }}>
                  <div className="field-label-inline" style={{ flex: 1, minWidth: 0 }}>
                    <span>{t.dataManagement.rawData}</span>
                    {status?.has_raw_data
                      ? (
                          <span className="field-hint" style={{ marginTop: '0.25rem', display: 'block' }}>
                            {formatBytes(status.raw_data_size)}
                            {' '}
                            ·
                            {formatDateTime(status.raw_fetched_at)}
                          </span>
                        )
                      : (
                          <span className="field-hint" style={{ marginTop: '0.25rem', display: 'block' }}>{t.dataManagement.noData}</span>
                        )}
                  </div>
                  <div className="field-control">
                    <button
                      type="button"
                      onClick={() => refreshSinglePlatform(platform.id, (platform as any).nameKey ? t.dataManagement[(platform as any).nameKey as keyof typeof t.dataManagement] as string : platform.name)}
                      disabled={refreshingPlatform === platform.id || statusLoading}
                      className="btn-base btn-sm btn-secondary"
                      title={t.dataManagement.refreshData}
                      style={{ minWidth: '4rem' }}
                    >
                      {refreshingPlatform === platform.id
                        ? (
                            <>
                              <ButtonSpinner size="sm" />
                              <span>{t.dataManagement.refreshing}</span>
                            </>
                          )
                        : (
                            <>
                              <FaSyncAlt />
                              <span>{t.dataManagement.refresh}</span>
                            </>
                          )}
                    </button>
                  </div>
                </div>

                {/* 智能过滤 - 横向布局 */}
                <div className="config-field-row" style={{ marginBottom: 0 }}>
                  <div className="field-label-inline" style={{ flex: 1, minWidth: 0 }}>
                    <span>{t.dataManagement.smartFilter}</span>
                    {cache
                      ? (
                          <span className="field-hint" style={{ marginTop: '0.25rem', display: 'block' }}>
                            {formatBytes(cache.size_bytes || 0)}
                            {' '}
                            ·
                            {formatDateTime(cache.modified_at || null)}
                          </span>
                        )
                      : (
                          <span className="field-hint" style={{ marginTop: '0.25rem', display: 'block' }}>{t.dataManagement.noCache}</span>
                        )}
                  </div>
                  <div className="field-control" style={{ display: 'flex', gap: '0.5rem' }}>
                    <button
                      type="button"
                      onClick={() => handleProcessPlatform(platform.id, (platform as any).nameKey ? t.dataManagement[(platform as any).nameKey as keyof typeof t.dataManagement] as string : platform.name)}
                      disabled={isProcessing || !status?.has_raw_data || statusLoading}
                      className="btn-base btn-sm btn-primary"
                      title={t.dataManagement.processData}
                      style={{ minWidth: '4rem' }}
                    >
                      {isProcessing
                        ? (
                            <>
                              <ButtonSpinner size="sm" />
                              <span>{t.dataManagement.processing}</span>
                            </>
                          )
                        : (
                            t.dataManagement.process
                          )}
                    </button>
                    {cache && (
                      <button
                        type="button"
                        onClick={() => handleClearCache(platform.id, (platform as any).nameKey ? t.dataManagement[(platform as any).nameKey as keyof typeof t.dataManagement] as string : platform.name)}
                        disabled={clearingPlatform === platform.id || statusLoading}
                        className="btn-base btn-sm btn-danger"
                        title={t.dataManagement.clearCache}
                        style={{ minWidth: '4rem' }}
                      >
                        {clearingPlatform === platform.id
                          ? (
                              <>
                                <ButtonSpinner size="sm" />
                                <span>{t.dataManagement.clearing}</span>
                              </>
                            )
                          : (
                              <>
                                <svg style={{ width: '0.875rem', height: '0.875rem' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                                </svg>
                                <span>{t.dataManagement.clear}</span>
                              </>
                            )}
                      </button>
                    )}
                  </div>
                </div>
              </motion.div>
            )
          })}
        </div>

        {/* 使用说明 */}
        <div className="info-card" style={{ marginTop: '1rem', marginBottom: 0 }}>
          <p className="info-title" style={{ marginBottom: '0.75rem' }}>{t.dataManagement.usageTitle}</p>
          <div className="info-text" style={{ fontSize: '0.8125rem', lineHeight: '1.5' }}>
            <p style={{ marginBottom: '0.5rem' }}>{t.dataManagement.usageRawData}</p>
            <p style={{ marginBottom: '0.5rem' }}>{t.dataManagement.usageSmartFilter}</p>
            <p style={{ marginBottom: 0 }}>{t.dataManagement.usageBackground}</p>
          </div>
        </div>
      </div>
    </AnimatedView>
  )
}
