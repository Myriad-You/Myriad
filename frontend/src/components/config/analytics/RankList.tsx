import type { ReactNode, UIEvent } from 'react'
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { EmptyCard } from './EmptyCard'

export interface RankSubRow {
  key: string
  name: string
  value: number
  secondary?: string
}

export interface RankRow {
  key: string
  name: string
  meta?: string
  value: number
  secondary?: string
  tertiary?: string
  subRows?: RankSubRow[]
}

interface RankListProps {
  rows: RankRow[]
  formatValue: (n: number) => string
  headers: {
    name: string
    value: string
    secondary?: string
    tertiary?: string
  }
  emptyText: string
  emptyIcon?: ReactNode
  loading?: boolean
  refreshing?: boolean
  initialCount?: number
}

export const DEFAULT_RANK_LOAD_COUNT = 30
const SCROLL_LOAD_THRESHOLD_PX = 32

export function nextRankVisibleCount(
  current: number,
  total: number,
): number {
  if (total <= 0) return 0
  if (current >= total) return total
  return total
}

export function clampRankInitialLoad(
  initialCount: unknown,
  total: number,
): number {
  const raw = Math.floor(Number(initialCount))
  const page =
    Number.isFinite(raw) && raw > 0 ? raw : DEFAULT_RANK_LOAD_COUNT
  if (total <= 0) return 0
  return Math.min(total, page)
}

function rowsWindowKey(rows: RankRow[]): string {
  if (rows.length === 0) return '0'
  return `${rows.length}:${rows[0]?.key ?? ''}:${rows.at(-1)?.key ?? ''}`
}

export const RankList: React.FC<RankListProps> = ({
  rows,
  formatValue,
  headers,
  emptyText,
  emptyIcon,
  loading = false,
  refreshing = false,
  initialCount = DEFAULT_RANK_LOAD_COUNT,
}) => {
  const listRef = useRef<HTMLUListElement>(null)
  const windowKey = rowsWindowKey(rows)
  const [visibleCount, setVisibleCount] = useState(() =>
    clampRankInitialLoad(initialCount, rows.length),
  )

  useEffect(() => {
    setVisibleCount(clampRankInitialLoad(initialCount, rows.length))
  }, [windowKey, initialCount, rows.length])

  const max = useMemo(
    () => Math.max(1, ...rows.map((r) => r.value)),
    [rows],
  )
  const visible = rows.slice(0, visibleCount)
  const hasMore = visibleCount < rows.length

  const loadRemaining = useCallback(() => {
    setVisibleCount((cur) => nextRankVisibleCount(cur, rows.length))
  }, [rows.length])

  const onListScroll = useCallback(
    (event: UIEvent<HTMLUListElement>) => {
      if (!hasMore) return
      const el = event.currentTarget
      if (
        el.scrollTop + el.clientHeight >=
        el.scrollHeight - SCROLL_LOAD_THRESHOLD_PX
      ) {
        loadRemaining()
      }
    },
    [hasMore, loadRemaining],
  )

  useLayoutEffect(() => {
    if (!hasMore) return
    const el = listRef.current
    if (!el) return
    if (el.scrollHeight <= el.clientHeight + 1) {
      loadRemaining()
    }
  }, [hasMore, visibleCount, loadRemaining])

  if (rows.length === 0) {
    return <EmptyCard text={emptyText} icon={emptyIcon} loading={loading} />
  }

  const hasSecondary = Boolean(headers.secondary)
  const hasTertiary = Boolean(headers.tertiary) && rows.some((r) => r.tertiary)

  const mods = [
    refreshing ? 'is-refreshing' : '',
    hasSecondary ? 'has-secondary' : '',
    hasTertiary ? 'has-tertiary' : '',
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <div className={`site-analytics-rank${mods ? ` ${mods}` : ''}`}>
      <div className="site-analytics-rank-head" aria-hidden>
        <span className="site-analytics-rank-h-name">{headers.name}</span>
        <span className="site-analytics-rank-h-track" />
        <span className="site-analytics-rank-h-value">{headers.value}</span>
        {hasSecondary ? (
          <span className="site-analytics-rank-h-second">{headers.secondary}</span>
        ) : null}
        {hasTertiary ? (
          <span className="site-analytics-rank-h-third">{headers.tertiary}</span>
        ) : null}
      </div>

      <ul
        ref={listRef}
        className="site-analytics-rank-list"
        onScroll={onListScroll}
      >
        {visible.map((row) => {
          const main = formatValue(row.value)
          const aria = [
            row.name,
            headers.value ? `${headers.value} ${main}` : main,
            hasSecondary && row.secondary
              ? `${headers.secondary} ${row.secondary}`
              : null,
            hasTertiary && headers.tertiary && row.tertiary
              ? `${headers.tertiary} ${row.tertiary}`
              : null,
          ]
            .filter(Boolean)
            .join(' · ')
          const sub = row.subRows?.filter((s) => s.value > 0) ?? []
          return (
            <li key={row.key} className="site-analytics-rank-item">
              <div className="site-analytics-rank-row" aria-label={aria}>
                <div className="site-analytics-rank-label">
                  <span className="site-analytics-rank-name">{row.name}</span>
                  {row.meta ? (
                    <span className="site-analytics-rank-meta" title={row.meta}>
                      {row.meta}
                    </span>
                  ) : null}
                </div>

                <div className="site-analytics-rank-track" aria-hidden>
                  <div
                    className="site-analytics-rank-bar"
                    style={{
                      width: `${Math.max(1.5, (row.value / max) * 100)}%`,
                    }}
                  />
                </div>

                <span className="site-analytics-rank-value">{main}</span>
                {hasSecondary ? (
                  <span className="site-analytics-rank-second">
                    {row.secondary ?? '—'}
                  </span>
                ) : null}
                {hasTertiary ? (
                  <span className="site-analytics-rank-third">
                    {row.tertiary ?? '—'}
                  </span>
                ) : null}
              </div>
              {sub.length > 0 ? (
                <ul className="site-analytics-rank-sublist">
                  {sub.map((s) => {
                    const sMain = formatValue(s.value)
                    return (
                      <li
                        key={s.key}
                        className="site-analytics-rank-row site-analytics-rank-row--sub"
                        aria-label={`${row.name} · ${s.name} · ${sMain}`}
                      >
                        <div className="site-analytics-rank-label">
                          <span className="site-analytics-rank-name">
                            {s.name}
                          </span>
                        </div>
                        <div className="site-analytics-rank-track" aria-hidden>
                          <div
                            className="site-analytics-rank-bar"
                            style={{
                              width: `${Math.max(1.5, (s.value / max) * 100)}%`,
                            }}
                          />
                        </div>
                        <span className="site-analytics-rank-value">
                          {sMain}
                        </span>
                        {hasSecondary ? (
                          <span className="site-analytics-rank-second">
                            {s.secondary ?? '—'}
                          </span>
                        ) : null}
                        {hasTertiary ? (
                          <span className="site-analytics-rank-third">—</span>
                        ) : null}
                      </li>
                    )
                  })}
                </ul>
              ) : null}
            </li>
          )
        })}
      </ul>
    </div>
  )
}

export default RankList
