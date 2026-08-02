/**
 * 访客统计 / AI 用量共用时间范围：7 · 14 · 30 · 自定义
 * 自定义：Portal 弹出双月历范围选择（DateRangePopover）
 */

import type { DateRangePopoverLabels } from '../../settings/DateRangePopover'
import type { AnalyticsRangePreset, AnalyticsRangeState } from './analyticsRangeLogic'
import { LuCalendar, LuChevronDown } from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { DateRangePopover, SegmentedControl } from '../../settings'
import {
  ANALYTICS_RANGE_PRESETS,
  analyticsRangeDayCount,

  defaultCustomRange,
  localIsoToday,
} from './analyticsRangeLogic'

export type {
  AnalyticsRangePreset,
  AnalyticsRangeState,
} from './analyticsRangeLogic'
export {
  ANALYTICS_RANGE_PRESETS,
  analyticsRangeDayCount,
  analyticsRangeQuery,
  defaultAnalyticsRange,
  defaultCustomRange,
} from './analyticsRangeLogic'

export interface AnalyticsRangeLabels {
  daysN: string
  custom: string
  rangeAria: string
  fromAria?: string
  toAria?: string
  customTitle?: string
  customHint?: string
  apply?: string
  clear?: string
  daysSelected?: string
  prevMonth?: string
  nextMonth?: string
  weekdays?: string[]
  monthTitle?: string
  today?: string
}

export interface AnalyticsRangePickerProps {
  value: AnalyticsRangeState
  onChange: (next: AnalyticsRangeState) => void
  labels: AnalyticsRangeLabels
  disabled?: boolean
  maxDate?: string
}

function shortRangeLabel(from: string, to: string): string {
  if (!from) return '—'
  if (!to || from === to) return from.slice(5)
  return `${from.slice(5)} – ${to.slice(5)}`
}

export const AnalyticsRangePicker: React.FC<AnalyticsRangePickerProps> = ({
  value,
  onChange,
  labels,
  disabled = false,
  maxDate,
}) => {
  const max = maxDate ?? localIsoToday()
  const anchorRef = useRef<HTMLDivElement>(null)
  const [open, setOpen] = useState(false)
  const openOnCustomRef = useRef(false)

  const segmentOptions = useMemo(
    () => [
      ...ANALYTICS_RANGE_PRESETS.map((d) => ({
        value: d as AnalyticsRangePreset,
        label: labels.daysN.replace('{n}', d),
      })),
      { value: 'custom' as const, label: labels.custom },
    ],
    [labels.daysN, labels.custom],
  )

  const onPreset = useCallback(
    (preset: AnalyticsRangePreset) => {
      if (preset === 'custom') {
        const fallback = defaultCustomRange(max)
        openOnCustomRef.current = true
        onChange({
          preset: 'custom',
          from: value.from || fallback.from,
          to: value.to || fallback.to,
        })
        setOpen(true)
        return
      }
      setOpen(false)
      onChange({ ...value, preset })
    },
    [onChange, value, max],
  )

  useEffect(() => {
    if (value.preset === 'custom' && openOnCustomRef.current) {
      openOnCustomRef.current = false
      setOpen(true)
    }
  }, [value.preset])

  const popoverLabels: DateRangePopoverLabels = useMemo(() => {
    const weekdays =
      labels.weekdays && labels.weekdays.length === 7
        ? labels.weekdays
        : ['一', '二', '三', '四', '五', '六', '日']
    const monthTpl = labels.monthTitle || '{y}-{m}'
    return {
      title: labels.customTitle || labels.custom,
      from: labels.fromAria || 'From',
      to: labels.toAria || 'To',
      apply: labels.apply || 'Apply',
      clear: labels.clear || 'Clear',
      hint: labels.customHint || '',
      weekdays,
      monthTitle: (y, m) =>
        monthTpl
          .replace('{y}', String(y))
          .replace('{m}', String(m).padStart(2, '0')),
      daysSelected: (n) =>
        (labels.daysSelected || '{n}d').replace('{n}', String(n)),
      prevMonth: labels.prevMonth || 'Previous month',
      nextMonth: labels.nextMonth || 'Next month',
      today: labels.today,
    }
  }, [labels])

  const onRangeCommit = useCallback(
    (next: { from: string; to: string }) => {
      onChange({
        preset: 'custom',
        from: next.from,
        to: next.to,
      })
    },
    [onChange],
  )

  const chipLabel = useMemo(() => {
    if (!value.from) return labels.custom
    const days = labels.daysN.replace(
      '{n}',
      String(analyticsRangeDayCount(value)),
    )
    return `${shortRangeLabel(value.from, value.to)} · ${days}`
  }, [value, labels.custom, labels.daysN])

  return (
    <div
      ref={anchorRef}
      className="site-analytics-scope-controls analytics-range-picker"
    >
      <SegmentedControl<AnalyticsRangePreset>
        size="sm"
        value={value.preset}
        onChange={onPreset}
        ariaLabel={labels.rangeAria}
        disabled={disabled}
        options={segmentOptions}
      />
      {value.preset === 'custom' ? (
        <button
          type="button"
          className="date-range-trigger-chip"
          disabled={disabled}
          aria-expanded={open}
          aria-haspopup="dialog"
          title={
            value.from && value.to
              ? `${value.from} → ${value.to}`
              : labels.custom
          }
          onClick={(e) => {
            e.stopPropagation()
            setOpen((v) => !v)
          }}
        >
          <span className="date-range-trigger-chip-icon" aria-hidden>
            <LuCalendar size={12} />
          </span>
          <span className="date-range-trigger-chip-text">{chipLabel}</span>
          <LuChevronDown
            className="date-range-trigger-chip-chevron"
            size={12}
            aria-hidden
          />
        </button>
      ) : null}

      <DateRangePopover
        open={open && value.preset === 'custom'}
        onOpenChange={setOpen}
        anchorRef={anchorRef}
        from={value.from}
        to={value.to}
        onChange={onRangeCommit}
        maxDate={max}
        maxSpanDays={365}
        labels={popoverLabels}
        disabled={disabled}
      />
    </div>
  )
}

export default AnalyticsRangePicker
