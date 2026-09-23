import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { apiService } from '../../services/api'
import { resolvePlatformId } from '../../utils/platformId'
import { localizePlatformSummary } from '../../utils/platformRefresh'
import { userFacingError } from '../../utils/userFacingError'
import { SettingGroup, useSettingGuide } from '../settings'
import { PreviewSampleImage } from './PreviewSampleImage'
import './PlatformDataPreview.css'

export interface PlatformDataPreviewHandle {
  reload: () => Promise<void>
}

interface PreviewUser {
  username: string
  user_id: string
  level: string | null
  follower_count: number | null
  following_count: number | null
  total_content: number
}

interface PreviewMetric {
  key: string
  value: number
}

interface PreviewSample {
  title: string
  subtitle?: string | null
  image?: string | null
}

interface PlatformCachePreviewResponse {
  success: boolean
  platform: string
  exists: boolean
  modified_at: string | null
  user: PreviewUser | null
  summary: string | null
  metrics: PreviewMetric[]
  samples: PreviewSample[]
  message?: string
}

export interface PlatformDataPreviewProps {
  platformName: string
}

function formatCompactNumber(n: number, locale: string): string {
  if (!Number.isFinite(n)) return '—'
  try {
    return new Intl.NumberFormat(locale, {
      notation: Math.abs(n) >= 10000 ? 'compact' : 'standard',
      maximumFractionDigits: n % 1 === 0 ? 0 : 1,
    }).format(n)
  } catch {
    return String(n)
  }
}

function formatPlaytimeMinutes(minutes: number, t: {
  playtimeHours: string
  playtimeMinutes: string
}, format: (template: string, params: Record<string, string | number>) => string): string {
  if (minutes < 60) {
    return format(t.playtimeMinutes, { n: Math.round(minutes) })
  }
  const hours = minutes / 60
  const rounded = hours >= 100 ? Math.round(hours) : Math.round(hours * 10) / 10
  return format(t.playtimeHours, { n: rounded })
}

export const PlatformDataPreview = forwardRef<
  PlatformDataPreviewHandle,
  PlatformDataPreviewProps
>(({ platformName }, ref) => {
  const { t, locale, format } = useI18n()
  const dm = t.dataManagement
  const { catalog: g, bindGuide } = useSettingGuide()
  const platformId = resolvePlatformId(platformName)
  const [loading, setLoading] = useState(false)
  const [preview, setPreview] = useState<PlatformCachePreviewResponse | null>(
    null,
  )
  const [error, setError] = useState<string | null>(null)
  const requestRef = useRef(0)

  const numberLocale = locale

  const load = useCallback(async () => {
    if (!platformId) return
    const requestId = ++requestRef.current
    setLoading(true)
    setError(null)
    try {
      const data = await apiService.get<PlatformCachePreviewResponse>(
        `/cache/preview/${encodeURIComponent(platformId)}`,
      )
      if (requestId !== requestRef.current) return
      if (!data.success) {
        setError(dm.previewLoadFailed)
        setPreview(null)
        return
      }
      setPreview(data)
    } catch (e) {
      console.error(`Failed to load ${platformName} data preview:`, e)
      if (requestId !== requestRef.current) return
      setError(userFacingError(e, dm.previewLoadFailed))
      setPreview(null)
    } finally {
      if (requestId === requestRef.current) {
        setLoading(false)
      }
    }
  }, [platformId, platformName, dm.previewLoadFailed])

  useImperativeHandle(ref, () => ({ reload: load }), [load])

  useEffect(() => {
    setPreview(null)
    void load()
    return () => {
      requestRef.current += 1
    }
  }, [load])

  if (!platformId) return null

  const metricLabel = (key: string): string => {
    const map = dm.previewMetric as Record<string, string | undefined>
    return map[key] || key
  }

  const formatMetricValue = (key: string, value: number): string => {
    if (key === 'total_playtime_minutes') {
      return formatPlaytimeMinutes(value, dm, format)
    }
    if (key === 'average_completion') {
      return `${formatCompactNumber(value, numberLocale)}%`
    }
    return formatCompactNumber(value, numberLocale)
  }

  const user = preview?.user
  const hasUser =
    user &&
    (Boolean(user.username && user.username.trim()) ||
      Boolean(user.user_id && user.user_id.trim()))
  const metrics = preview?.metrics ?? []
  const samples = preview?.samples ?? []
  const summary = localizePlatformSummary(preview?.summary?.trim() || '', dm.fetchResult)
  const hasBody =
    hasUser || summary || metrics.length > 0 || samples.length > 0
  const empty =
    !loading && !error && preview && (!preview.exists || !hasBody)

  const updatedText = preview?.modified_at
    ? dm.previewUpdatedAt.replace(
        '{time}',
        new Date(preview.modified_at).toLocaleString(numberLocale),
      )
    : null

  return (
    <SettingGroup
      title={dm.previewTitle}
      detail={dm.previewDesc}
      {...bindGuide('platforms.dataPreview', g.platforms.dataPreview)}
      className="platform-data-preview"
    >
      {loading && !preview ? (
        <p className="platform-data-preview-status settings-text-3">
          {t.common.loading}
        </p>
      ) : null}

      {error ? (
        <p className="platform-data-preview-status settings-text-3">
          {error}
        </p>
      ) : null}

      {empty ? (
        <p className="platform-data-preview-status settings-text-3">
          {dm.previewEmpty}
        </p>
      ) : null}

      {!error && hasBody ? (
        <div className="platform-data-preview-body">
          {hasUser ? (
            <div className="platform-data-preview-user">
              <span className="platform-data-preview-username settings-text-1">
                {user!.username || user!.user_id}
              </span>
              {user!.level ? (
                <span className="platform-data-preview-level">{user!.level}</span>
              ) : null}
              {user!.user_id && user!.username ? (
                <span className="platform-data-preview-id settings-text-3">
                  ID {user!.user_id}
                </span>
              ) : null}
            </div>
          ) : null}

          {summary ? (
            <p className="platform-data-preview-summary settings-text-2">
              {summary}
            </p>
          ) : null}

          {metrics.length > 0 ? (
            <div className="platform-data-preview-metrics" role="list">
              {metrics.map((m) => (
                <div
                  key={m.key}
                  className="platform-data-preview-metric"
                  role="listitem"
                >
                  <span className="platform-data-preview-metric-value">
                    {formatMetricValue(m.key, m.value)}
                  </span>
                  <span className="platform-data-preview-metric-label settings-text-3">
                    {metricLabel(m.key)}
                  </span>
                </div>
              ))}
            </div>
          ) : null}

          {samples.length > 0 ? (
            <ul className="platform-data-preview-samples">
              {samples.map((s, i) => (
                <li key={`${s.title}-${i}`} className="platform-data-preview-sample">
                  <PreviewSampleImage src={s.image} />
                  <span className="platform-data-preview-sample-text">
                    <span className="platform-data-preview-sample-title settings-text-1">
                      {s.title}
                    </span>
                    {s.subtitle ? (
                      <span className="platform-data-preview-sample-sub settings-text-3">
                        {s.subtitle}
                      </span>
                    ) : null}
                  </span>
                </li>
              ))}
            </ul>
          ) : null}

          {updatedText ? (
            <p className="platform-data-preview-updated settings-text-3">
              {updatedText}
            </p>
          ) : null}
        </div>
      ) : null}
    </SettingGroup>
  )
})

export default PlatformDataPreview
