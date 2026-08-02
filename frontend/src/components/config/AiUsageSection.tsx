/**
 * 数据及统计 · AI 使用统计
 *
 * 复用访客统计的 TrendChart / RankList / EmptyCard / site-analytics CSS。
 * 数据源：GET /api/analytics/ai-usage（tapp_ai_cost_ledger 按日/用户/模型聚合）。
 */

import type { SettingOption } from '../settings/types'
import type { ToastType } from '../Toast'
import type { AnalyticsRangeState } from './analytics/AnalyticsRangePicker'
import type { MetricDelta } from './analytics/compareDeltaLogic'
import {
  LuActivity,
  LuBarChart3,
  LuCalendar,
  LuCpu,
  LuRefreshCw,
  LuSparkles,
  LuUser,
  LuUsers,
} from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { fetchJson } from '../../utils/apiHelper'
import {
  SettingGroup,
  SettingTitleSelect,
  SettingTitleTag,
  useSettingGuide,
} from '../settings'
import {
  aiDailyToTrendPoints,
  aiModelsToRankRows,
  aiUserDisplayName,
  aiUsersToRankRows,
} from './analytics/aiUsageMap'
import {
  analyticsRangeDayCount,
  AnalyticsRangePicker,
  analyticsRangeQuery,

  defaultAnalyticsRange,
} from './analytics/AnalyticsRangePicker'
import { CompareDelta } from './analytics/CompareDelta'
import { EmptyCard } from './analytics/EmptyCard'
import { formatCount, shortDay } from './analytics/format'
import { RankList } from './analytics/RankList'
import { TrendChart } from './analytics/TrendChart'
import './SiteAnalyticsSection.css'

interface AiUsageUserRow {
  subject_id: number
  username?: string | null
  display_name?: string | null
  is_admin?: boolean
  is_owner?: boolean
  calls: number
  tokens: number
  input_tokens?: number
  output_tokens?: number
}

interface AiUsageModelRow {
  model: string
  provider?: string
  calls: number
  tokens: number
  input_tokens?: number
  output_tokens?: number
}

interface AiUsageSourceRow {
  source: string
  calls: number
  tokens: number
  input_tokens?: number
  output_tokens?: number
}

interface AiUsageDaily {
  day: string
  calls: number
  tokens: number
  input_tokens?: number
  output_tokens?: number
}

interface AiUsageSummary {
  success?: boolean
  from?: string
  to?: string
  timezone?: string
  days?: number
  today?: { calls: number; tokens: number }
  range?: {
    calls: number
    tokens: number
    input_tokens?: number
    output_tokens?: number
    users?: number
    models?: number
  }
  compare?: {
    day?: {
      kind?: string
      calls?: MetricDelta
      tokens?: MetricDelta
    }
    range?: {
      kind?: string
      calls?: MetricDelta
      tokens?: MetricDelta
    }
  }
  daily?: AiUsageDaily[]
  by_user?: AiUsageUserRow[]
  by_model?: AiUsageModelRow[]
  by_source?: AiUsageSourceRow[]
  filter_options?: {
    users?: AiUsageUserRow[]
    models?: string[]
  }
}

interface AiUsageSectionProps {
  showMessage?: (msg: string, type?: ToastType, duration?: number) => void
}

const AiUsageSection: React.FC<AiUsageSectionProps> = () => {
  const { t, locale } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const a = t.config.analytics
  const numberLocale =
    locale === 'zh-CN' ? 'zh-CN' : locale === 'ja-JP' ? 'ja-JP' : 'en-US'

  const [range, setRange] = useState<AnalyticsRangeState>(() =>
    defaultAnalyticsRange(),
  )
  const [subjectId, setSubjectId] = useState<string>('')
  const [model, setModel] = useState<string>('')
  const [data, setData] = useState<AiUsageSummary | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(
    async (signal?: AbortSignal) => {
      setLoading(true)
      setError(null)
      try {
        const params = new URLSearchParams(analyticsRangeQuery(range))
        if (subjectId) params.set('subject_id', subjectId)
        if (model) params.set('model', model)
        const res = await fetchJson<AiUsageSummary>(
          `${API_URL}/api/analytics/ai-usage?${params.toString()}`,
          signal ? { signal } : undefined,
          'Unable to load AI usage',
        )
        if (signal?.aborted) return
        if (res?.success) {
          setData(res)
        } else {
          setError(a.aiUsageLoadFailed)
          setData(null)
        }
      } catch (e) {
        if (signal?.aborted) return
        if (e instanceof DOMException && e.name === 'AbortError') return
        console.error('ai usage summary failed', e)
        setError(a.aiUsageLoadFailed)
        setData(null)
      } finally {
        if (!signal?.aborted) setLoading(false)
      }
    },
    [range, subjectId, model, a.aiUsageLoadFailed],
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
  const dayCount = data?.days ?? analyticsRangeDayCount(range)

  const trendPoints = useMemo(
    () => aiDailyToTrendPoints(data?.daily ?? []),
    [data?.daily],
  )

  const hasTrend = trendPoints.some((p) => p.views > 0 || p.visitors > 0)

  const callsLabel = useCallback(
    (n: number) => a.aiUsageCallsN.replace('{n}', count(n)),
    [a.aiUsageCallsN, count],
  )

  const userRows = useMemo(() => {
    const users = data?.by_user ?? []
    const base = aiUsersToRankRows(users, {
      anonymousLabel: a.aiUsageAnonymous,
      callsLabel,
    })
    // Annotate admin/owner in meta (staff are included in site-wide stats).
    const byKey = new Map(users.map((u) => [String(u.subject_id), u]))
    return base.map((row) => {
      const raw = byKey.get(row.key)
      if (!raw) return row
      const tags: string[] = []
      if (raw.is_owner) tags.push(a.aiUsageRoleOwner)
      else if (raw.is_admin) tags.push(a.aiUsageRoleAdmin)
      if (tags.length === 0) return row
      return {
        ...row,
        meta: row.meta ? `${row.meta} · ${tags.join('/')}` : tags.join('/'),
      }
    })
  }, [
    data?.by_user,
    a.aiUsageAnonymous,
    a.aiUsageRoleAdmin,
    a.aiUsageRoleOwner,
    callsLabel,
  ])

  const modelRows = useMemo(
    () =>
      aiModelsToRankRows(data?.by_model ?? [], {
        callsLabel,
      }),
    [data?.by_model, callsLabel],
  )

  const sourceLabel = useCallback(
    (source: string) => {
      const key = source.trim().toLowerCase()
      if (key === 'scheduler' || key.startsWith('internal:scheduler')) {
        return a.aiUsageSourceScheduler
      }
      if (key === 'agent') return a.aiUsageSourceAgent
      if (key === 'reports') return a.aiUsageSourceReports
      if (key === 'runtime' || key.startsWith('internal:')) {
        return a.aiUsageSourceRuntime
      }
      return source || a.aiUsageSourceOther
    },
    [
      a.aiUsageSourceScheduler,
      a.aiUsageSourceAgent,
      a.aiUsageSourceReports,
      a.aiUsageSourceRuntime,
      a.aiUsageSourceOther,
    ],
  )

  const sourceRows = useMemo(
    () =>
      (data?.by_source ?? []).map((s) => ({
        key: s.source,
        name: sourceLabel(s.source),
        meta: s.source,
        value: s.tokens,
        secondary: callsLabel(s.calls),
      })),
    [data?.by_source, sourceLabel, callsLabel],
  )

  const filterUsers = data?.filter_options?.users ?? []
  const filterModels = data?.filter_options?.models ?? []

  const userFilterOptions: SettingOption<string>[] = useMemo(() => {
    const opts: SettingOption<string>[] = [
      { value: '', label: a.aiUsageFilterAll },
      ...filterUsers.map((u) => ({
        value: String(u.subject_id),
        label: aiUserDisplayName(u, a.aiUsageAnonymous),
      })),
    ]
    // Keep current selection visible if filter list shrank after reload
    if (subjectId && !opts.some((o) => o.value === subjectId)) {
      opts.push({ value: subjectId, label: `#${subjectId}` })
    }
    return opts
  }, [filterUsers, a.aiUsageFilterAll, a.aiUsageAnonymous, subjectId])

  const modelFilterOptions: SettingOption<string>[] = useMemo(() => {
    const opts: SettingOption<string>[] = [
      { value: '', label: a.aiUsageFilterAll },
      ...filterModels.map((m) => ({ value: m, label: m })),
    ]
    if (model && !opts.some((o) => o.value === model)) {
      opts.push({ value: model, label: model })
    }
    return opts
  }, [filterModels, a.aiUsageFilterAll, model])

  const tile = (v: string) => (firstLoad ? '…' : v)

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
        disabled={loading}
        title={a.refresh}
      >
        {a.refresh}
      </SettingTitleTag>
      <SettingTitleSelect
        variant="title"
        icon={<LuUser size={12} />}
        label={a.aiUsageFilterUser}
        value={subjectId}
        options={userFilterOptions}
        onChange={setSubjectId}
        aria-label={a.aiUsageFilterUser}
        disabled={loading}
        searchable
        searchPlaceholder={t.common.search}
        emptySearchText={t.common.noResults}
      />
      <SettingTitleSelect
        variant="title"
        icon={<LuCpu size={12} />}
        label={a.aiUsageFilterModel}
        value={model}
        options={modelFilterOptions}
        onChange={setModel}
        aria-label={a.aiUsageFilterModel}
        disabled={loading}
      />
      <AnalyticsRangePicker
        value={range}
        onChange={setRange}
        disabled={loading}
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
    <div className="site-analytics">
      <SettingGroup
        id="ai-usage-stats"
        title={a.aiUsageTitle}
        description={a.aiUsageDesc}
        icon={<LuSparkles size={15} />}
        titleExtra={titleExtra}
        {...bindGuide('platforms.aiUsage', g.platforms.aiUsage)}
      >
        <div className="site-analytics-visitors">
          <div className="site-analytics-tiles site-analytics-tiles--6">
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuActivity size={13} aria-hidden />
                {a.aiUsageTodayCalls}
              </span>
              <div className="site-analytics-tile-metric">
                <span className="site-analytics-tile-value">
                  {tile(count(data?.today?.calls ?? 0))}
                </span>
                <CompareDelta
                  kind={data?.compare?.day?.kind ?? 'day'}
                  delta={data?.compare?.day?.calls}
                  labels={compareLabels}
                  locale={numberLocale}
                  hidden={firstLoad}
                  formatPrevious={count}
                />
              </div>
            </div>
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuCpu size={13} aria-hidden />
                {a.aiUsageTodayTokens}
              </span>
              <div className="site-analytics-tile-metric">
                <span className="site-analytics-tile-value">
                  {tile(count(data?.today?.tokens ?? 0))}
                </span>
                <CompareDelta
                  kind={data?.compare?.day?.kind ?? 'day'}
                  delta={data?.compare?.day?.tokens}
                  labels={compareLabels}
                  locale={numberLocale}
                  hidden={firstLoad}
                  formatPrevious={count}
                />
              </div>
            </div>
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuActivity size={13} aria-hidden />
                {a.aiUsageRangeCalls.replace('{n}', String(dayCount))}
              </span>
              <div className="site-analytics-tile-metric">
                <span className="site-analytics-tile-value">
                  {tile(count(data?.range?.calls ?? 0))}
                </span>
                <CompareDelta
                  kind={data?.compare?.range?.kind}
                  delta={data?.compare?.range?.calls}
                  labels={compareLabels}
                  locale={numberLocale}
                  hidden={firstLoad}
                  formatPrevious={count}
                />
              </div>
            </div>
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuCpu size={13} aria-hidden />
                {a.aiUsageRangeTokens.replace('{n}', String(dayCount))}
              </span>
              <div className="site-analytics-tile-metric">
                <span className="site-analytics-tile-value">
                  {tile(count(data?.range?.tokens ?? 0))}
                </span>
                <CompareDelta
                  kind={data?.compare?.range?.kind}
                  delta={data?.compare?.range?.tokens}
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
                {a.aiUsageUsers}
              </span>
              <span className="site-analytics-tile-value">
                {tile(count(data?.range?.users ?? 0))}
              </span>
            </div>
            <div className="site-analytics-tile">
              <span className="site-analytics-tile-label">
                <LuSparkles size={13} aria-hidden />
                {a.aiUsageModels}
              </span>
              <span className="site-analytics-tile-value">
                {tile(count(data?.range?.models ?? 0))}
              </span>
            </div>
          </div>

          {hasTrend ? (
            <TrendChart
              points={trendPoints}
              refreshing={refreshing}
              numberLocale={numberLocale}
              seriesLabels={{
                primary: a.aiUsageLegendCalls,
                secondary: a.aiUsageLegendTokens,
                chartAria: a.aiUsageChartAria,
                showEngagement: false,
              }}
            />
          ) : (
            <EmptyCard
              text={a.aiUsageEmpty}
              icon={<LuBarChart3 size={18} />}
              loading={loading}
              tall
            />
          )}

          <dl className="site-analytics-meta">
            <div>
              <dt>{a.scopeLabel}</dt>
              <dd>
                {data?.from && data?.to ? (
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
              <dt>{a.aiUsageInputTokens}</dt>
              <dd>{tile(count(data?.range?.input_tokens ?? 0))}</dd>
            </div>
            <div>
              <dt>{a.aiUsageOutputTokens}</dt>
              <dd>{tile(count(data?.range?.output_tokens ?? 0))}</dd>
            </div>
          </dl>

        </div>

        <section className="site-analytics-block" id="ai-usage-by-source">
          <h5 className="site-analytics-block-title">
            <span className="site-analytics-block-title-text">
              {a.aiUsageBySource}
            </span>
          </h5>
          <RankList
            rows={sourceRows}
            formatValue={count}
            headers={{
              name: a.aiUsageColSource,
              value: a.aiUsageColTokens,
              secondary: a.aiUsageColCalls,
            }}
            emptyText={a.aiUsageEmptySources}
            emptyIcon={<LuActivity size={18} />}
            loading={firstLoad}
            refreshing={refreshing}
          />
        </section>

        <div
          className="site-analytics-side-grid"
          role="group"
          aria-label={`${a.aiUsageByUser} / ${a.aiUsageByModel}`}
        >
          <section className="site-analytics-block" id="ai-usage-by-user">
            <h5 className="site-analytics-block-title">
              <span className="site-analytics-block-title-text">
                {a.aiUsageByUser}
              </span>
            </h5>
            <RankList
              rows={userRows}
              formatValue={count}
              headers={{
                name: a.aiUsageColUser,
                value: a.aiUsageColTokens,
                secondary: a.aiUsageColCalls,
              }}
              emptyText={a.aiUsageEmptyUsers}
              emptyIcon={<LuUsers size={18} />}
              loading={firstLoad}
              refreshing={refreshing}
            />
          </section>

          <section className="site-analytics-block" id="ai-usage-by-model">
            <h5 className="site-analytics-block-title">
              <span className="site-analytics-block-title-text">
                {a.aiUsageByModel}
              </span>
            </h5>
            <RankList
              rows={modelRows}
              formatValue={count}
              headers={{
                name: a.aiUsageColModel,
                value: a.aiUsageColTokens,
                secondary: a.aiUsageColCalls,
              }}
              emptyText={a.aiUsageEmptyModels}
              emptyIcon={<LuCpu size={18} />}
              loading={firstLoad}
              refreshing={refreshing}
            />
          </section>
        </div>
      </SettingGroup>
    </div>
  )
}

export default AiUsageSection
