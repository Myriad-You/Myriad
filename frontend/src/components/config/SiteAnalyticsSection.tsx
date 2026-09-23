import type { ReactNode } from 'react'
import type { SettingOption } from '../settings/types'
import type { ToastType } from '../Toast'
import type { AnalyticsRangeState } from './analytics/AnalyticsRangePicker'
import type { MetricDelta } from './analytics/compareDeltaLogic'
import type { RankRow } from './analytics/RankList'
import type { TrendPoint } from './analytics/TrendChart'
import {
  LuActivity,
  LuBarChart3,
  LuCalendar,
  LuClock,
  LuDownload,
  LuEye,
  LuFileText,
  LuGlobe,
  LuRefreshCw,
  LuUpload,
  LuUsers,
  LuZap,
} from '@lib/icons'
import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { apiService } from '../../services/api'
import {
  isAnalyticsOptedOut,
  setAnalyticsOptOut,
} from '../../utils/siteAnalytics'
import { userFacingError } from '../../utils/userFacingError'
import {
  guideDomProps,
  SettingGroup,
  SettingTitleGuideEntry,
  SettingTitleHelp,
  SettingTitleSelect,
  SettingTitleTag,
  SwitchItem,
  useSettingGuide,
  useSettingsHelp,
} from '../settings'
import {
  analyticsRangeDayCount,
  AnalyticsRangePicker,
  analyticsRangeQuery,

  defaultAnalyticsRange,
} from './analytics/AnalyticsRangePicker'
import { CompareDelta } from './analytics/CompareDelta'
import { EmptyCard } from './analytics/EmptyCard'
import {
  analyticsBackupFilenameDay,
  formatCount,
  formatDuration,
  shortDay,
} from './analytics/format'
import { RankList } from './analytics/RankList'
import { TrendChart } from './analytics/TrendChart'
import './SiteAnalyticsSection.css'

function AnalyticsTextBlock({
  id,
  title,
  description,
  guidePath,
  guide,
  titleExtra,
  children,
}: {
  id: string
  title: string
  description?: string
  guidePath?: string
  guide?: ReactNode
  titleExtra?: ReactNode
  children: ReactNode
}) {
  const { t, format } = useI18n()
  const helpCtx = useSettingsHelp()
  const expandHelp = Boolean(helpCtx?.showDetails)
  const showHelp = Boolean(description)

  return (
    <section
      id={id}
      className="site-analytics-block"
      {...guideDomProps(guidePath)}
    >
      <h5 className="site-analytics-block-title">
        <span className="site-analytics-block-title-text">
          {title}
          {showHelp && !expandHelp ? (
            <SettingTitleHelp
              ariaLabel={format(t.config.detailHelpAriaNamed, { title })}
            >
              {description}
            </SettingTitleHelp>
          ) : null}
          {guide ? (
            <SettingTitleGuideEntry title={title} guide={guide} />
          ) : null}
        </span>
        {titleExtra}
      </h5>
      {expandHelp && showHelp ? (
        <p className="site-analytics-block-desc">{description}</p>
      ) : null}
      {children}
    </section>
  )
}

interface DailyPoint {
  day: string
  views: number
  unique_visitors: number
  engagement_ms?: number
}

interface PageRow {
  path: string
  views: number
  unique_visitors: number
  avg_engagement_ms?: number
}

interface EventTargetRow {
  target: string
  count: number
  unique_visitors: number
}

interface EventRow {
  name: string
  count: number
  unique_visitors: number
  targets?: EventTargetRow[]
}

interface ReferrerRow {
  host: string
  count: number
}

interface CountryRow {
  code: string
  name: string
  views: number
  unique_visitors: number
}

interface AnalyticsSummary {
  success: boolean
  days: number
  from: string
  to: string
  timezone?: string
  today: { views: number; unique_visitors: number }
  range: {
    views: number
    unique_visitors: number
    engagement_ms?: number
    avg_engagement_ms?: number
    approx_bounce_permille?: number
  }
  compare?: {
    day?: {
      kind?: string
      views?: MetricDelta
      unique_visitors?: MetricDelta
    }
    range?: {
      kind?: string
      views?: MetricDelta
      unique_visitors?: MetricDelta
    }
  }
  all_time: { views: number; unique_visitors: number }
  daily: DailyPoint[]
  pages: PageRow[]
  events?: EventRow[]
  referrers?: ReferrerRow[]
  countries?: CountryRow[]
}

function flagEmoji(code: string): string {
  const cc = code.trim().toUpperCase()
  if (!/^[A-Z]{2}$/.test(cc)) return '🏳️'
  return String.fromCodePoint(
    ...Iterator.from(cc).map((c) => 0x1F1E6 - 65 + c.charCodeAt(0)),
  )
}

function pageLabel(
  path: string,
  labels: Record<string, string | undefined>,
): string {
  if (labels[path]) return labels[path]!
  if (path.startsWith('/journal/')) return labels['/journal'] || path
  if (path.startsWith('/tapp/')) return labels['/tapp/:id'] || path
  if (path.startsWith('/tapps/')) return labels['/tapps/:id'] || path
  return path
}

const ANALYTICS_BACKUP_FORMAT = 'myriad-analytics-backup'
/** Full-history backups can be large; the default 30s budget is for ordinary calls. */
const ANALYTICS_TRANSFER_TIMEOUT_MS = 5 * 60_000

interface SiteAnalyticsSectionProps {
  showMessage?: (message: string, type?: ToastType, duration?: number) => void
  enabled?: boolean
  onEnabledChange?: (enabled: boolean) => void
}

function isAnalyticsBackup(data: unknown): data is Record<string, unknown> {
  if (!data || typeof data !== 'object') return false
  const o = data as Record<string, unknown>
  if (
    o.format !== ANALYTICS_BACKUP_FORMAT ||
    typeof o.version !== 'number' ||
    o.version !== 1
  ) {
    return false
  }
  const integrity = o.integrity
  if (!integrity || typeof integrity !== 'object') return false
  const i = integrity as Record<string, unknown>
  if (typeof i.alg !== 'string' || typeof i.token !== 'string') return false
  if (typeof i.content_hash !== 'string' || i.content_hash.length < 32) {
    return false
  }
  return true
}

function analyticsImportErrorMessage(
  code: string | undefined,
  a: {
    importFailed: string
    importIntegrityFailed: string
    importMissingIntegrity: string
    importInvalid: string
  },
): string {
  switch (code) {
    case 'missing_integrity':
      return a.importMissingIntegrity
    case 'content_hash_mismatch':
    case 'invalid_integrity_token':
    case 'integrity_token_mismatch':
    case 'integrity_key_mismatch':
    case 'unsupported_integrity_alg':
    case 'missing_integrity_token':
      return a.importIntegrityFailed
    case 'counts_mismatch':
    case 'row_validation_failed':
    case 'invalid_format':
    case 'unsupported_version':
    case 'too_many_rows':
    case 'invalid_mode':
      return `${a.importFailed}: ${code}`
    default:
      return code ? `${a.importFailed}: ${code}` : a.importFailed
  }
}

const SiteAnalyticsSection: React.FC<SiteAnalyticsSectionProps> = ({
  showMessage,
  enabled = true,
  onEnabledChange,
}) => {
  const { t, locale, format } = useI18n()
  const { catalog: g, bindGuide, renderGuide } = useSettingGuide()
  const a = t.config.analytics
  const numberLocale = locale

  const [range, setRange] = useState<AnalyticsRangeState>(() =>
    defaultAnalyticsRange(),
  )
  const [data, setData] = useState<AnalyticsSummary | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [ioBusy, setIoBusy] = useState(false)
  const [eventFilter, setEventFilter] = useState('')
  const [optedOut, setOptedOut] = useState(() => isAnalyticsOptedOut())
  const importInputRef = useRef<HTMLInputElement>(null)
  const collectionEnabled = enabled !== false

  const load = useCallback(
    async (signal?: AbortSignal) => {
      setLoading(true)
      setError(null)
      try {
        const res = await apiService.get<AnalyticsSummary>(
          `/analytics/summary?${analyticsRangeQuery(range)}`,
          { signal },
        )
        if (signal?.aborted) return
        if (res?.success) {
          setData(res)
        } else {
          setError(a.loadFailed)
          setData(null)
          showMessage?.(a.loadFailed, 'error', 0)
        }
      } catch (e) {
        if (signal?.aborted) return
        if (e instanceof DOMException && e.name === 'AbortError') return
        console.error('analytics summary failed', e)
        const msg = userFacingError(e, a.loadFailed)
        setError(msg)
        setData(null)
        showMessage?.(msg, 'error', 0)
      } finally {
        if (!signal?.aborted) setLoading(false)
      }
    },
    [range, a.loadFailed, showMessage],
  )

  useEffect(() => {
    const ac = new AbortController()
    void load(ac.signal)
    return () => ac.abort()
  }, [load])

  const count = useCallback(
    (n: number) => formatCount(n, numberLocale),
    [numberLocale],
  )
  const duration = useCallback(
    (ms: number) => formatDuration(ms, numberLocale),
    [numberLocale],
  )
  const compareLabels = useMemo(
    () => ({
      day: a.compareDay,
      week: a.compareWeek,
      month: a.compareMonth,
      period: a.comparePeriod,
      new: a.compareNew,
      vsPrevious: a.compareVsPrevious,
    }),
    [
      a.compareDay,
      a.compareWeek,
      a.compareMonth,
      a.comparePeriod,
      a.compareNew,
      a.compareVsPrevious,
    ],
  )

  const firstLoad = loading && !data
  const refreshing = loading && !!data
  const tile = (value: string) => (firstLoad ? '…' : value)

  const pageLabels = a.pageLabels as Record<string, string | undefined>
  const eventLabels = (a.eventLabels || {}) as Record<
    string,
    string | undefined
  >

  const trendPoints = useMemo<TrendPoint[]>(
    () =>
      (data?.daily ?? []).map((d) => ({
        day: d.day,
        views: d.views,
        visitors: d.unique_visitors,
        engagementMs: d.engagement_ms,
      })),
    [data],
  )

  const pageRows = useMemo<RankRow[]>(
    () =>
      (data?.pages ?? []).map((p) => ({
        key: p.path,
        name: pageLabel(p.path, pageLabels),
        meta: p.path,
        value: p.views,
        secondary: count(p.unique_visitors),
        tertiary:
          p.avg_engagement_ms != null && p.avg_engagement_ms > 0
            ? duration(p.avg_engagement_ms)
            : undefined,
      })),
    [data, pageLabels, count, duration],
  )

  const eventFilterOptions: SettingOption<string>[] = useMemo(() => {
    const opts: SettingOption<string>[] = [
      { value: '', label: a.eventFilterAll },
    ]
    for (const ev of data?.events ?? []) {
      if (!ev.name) continue
      opts.push({
        value: ev.name,
        label: eventLabels[ev.name] || ev.name,
      })
    }
    if (eventFilter && !opts.some((o) => o.value === eventFilter)) {
      opts.push({
        value: eventFilter,
        label: eventLabels[eventFilter] || eventFilter,
      })
    }
    return opts
  }, [data?.events, eventLabels, a.eventFilterAll, eventFilter])

  const eventRows = useMemo<RankRow[]>(() => {
    const source = data?.events ?? []
    const list = eventFilter
      ? source.filter((ev) => ev.name === eventFilter)
      : source
    return list.map((ev) => {
      const targets = (ev.targets ?? [])
        .filter((t) => t.target && t.count > 0)
        .slice(0, 12)
      return {
        key: ev.name,
        name: eventLabels[ev.name] || ev.name,
        meta: eventLabels[ev.name] ? ev.name : undefined,
        value: ev.count,
        secondary: count(ev.unique_visitors),
        subRows: targets.map((t) => ({
          key: `${ev.name}:${t.target}`,
          name: t.target,
          value: t.count,
          secondary: count(t.unique_visitors),
        })),
      }
    })
  }, [data?.events, eventFilter, eventLabels, count])

  const referrerRows = useMemo<RankRow[]>(
    () =>
      (data?.referrers ?? []).map((r) => ({
        key: r.host,
        name: r.host,
        value: r.count,
      })),
    [data],
  )

  const countryRows = useMemo(
    () =>
      Iterator.from(data?.countries ?? [])
        .filter((c) => c.code && (c.unique_visitors > 0 || c.views > 0))
        .toArray()
        .toSorted(
          (a, b) =>
            b.unique_visitors - a.unique_visitors ||
            b.views - a.views ||
            a.code.localeCompare(b.code),
        ),
    [data],
  )
  const topCountries = countryRows.slice(0, 3)

  const avgEng = data?.range?.avg_engagement_ms ?? 0
  const bouncePct =
    data?.range?.approx_bounce_permille != null && (data?.range?.views ?? 0) > 0
      ? Math.round((data.range.approx_bounce_permille / 1000) * 100)
      : null
  const dayCount = Math.max(
    1,
    data?.days ?? analyticsRangeDayCount(range),
  )
  const dailyAvg = Math.round((data?.range.views ?? 0) / dayCount)
  const peak = useMemo(
    () =>
      trendPoints.reduce<TrendPoint | null>(
        (best, p) => (best == null || p.views > best.views ? p : best),
        null,
      ),
    [trendPoints],
  )
  const hasTrend = trendPoints.length > 0

  const handleExport = useCallback(async () => {
    if (ioBusy) return
    setIoBusy(true)
    try {
      const backup = await apiService.get<Record<string, unknown>>('/analytics/export', {
        timeout: ANALYTICS_TRANSFER_TIMEOUT_MS,
      })
      if (!backup || backup.success === false) {
        throw new Error(
          typeof backup?.error === 'string' ? backup.error : a.exportFailed,
        )
      }
      const json = JSON.stringify(backup, null, 2)
      const blob = new Blob([json], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const el = document.createElement('a')
      el.href = url
      const day = analyticsBackupFilenameDay(backup)
      el.download = `myriad-analytics-backup-${day}.json`
      document.body.appendChild(el)
      el.click()
      document.body.removeChild(el)
      URL.revokeObjectURL(url)
      showMessage?.(a.exportSuccess, 'success')
    } catch (e) {
      console.error('analytics export failed', e)
      showMessage?.(userFacingError(e, a.exportFailed), 'error')
    } finally {
      setIoBusy(false)
    }
  }, [a, ioBusy, showMessage])

  const handleImportClick = useCallback(() => {
    if (ioBusy) return
    importInputRef.current?.click()
  }, [ioBusy])

  const handleImportFile = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      e.target.value = ''
      if (!file || ioBusy) return

      const reader = new FileReader()
      reader.onload = async (ev) => {
        try {
          const raw = JSON.parse(String(ev.target?.result ?? ''))
          if (!isAnalyticsBackup(raw)) {
            showMessage?.(a.importInvalid, 'error')
            return
          }
          if (!window.confirm(a.importConfirm)) return

          setIoBusy(true)
          const res = await apiService.post<{
            success?: boolean
            error?: string
          }>(
            '/analytics/import',
            { ...raw, mode: 'replace' },
            { timeout: ANALYTICS_TRANSFER_TIMEOUT_MS },
          )
          if (!res?.success) {
            throw new Error(
              analyticsImportErrorMessage(res?.error, a),
            )
          }
          showMessage?.(a.importSuccess, 'success')
          await load(undefined)
        } catch (err) {
          console.error('analytics import failed', err)
          showMessage?.(userFacingError(err, a.importFailed), 'error')
        } finally {
          setIoBusy(false)
        }
      }
      reader.onerror = () => showMessage?.(a.importInvalid, 'error')
      reader.readAsText(file)
    },
    [a, ioBusy, load, showMessage],
  )

  const titleExtra = (
    <>
      {error ? (
        <SettingTitleTag variant="danger" title={error}>
          {error}
        </SettingTitleTag>
      ) : null}
      <SettingTitleTag
        variant="muted"
        className={
          loading
            ? 'site-analytics-refresh-tag is-loading'
            : 'site-analytics-refresh-tag'
        }
        icon={
          <LuRefreshCw
            size={12}
            className={loading ? 'is-spinning' : undefined}
            aria-hidden
          />
        }
        onClick={() => void load(undefined)}
        disabled={loading || ioBusy}
        title={a.refresh}
      >
        {a.refresh}
      </SettingTitleTag>
      <SettingTitleTag
        variant="muted"
        className={ioBusy ? 'site-analytics-io-tag is-loading' : 'site-analytics-io-tag'}
        icon={<LuDownload size={12} aria-hidden />}
        onClick={() => void handleExport()}
        disabled={ioBusy}
        title={a.exportTitle}
      >
        {a.exportLabel}
      </SettingTitleTag>
      <SettingTitleTag
        variant="muted"
        className={ioBusy ? 'site-analytics-io-tag is-loading' : 'site-analytics-io-tag'}
        icon={<LuUpload size={12} aria-hidden />}
        onClick={handleImportClick}
        disabled={ioBusy}
        title={a.importTitle}
      >
        {a.importLabel}
      </SettingTitleTag>
      <input
        ref={importInputRef}
        type="file"
        accept="application/json,.json"
        className="site-analytics-import-input"
        aria-hidden
        tabIndex={-1}
        onChange={handleImportFile}
      />
      <AnalyticsRangePicker
        value={range}
        onChange={setRange}
        disabled={loading || ioBusy}
        labels={{
          daysN: a.daysN,
          custom: a.rangeCustom,
          rangeAria: a.rangeAria,
          fromAria: a.rangeFromAria,
          toAria: a.rangeToAria,
          customTitle: a.rangeCustomTitle,
          customHint: a.rangeCustomHint,
          apply: a.rangeApply,
          clear: a.rangeClear,
          daysSelected: a.rangeDaysSelected,
          prevMonth: a.rangePrevMonth,
          nextMonth: a.rangeNextMonth,
          weekdays: a.rangeWeekdays,
          monthTitle: a.rangeMonthTitle,
          today: a.rangeToday,
        }}
      />
    </>
  )

  return (
    <div className={`site-analytics${collectionEnabled ? '' : ' is-disabled'}`}>
      <SettingGroup
        id="visitor-stats"
        title={a.visitorTitle}
        description={a.visitorDesc}
        icon={<LuBarChart3 size={15} />}
        titleExtra={titleExtra}
        switch={
          onEnabledChange
            ? {
                checked: collectionEnabled,
                onChange: onEnabledChange,
                ariaLabel: a.enableAria,
                preview: {
                  on: a.enablePreviewOn,
                  off: a.enablePreviewOff,
                },
              }
            : undefined
        }
        {...bindGuide('platforms.visitorStats', g.platforms.visitorStats)}
      >
        {!collectionEnabled ? (
          <p className="site-analytics-disabled-banner" role="status">
            {a.disabledBanner}
          </p>
        ) : null}
        <div className="site-analytics-visitors">
          <div className="site-analytics-tiles">
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuEye size={13} aria-hidden />
                {a.todayViews}
              </span>
              <div className="site-analytics-tile-metric">
                <span className="site-analytics-tile-value">
                  {tile(count(data?.today.views ?? 0))}
                </span>
                <CompareDelta
                  kind={data?.compare?.day?.kind ?? 'day'}
                  delta={data?.compare?.day?.views}
                  labels={compareLabels}
                  locale={numberLocale}
                  hidden={firstLoad}
                  formatPrevious={count}
                />
              </div>
            </div>
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuUsers size={13} aria-hidden />
                {a.todayVisitors}
              </span>
              <div className="site-analytics-tile-metric">
                <span className="site-analytics-tile-value">
                  {tile(count(data?.today.unique_visitors ?? 0))}
                </span>
                <CompareDelta
                  kind={data?.compare?.day?.kind ?? 'day'}
                  delta={data?.compare?.day?.unique_visitors}
                  labels={compareLabels}
                  locale={numberLocale}
                  hidden={firstLoad}
                  formatPrevious={count}
                />
              </div>
            </div>
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuEye size={13} aria-hidden />
                {format(a.rangeViews, { n: dayCount })}
              </span>
              <div className="site-analytics-tile-metric">
                <span className="site-analytics-tile-value">
                  {tile(count(data?.range.views ?? 0))}
                </span>
                <CompareDelta
                  kind={data?.compare?.range?.kind}
                  delta={data?.compare?.range?.views}
                  labels={compareLabels}
                  locale={numberLocale}
                  hidden={firstLoad}
                  formatPrevious={count}
                />
              </div>
            </div>
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuUsers size={13} aria-hidden />
                {format(a.rangeVisitors, { n: dayCount })}
              </span>
              <div className="site-analytics-tile-metric">
                <span className="site-analytics-tile-value">
                  {tile(count(data?.range.unique_visitors ?? 0))}
                </span>
                <CompareDelta
                  kind={data?.compare?.range?.kind}
                  delta={data?.compare?.range?.unique_visitors}
                  labels={compareLabels}
                  locale={numberLocale}
                  hidden={firstLoad}
                  formatPrevious={count}
                />
              </div>
            </div>
            <div className="site-analytics-tile" title={a.avgEngagementHint}>
              <span className="site-analytics-tile-label">
                <LuClock size={13} aria-hidden />
                {a.avgEngagement}
              </span>
              <span className="site-analytics-tile-value site-analytics-tile-value--sm">
                {tile(duration(avgEng))}
              </span>
            </div>
            <div className="site-analytics-tile" title={a.approxBounceHint}>
              <span className="site-analytics-tile-label">
                <LuActivity size={13} aria-hidden />
                {a.approxBounce}
              </span>
              <span className="site-analytics-tile-value site-analytics-tile-value--sm">
                {tile(bouncePct == null ? '—' : `${bouncePct}%`)}
              </span>
            </div>
            <div
              className="site-analytics-tile site-analytics-tile--countries"
              tabIndex={0}
              aria-label={
                topCountries.length > 0
                  ? `${a.topCountries}: ${countryRows
                      .map(
                        (c) =>
                          `${c.name || c.code} ${c.unique_visitors} ${a.countryVisitors}`,
                      )
                      .join(', ')}`
                  : `${a.topCountries}: ${a.topCountriesEmpty}`
              }
            >
              <span className="site-analytics-tile-label">
                <LuGlobe size={13} aria-hidden />
                {a.topCountries}
              </span>
              {firstLoad ? (
                <span className="site-analytics-tile-value site-analytics-tile-value--sm">
                  …
                </span>
              ) : topCountries.length > 0 ? (
                <ul className="site-analytics-country-list" aria-label={a.topCountries}>
                  {topCountries.map((c, i) => (
                    <li key={c.code} className="site-analytics-country-chip">
                      <span className="site-analytics-country-rank" aria-hidden>
                        {i + 1}
                      </span>
                      <span
                        className="site-analytics-country-flag"
                        aria-hidden
                      >
                        {flagEmoji(c.code)}
                      </span>
                      <span className="site-analytics-country-code">
                        {c.code}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : (
                <span className="site-analytics-tile-value site-analytics-tile-value--sm">
                  {a.topCountriesEmpty}
                </span>
              )}
              {!firstLoad && countryRows.length > 0 ? (
                <div className="site-analytics-country-tip" role="tooltip">
                  <div className="site-analytics-country-tip-head">
                    {a.topCountries}
                    <small>{format(a.daysN, { n: dayCount })}</small>
                  </div>
                  <ul className="site-analytics-country-tip-list">
                    {countryRows.map((c, i) => (
                      <li key={c.code}>
                        <span className="site-analytics-country-tip-rank">
                          {i + 1}
                        </span>
                        <span
                          className="site-analytics-country-flag"
                          aria-hidden
                        >
                          {flagEmoji(c.code)}
                        </span>
                        <span className="site-analytics-country-tip-name">
                          {c.name || c.code}
                          <small>{c.code}</small>
                        </span>
                        <span className="site-analytics-country-tip-stats">
                          {count(c.unique_visitors)} {a.countryVisitors}
                          <small>
                            {count(c.views)} {a.countryViews}
                          </small>
                        </span>
                      </li>
                    ))}
                  </ul>
                </div>
              ) : null}
            </div>
          </div>

          {hasTrend ? (
            <TrendChart
              points={trendPoints}
              refreshing={refreshing}
              numberLocale={numberLocale}
            />
          ) : (
            <EmptyCard
              text={a.empty}
              icon={<LuBarChart3 size={18} />}
              loading={loading}
              tall
            />
          )}

          <dl className="site-analytics-meta">
            <div>
              <dt>{a.scopeLabel}</dt>
              <dd>
                {data ? (
                  <>
                    <LuCalendar size={12} aria-hidden />
                    {shortDay(data.from)}
                    <span aria-hidden>→</span>
                    {shortDay(data.to)}
                    {data.timezone ? (
                      <small
                        className="site-analytics-scope-tz"
                        title={a.timezoneHint}
                      >
                        {data.timezone}
                      </small>
                    ) : null}
                  </>
                ) : (
                  '—'
                )}
              </dd>
            </div>
            <div>
              <dt>{a.allTimeViews}</dt>
              <dd>{tile(count(data?.all_time.views ?? 0))}</dd>
            </div>
            <div title={a.allTimeVisitorsHint}>
              <dt>{a.allTimeVisitors}</dt>
              <dd>{tile(count(data?.all_time.unique_visitors ?? 0))}</dd>
            </div>
            <div>
              <dt>{a.dailyAvgViews}</dt>
              <dd>{tile(count(dailyAvg))}</dd>
            </div>
            {peak && peak.views > 0 ? (
              <div>
                <dt>{a.peakViews}</dt>
                <dd>
                  {count(peak.views)}
                  <small>{shortDay(peak.day)}</small>
                </dd>
              </div>
            ) : null}
          </dl>

        </div>

        <AnalyticsTextBlock
          id="page-analytics"
          title={a.pagesTitle}
          description={a.pagesDesc}
          guidePath="platforms.pageAnalytics"
          guide={renderGuide(g.platforms.pageAnalytics)}
        >
          <RankList
            rows={pageRows}
            formatValue={count}
            headers={{
              name: a.colPage,
              value: a.colViews,
              secondary: a.colVisitors,
              tertiary: a.avgEngagementShort,
            }}
            emptyText={a.emptyPages}
            emptyIcon={<LuFileText size={18} />}
            loading={firstLoad}
            refreshing={refreshing}
          />
        </AnalyticsTextBlock>

        <div
          className="site-analytics-side-grid"
          role="group"
          aria-label={`${a.eventsTitle} / ${a.referrersTitle}`}
        >
          <AnalyticsTextBlock
            id="event-analytics"
            title={a.eventsTitle}
            description={a.eventsDesc}
            guidePath="platforms.eventAnalytics"
            guide={renderGuide(g.platforms.eventAnalytics)}
            titleExtra={
              <SettingTitleSelect
                variant="title"
                icon={<LuZap size={12} />}
                label={a.eventFilter}
                value={eventFilter}
                options={eventFilterOptions}
                onChange={setEventFilter}
                aria-label={a.eventFilterAria}
                disabled={firstLoad}
                searchable
                searchPlaceholder={t.common.search}
                emptySearchText={t.common.noResults}
              />
            }
          >
            <RankList
              rows={eventRows}
              formatValue={count}
              headers={{
                name: a.colEvent,
                value: a.colCount,
                secondary: a.colVisitors,
              }}
              emptyText={a.emptyEvents}
              emptyIcon={<LuZap size={18} />}
              loading={firstLoad}
              refreshing={refreshing}
            />
          </AnalyticsTextBlock>

          <AnalyticsTextBlock
            id="referrer-analytics"
            title={a.referrersTitle}
            description={a.referrersDesc}
            guidePath="platforms.referrerAnalytics"
            guide={renderGuide(g.platforms.referrerAnalytics)}
          >
            <RankList
              rows={referrerRows}
              formatValue={count}
              headers={{ name: a.colReferrer, value: a.colViews }}
              emptyText={a.emptyReferrers}
              emptyIcon={<LuGlobe size={18} />}
              loading={firstLoad}
              refreshing={refreshing}
            />
          </AnalyticsTextBlock>
        </div>

        <SwitchItem
          itemKey="analytics_opt_out"
          label={a.optOutLabel}
          description={a.optOutDesc}
          {...bindGuide('platforms.analyticsOptOut', g.platforms.analyticsOptOut)}
          value={optedOut}
          onChange={(next) => {
            setAnalyticsOptOut(next)
            setOptedOut(next)
          }}
          preview={{
            on: a.optOutPreviewOn,
            off: a.optOutPreviewOff,
          }}
        />
      </SettingGroup>
    </div>
  )
}

export default SiteAnalyticsSection
