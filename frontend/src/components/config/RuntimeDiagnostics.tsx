import type { ReactNode } from 'react'

import type { ToastType } from '../Toast'
import {
  LuActivity,
  LuAlertTriangle,
  LuCheckCircle,
  LuClock,
  LuCopy,
  LuCpu,
  LuDatabase,
  LuDownload,
  LuGauge,
  LuGlobe,
  LuRefreshCw,
  LuServer,
} from '@lib/icons'
import {

  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { fetchJson } from '../../utils/apiHelper'
import { userFacingError } from '../../utils/userFacingError'
import { getBuildInfo } from '../../utils/buildInfo'
import {
  SettingGroup,
  SettingGroupGrid,
  SettingsButton,
  useSettingGuide,
} from '../settings'
import { truncateVersionTag } from './runtimeDiagnosticsVersion'
import './RuntimeDiagnostics.css'

type DiagnosticStatus = 'ok' | 'warning' | 'error'
type OverallStatus = 'healthy' | 'warning' | 'critical'

interface DiagnosticCheck {
  id: 'database' | 'storage' | 'migrations' | 'memory' | 'location'
  status: DiagnosticStatus
  latency_ms?: number
  detail?: string | null
}

interface DiagnosticTask {
  id: string
  platform: string
  status: string
  progress: number
  created_at: string
  updated_at: string
  stuck: boolean
}

interface DiagnosticFailure {
  id: string
  platform: string
  error: string | null
  updated_at: string
}

interface RuntimeDiagnosticsResponse {
  success: boolean
  generated_at: string
  overall_status: OverallStatus
  runtime: {
    version: string
    commit_sha: string | null
    uptime_seconds: number
    config_mode: boolean
    /**
     * First schema apply / DB catalog stamp (RFC3339) — proxy for deploy time.
     */
    database_established_at?: string | null
    /** Process target OS (`linux` / `macos` / `windows`) */
    os?: string
    /** Process CPU architecture (`x86_64` / `aarch64` / …) */
    arch?: string
    family?: string
    pointer_width?: string
  }
  checks: DiagnosticCheck[]
  memory: {
    rss_mb?: number
    platform?: string
    note?: string
  }
  server_location?: {
    status: DiagnosticStatus
    confidence: 'high' | 'low' | 'unavailable'
    reason: 'verified' | 'single_source' | 'conflict' | 'unavailable'
    public_ips: string[]
    city?: string | null
    region?: string | null
    country?: string | null
    country_code?: string | null
    latitude?: number | null
    longitude?: number | null
    agreement_km?: number | null
    asn?: string | null
    organization?: string | null
    sources: string[]
    app_proxy_bypassed: boolean
    method: 'direct_https_consensus'
  }
  tasks: {
    counts: {
      total: number
      pending: number
      processing: number
      completed: number
      failed: number
    }
    active: DiagnosticTask[]
    recent_failures: DiagnosticFailure[]
    stuck_after_minutes: number
    recent_failure_hours: number
  }
}

interface RuntimeDiagnosticsProps {
  onMessage?: (message: string, type?: ToastType) => void
}

export default function RuntimeDiagnostics({
  onMessage,
}: RuntimeDiagnosticsProps) {
  const { locale, t, format } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [data, setData] = useState<RuntimeDiagnosticsResponse | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [requestLatency, setRequestLatency] = useState<number | null>(null)
  const requestIdRef = useRef(0)
  const buildInfo = useMemo(getBuildInfo, [])

  const loadDiagnostics = useCallback(async () => {
    const requestId = ++requestIdRef.current
    const startedAt = performance.now()
    setLoading(true)
    setError(null)

    try {
      const response = await fetchJson<RuntimeDiagnosticsResponse>(
        '/api/admin/diagnostics',
        undefined,
        t.config.runtimeDiagnosticsLoadFailed,
      )
      if (!response.success) {
        throw new Error(t.config.runtimeDiagnosticsLoadFailed)
      }
      if (requestId !== requestIdRef.current) return
      setRequestLatency(Math.max(0, Math.round(performance.now() - startedAt)))
      setData(response)
    } catch (loadError) {
      if (requestId !== requestIdRef.current) return
      setError(
        userFacingError(
          loadError,
          t.config.runtimeDiagnosticsLoadFailed,
        ),
      )
    } finally {
      if (requestId === requestIdRef.current) setLoading(false)
    }
  }, [t.config.runtimeDiagnosticsLoadFailed])

  useEffect(() => {
    void loadDiagnostics()
    return () => {
      requestIdRef.current += 1
    }
  }, [loadDiagnostics])

  const developmentBuild = buildInfo.version === 'dev'
  const versionMismatch = Boolean(
    data &&
      !developmentBuild &&
      buildInfo.version &&
      data.runtime.version &&
      buildInfo.version !== data.runtime.version,
  )
  /**
   * 总览「需要关注」不含服务器出口位置：该检查可保留卡片强调色，
   * 但不抬升 overall（兼容旧后端仍把 location warning 写进 overall_status）。
   */
  const overallStatus: OverallStatus = (() => {
    if (!data) return 'healthy'

    let status: OverallStatus = 'healthy'
    for (const check of data.checks) {
      if (check.id === 'location') continue
      if (check.status === 'error') {
        status = 'critical'
        break
      }
      if (check.status === 'warning') {
        status = 'warning'
      }
    }

    if (status !== 'critical') {
      const hasStuck = data.tasks.active.some((task) => task.stuck)
      const hasRecentFailures = data.tasks.recent_failures.length > 0
      if (hasStuck || hasRecentFailures || versionMismatch) {
        status = 'warning'
      }
    }

    return status
  })()

  const overallStatusLabel = (status: OverallStatus) => {
    if (status === 'healthy')
      return t.config.runtimeDiagnosticsStatusHealthy
    if (status === 'warning')
      return t.config.runtimeDiagnosticsStatusWarning
    return t.config.runtimeDiagnosticsStatusCritical
  }

  const checkStatusLabel = (status: DiagnosticStatus) => {
    if (status === 'ok') return t.config.runtimeDiagnosticsCheckHealthy
    if (status === 'warning')
      return t.config.runtimeDiagnosticsCheckWarning
    return t.config.runtimeDiagnosticsCheckCritical
  }

  const checkLabel = (
    id: DiagnosticCheck['id'] | 'backend' | 'version' | 'system',
  ) => {
    const labels = {
      backend: t.config.runtimeDiagnosticsBackend,
      database: t.config.runtimeDiagnosticsDatabase,
      storage: t.config.runtimeDiagnosticsStorage,
      migrations: t.config.runtimeDiagnosticsMigrations,
      memory: t.config.runtimeDiagnosticsMemory,
      location: t.config.runtimeDiagnosticsServerLocation,
      version: t.config.runtimeDiagnosticsVersion,
      system: t.config.runtimeDiagnosticsSystem,
    }
    return labels[id]
  }

  const formatOsLabel = (os: string | undefined) => {
    if (!os) return t.config.runtimeDiagnosticsStatusUnavailable
    if (os === 'linux') return t.config.runtimeDiagnosticsOsLinux
    if (os === 'macos') return t.config.runtimeDiagnosticsOsMacos
    if (os === 'windows') return t.config.runtimeDiagnosticsOsWindows
    return os
  }

  const formatArchLabel = (arch: string | undefined) => {
    if (!arch) return t.config.runtimeDiagnosticsStatusUnavailable
    if (arch === 'x86_64' || arch === 'amd64')
      return t.config.runtimeDiagnosticsArchX86_64
    if (arch === 'aarch64' || arch === 'arm64')
      return t.config.runtimeDiagnosticsArchAarch64
    if (arch === 'arm') return t.config.runtimeDiagnosticsArchArm
    return arch
  }

  const systemBadge = () => {
    const arch = data?.runtime.arch
    if (!arch) return checkStatusLabel('ok')
    return formatArchLabel(arch)
  }

  const systemDetail = () => {
    const runtime = data?.runtime
    if (!runtime?.os && !runtime?.arch) {
      return t.config.runtimeDiagnosticsStatusUnavailable
    }
    const parts = [
      formatOsLabel(runtime.os),
      formatArchLabel(runtime.arch),
      runtime.pointer_width
        ? format(t.config.runtimeDiagnosticsPointerWidth, {
            n: runtime.pointer_width,
          })
        : null,
    ].filter((part): part is string => Boolean(part))
    return format(t.config.runtimeDiagnosticsSystemDetail, {
      detail: parts.join(' · '),
    })
  }

  const formatDuration = (seconds: number) => {
    const days = Math.floor(seconds / 86400)
    const hours = Math.floor((seconds % 86400) / 3600)
    const minutes = Math.floor((seconds % 3600) / 60)
    const parts: string[] = []
    if (days > 0)
      parts.push(format(t.config.runtimeDiagnosticsDays, { n: days }))
    if (hours > 0)
      parts.push(format(t.config.runtimeDiagnosticsHours, { n: hours }))
    if (days === 0 && minutes > 0)
      parts.push(format(t.config.runtimeDiagnosticsMinutes, { n: minutes }))
    return parts.join(' ') || format(t.config.runtimeDiagnosticsMinutes, { n: 0 })
  }

  const formatDate = (value: string) =>
    new Intl.DateTimeFormat(locale, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(value))

  /** 说明文案（显示说明 / tooltip），可稍长 */
  const checkDetail = (check: DiagnosticCheck) => {
    if (check.detail) return check.detail
    if (check.id === 'database') {
      return format(t.config.runtimeDiagnosticsLatency, {
        n: check.latency_ms ?? 0,
      })
    }
    if (check.id === 'storage') {
      return t.config.runtimeDiagnosticsStorageWritable
    }
    if (check.id === 'migrations') {
      return t.config.runtimeDiagnosticsMigrationsApplied
    }
    if (check.id === 'memory') {
      return typeof data?.memory.rss_mb === 'number'
        ? format(t.config.runtimeDiagnosticsMemoryRss, {
            n: data.memory.rss_mb,
          })
        : (data?.memory.note ?? t.config.runtimeDiagnosticsStatusUnavailable)
    }
    if (check.id === 'location') {
      const location = data?.server_location
      if (!location || location.reason === 'unavailable') {
        return t.config.runtimeDiagnosticsLocationUnavailable
      }

      const place = [location.city, location.region, location.country]
        .filter(
          (value, index, values): value is string =>
            Boolean(value) && values.indexOf(value) === index,
        )
        .join(', ')
      const egress = location.public_ips.length
        ? format(t.config.runtimeDiagnosticsLocationEgress, {
            ip: location.public_ips.join(' / '),
          })
        : ''
      const verification =
        location.reason === 'verified'
          ? format(t.config.runtimeDiagnosticsLocationVerified, {
              n: location.agreement_km ?? 0,
            })
          : location.reason === 'conflict'
            ? t.config.runtimeDiagnosticsLocationConflict
            : t.config.runtimeDiagnosticsLocationSingleSource
      const network = [location.asn, location.organization]
        .filter(Boolean)
        .join(' ')

      return [
        place || location.country_code,
        egress,
        verification,
        network,
        location.app_proxy_bypassed
          ? t.config.runtimeDiagnosticsLocationProxyBypassed
          : '',
      ]
        .filter(Boolean)
        .join(' · ')
    }
    return ''
  }

  /** 角标：短词，不要长句；位置/开发模式即使非 ok 也优先写具体信息 */
  const checkBadge = (
    id: DiagnosticCheck['id'] | 'backend' | 'version' | 'system',
    status: DiagnosticStatus,
    latencyMs?: number,
  ): string => {
    if (id === 'system') {
      return systemBadge()
    }
    if (id === 'location') {
      const location = data?.server_location
      if (!location || location.reason === 'unavailable') {
        return checkStatusLabel(status === 'ok' ? 'error' : status)
      }
      // 警告（单源 / 冲突等）仍展示具体地点，不写「关注」
      return (
        location.city ||
        location.region ||
        location.country ||
        location.country_code ||
        location.public_ips[0] ||
        checkStatusLabel(status)
      )
    }

    if (id === 'version') {
      if (developmentBuild) return t.config.runtimeDiagnosticsBadgeDevMode
      if (status !== 'ok' || versionMismatch) {
        return checkStatusLabel('warning')
      }
      const version = data?.runtime.version
      return version
        ? truncateVersionTag(version)
        : checkStatusLabel(status)
    }

    if (status !== 'ok') return checkStatusLabel(status)

    if (id === 'backend' || id === 'database') {
      return format(t.config.runtimeDiagnosticsBadgeMs, {
        n: latencyMs ?? 0,
      })
    }
    if (id === 'storage') return t.config.runtimeDiagnosticsBadgeWritable
    if (id === 'migrations') return t.config.runtimeDiagnosticsBadgePassed
    if (id === 'memory') {
      return typeof data?.memory.rss_mb === 'number'
        ? format(t.config.runtimeDiagnosticsBadgeMb, {
            n: data.memory.rss_mb,
          })
        : checkStatusLabel(status)
    }
    return checkStatusLabel(status)
  }

  const makeReport = useCallback(() => {
    if (!data) return ''
    return JSON.stringify(
      {
        format: 'myriad-runtime-diagnostics',
        schema_version: 1,
        generated_at: data.generated_at,
        frontend: {
          version: buildInfo.version,
          commit_sha: buildInfo.commitSha,
        },
        request_latency_ms: requestLatency,
        version_mismatch: versionMismatch,
        diagnostics: data,
      },
      null,
      2,
    )
  }, [buildInfo, data, requestLatency, versionMismatch])

  const copyReport = useCallback(async () => {
    const report = makeReport()
    if (!report) return
    try {
      await navigator.clipboard.writeText(report)
      onMessage?.(t.config.runtimeDiagnosticsCopied, 'success')
    } catch (copyError) {
      onMessage?.(
        userFacingError(
          copyError,
          t.config.runtimeDiagnosticsCopyFailed,
        ),
        'error',
      )
    }
  }, [makeReport, onMessage, t])

  const downloadReport = useCallback(() => {
    const report = makeReport()
    if (!report) return
    const blob = new Blob([report], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `myriad-diagnostics-${new Date()
      .toISOString()
      .replace(/[:.]/g, '-')}.json`
    document.body.appendChild(anchor)
    anchor.click()
    anchor.remove()
    URL.revokeObjectURL(url)
  }, [makeReport])

  const backendVersion = data?.runtime.version ?? ''
  const frontendVersion = buildInfo.version
  const versionDetailFull = !data
    ? ''
    : developmentBuild
      ? format(t.config.runtimeDiagnosticsVersionDevelopment, {
          version: backendVersion,
        })
      : versionMismatch
        ? format(t.config.runtimeDiagnosticsVersionMismatch, {
            frontend: frontendVersion,
            backend: backendVersion,
          })
        : format(t.config.runtimeDiagnosticsVersionMatch, {
            version: backendVersion,
          })
  const versionDetailDisplay = !data
    ? ''
    : developmentBuild
      ? format(t.config.runtimeDiagnosticsVersionDevelopment, {
          version: truncateVersionTag(backendVersion),
        })
      : versionMismatch
        ? format(t.config.runtimeDiagnosticsVersionMismatch, {
            frontend: truncateVersionTag(frontendVersion),
            backend: truncateVersionTag(backendVersion),
          })
        : format(t.config.runtimeDiagnosticsVersionMatch, {
            version: truncateVersionTag(backendVersion),
          })

  const checks: Array<{
    id: DiagnosticCheck['id'] | 'backend' | 'version' | 'system'
    status: DiagnosticStatus
    badge: string
    detail: string
    /** Optional full tooltip when `detail` is truncated for display. */
    title?: string
    icon: ReactNode
  }> = data
    ? [
        {
          id: 'backend',
          status: 'ok',
          badge: checkBadge('backend', 'ok', requestLatency ?? 0),
          detail: format(t.config.runtimeDiagnosticsLatency, {
            n: requestLatency ?? 0,
          }),
          icon: <LuServer />,
        },
        {
          id: 'system',
          status: 'ok',
          badge: checkBadge('system', 'ok'),
          detail: systemDetail(),
          icon: <LuCpu />,
        },
        ...data.checks.map((check) => ({
          id: check.id,
          // 位置卡片保留 warning/error 强调色；总览 overall 已排除 location
          status: check.status,
          badge: checkBadge(check.id, check.status, check.latency_ms),
          detail: checkDetail(check),
          icon:
            check.id === 'database' ? (
              <LuDatabase />
            ) : check.id === 'location' ? (
              <LuGlobe />
            ) : check.id === 'memory' ? (
              <LuGauge />
            ) : check.id === 'migrations' ? (
              <LuActivity />
            ) : (
              <LuCheckCircle />
            ),
        })),
        {
          id: 'version' as const,
          status: versionMismatch
            ? ('warning' as DiagnosticStatus)
            : ('ok' as DiagnosticStatus),
          badge: checkBadge(
            'version',
            versionMismatch ? 'warning' : 'ok',
          ),
          detail: versionDetailDisplay,
          title: versionDetailFull,
          icon: <LuActivity />,
        },
      ]
    : []

  return (
    <SettingGroup
      title={t.config.runtimeDiagnosticsTitle}
      description={t.config.runtimeDiagnosticsDesc}
      icon={<LuActivity />}
      className="runtime-diagnostics"
      {...bindGuide(
        'advanced.runtimeDiagnostics',
        g.advanced.runtimeDiagnostics,
      )}
    >
      {/* 总览 + 操作：独立卡片 */}
      <div
        className={`runtime-diagnostics-overview is-${error ? 'critical' : overallStatus}`}
      >
        <div
          className="runtime-diagnostics-summary"
          aria-live="polite"
        >
          <span className="runtime-diagnostics-summary-icon" aria-hidden>
            {error || overallStatus === 'critical' ? (
              <LuAlertTriangle />
            ) : (
              <LuCheckCircle />
            )}
          </span>
          <span className="runtime-diagnostics-summary-text">
            <strong>
              {error
                ? t.config.runtimeDiagnosticsStatusUnavailable
                : overallStatusLabel(overallStatus)}
            </strong>
            <small className="runtime-diagnostics-summary-meta">
              {error ?? (
                <>
                  {data ? (
                    <>
                      <span>{formatDate(data.generated_at)}</span>
                      <span>
                        {format(t.config.runtimeDiagnosticsUptime, {
                          duration: formatDuration(
                            data.runtime.uptime_seconds,
                          ),
                        })}
                      </span>
                      {data.runtime.database_established_at ? (
                        <span>
                          {format(t.config.runtimeDiagnosticsDeployedAt, {
                            date: formatDate(
                              data.runtime.database_established_at,
                            ),
                          })}
                        </span>
                      ) : null}
                    </>
                  ) : (
                    t.common.loading
                  )}
                </>
              )}
            </small>
          </span>
        </div>
        <div className="runtime-diagnostics-actions">
          <SettingsButton
            size="sm"
            icon={<LuRefreshCw />}
            loading={loading}
            aria-label={t.common.refresh}
            onClick={() => void loadDiagnostics()}
          >
            {t.common.refresh}
          </SettingsButton>
          <SettingsButton
            size="sm"
            icon={<LuCopy />}
            disabled={!data}
            aria-label={t.common.copy}
            onClick={() => void copyReport()}
          >
            {t.common.copy}
          </SettingsButton>
          <SettingsButton
            size="sm"
            icon={<LuDownload />}
            disabled={!data}
            aria-label={t.config.runtimeDiagnosticsDownload}
            onClick={downloadReport}
          >
            {t.config.runtimeDiagnosticsDownload}
          </SettingsButton>
        </div>
      </div>

      {data && (
        <>
          <SettingGroupGrid
            columns={3}
            minColumnWidth="15rem"
            variant="card"
            align="stretch"
            className="runtime-diagnostics-grid"
            ariaLabel={t.config.runtimeDiagnosticsTitle}
          >
            {checks.map((check) => (
              <SettingGroup
                key={check.id}
                title={checkLabel(check.id)}
                icon={check.icon}
                description={check.detail}
                toc={false}
                titleExtra={
                  check.badge ? (
                    <span
                      className="runtime-diagnostics-check-status"
                      title={check.title || check.detail || undefined}
                    >
                      {check.badge}
                    </span>
                  ) : null
                }
                className={`runtime-diagnostics-check-group is-${check.status}`}
              />
            ))}
          </SettingGroupGrid>

          {(data.tasks.active.length > 0 ||
            data.tasks.recent_failures.length > 0) && (
            <div className="runtime-diagnostics-task-panels">
              {data.tasks.active.length > 0 && (
                <section className="runtime-diagnostics-task-panel">
                  <h5>
                    <LuClock aria-hidden />
                    {t.config.runtimeDiagnosticsActiveTasks}
                  </h5>
                  {data.tasks.active.map((task) => (
                    <div
                      key={task.id}
                      className={`runtime-diagnostics-task${task.stuck ? ' is-stuck' : ''}`}
                    >
                      <span>
                        <strong>{task.platform}</strong>
                        <small>
                          {task.status} · {Math.round(task.progress)}%
                        </small>
                      </span>
                      <span>
                        {task.stuck
                          ? t.config.runtimeDiagnosticsStuck
                          : formatDate(task.updated_at)}
                      </span>
                    </div>
                  ))}
                </section>
              )}

              {data.tasks.recent_failures.length > 0 && (
                <section className="runtime-diagnostics-task-panel is-failure">
                  <h5>
                    <LuAlertTriangle aria-hidden />
                    {format(t.config.runtimeDiagnosticsRecentFailures, {
                      n: data.tasks.recent_failure_hours,
                    })}
                  </h5>
                  {data.tasks.recent_failures.map((failure) => (
                    <div
                      key={failure.id}
                      className="runtime-diagnostics-task"
                    >
                      <span>
                        <strong>{failure.platform}</strong>
                        <small>
                          {failure.error ??
                            t.config.runtimeDiagnosticsUnknownFailure}
                        </small>
                      </span>
                      <span>{formatDate(failure.updated_at)}</span>
                    </div>
                  ))}
                </section>
              )}
            </div>
          )}
        </>
      )}
    </SettingGroup>
  )
}
