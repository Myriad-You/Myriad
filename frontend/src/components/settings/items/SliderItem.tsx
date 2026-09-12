import type { SliderSettingConfig } from '../types'
import React, { useCallback, useMemo, useState } from 'react'
import { guideDomProps } from '../guides/guideAnchor'
import { SettingDefaultChangeTag } from '../SettingDefaultChangeTag'
import { SettingFieldErrorTag } from '../SettingFieldErrorTag'
import { SettingTitleGuideEntry } from '../SettingTitleGuideEntry'
import './SettingItem.css'

export interface SliderItemProps extends Omit<SliderSettingConfig, 'type'> {}

const THUMB_REM = 1.35
const TRACK_MIN_REM = 9
const TRACK_MAX_REM = 26
const MAX_TICKS = 11
const THUMB_PX = 20

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n))
}

function defaultFormat(value: number, step: number): string {
  if (!Number.isFinite(value)) return '0'
  if (step >= 1) return String(Math.round(value))
  const decimals = Math.min(
    4,
    (String(step).split('.')[1] || '').length || 1,
  )
  return value.toFixed(decimals).replaceAll(/\.?0+$/g, '')
}

function trackWidthRem(stepIntervals: number): number {
  const n = Math.max(1, stepIntervals)
  const segment =
    n <= 6 ? 2.45 : n <= 10 ? 2.05 : n <= 16 ? 1.75 : n <= 32 ? 1.35 : 1.05
  return clamp(THUMB_REM + n * segment, TRACK_MIN_REM, TRACK_MAX_REM)
}

function allocateTickCount(widthRem: number): number {
  const byWidth = Math.round(widthRem / 2)
  return clamp(byWidth, 2, MAX_TICKS)
}

function buildEvenTicks(tickCount: number): {
  p: number
  end: boolean
  index: number
}[] {
  const n = Math.max(2, Math.min(MAX_TICKS, Math.round(tickCount)))
  if (n === 1) {
    return [{ p: 0, end: true, index: 0 }]
  }
  return Array.from({ length: n }, (_, i) => ({
    p: (i / (n - 1)) * 100,
    end: i === 0 || i === n - 1,
    index: i,
  }))
}

export const SliderItem = React.memo<SliderItemProps>(
  ({
    itemKey,
    label,
    guide,
    guidePath,
    description,
    hint,
    value,
    onChange,
    onBlur,
    disabled = false,
    loading = false,
    required = false,
    error,
    size = 'md',
    layout = 'vertical',
    min = 0,
    max = 100,
    step = 1,
    unit,
    showValue = true,
    formatValue,
    showRangeLabels = false,
    startLabel,
    endLabel,
    recommendedValue,
    recommendedLabel,
    className = '',
  }) => {
    const [active, setActive] = useState(false)
    const busy = disabled || loading
    const safeMin = Number.isFinite(min) ? min : 0
    const safeMax = Number.isFinite(max) && max > safeMin ? max : safeMin + 1
    const safeStep = step > 0 ? step : 1
    const clamped = clamp(
      Number.isFinite(value) ? value : safeMin,
      safeMin,
      safeMax,
    )

    const stepIntervals = useMemo(() => {
      const n = (safeMax - safeMin) / safeStep
      if (!Number.isFinite(n) || n <= 0) return 1
      return Math.max(1, Math.round(n))
    }, [safeMax, safeMin, safeStep])

    const widthRem = useMemo(
      () => trackWidthRem(stepIntervals),
      [stepIntervals],
    )

    const ticks = useMemo(() => {
      if (!(safeMax > safeMin)) return []
      const count = allocateTickCount(widthRem)
      return buildEvenTicks(count)
    }, [safeMax, safeMin, widthRem])

    const pct = useMemo(() => {
      if (safeMax === safeMin) return 0
      return ((clamped - safeMin) / (safeMax - safeMin)) * 100
    }, [clamped, safeMax, safeMin])

    const formatNum = useCallback(
      (n: number) => {
        const text = formatValue ? formatValue(n) : defaultFormat(n, safeStep)
        return unit ? `${text}${unit}` : text
      },
      [formatValue, safeStep, unit],
    )

    const display = useMemo(() => formatNum(clamped), [clamped, formatNum])

    const startHint = startLabel?.trim() || ''
    const endHint = endLabel?.trim() || ''
    const minText = formatNum(safeMin)
    const maxText = formatNum(safeMax)

    const showEnds = showRangeLabels || Boolean(startHint) || Boolean(endHint)

    const recommended = useMemo(() => {
      if (
        recommendedValue === undefined ||
        !Number.isFinite(recommendedValue)
      ) {
        return null
      }
      if (recommendedValue < safeMin || recommendedValue > safeMax) {
        return null
      }
      const span = safeMax - safeMin
      const p = span <= 0 ? 0 : ((recommendedValue - safeMin) / span) * 100
      return {
        pct: p,
        label: recommendedLabel?.trim() || '',
        text: formatNum(recommendedValue),
      }
    }, [
      formatNum,
      recommendedLabel,
      recommendedValue,
      safeMax,
      safeMin,
    ])

    const handleChange = useCallback(
      (e: React.ChangeEvent<HTMLInputElement>) => {
        if (busy) return
        const next = Number.parseFloat(e.target.value)
        if (!Number.isFinite(next)) return
        onChange(clamp(next, safeMin, safeMax))
      },
      [busy, onChange, safeMax, safeMin],
    )

    const endActive = useCallback(() => setActive(false), [])

    const id = `setting-slider-${itemKey || label.replaceAll(/\s+/g, '-').toLowerCase()}`
    const inputName = `myriad-slider-${itemKey || label.replaceAll(/\s+/g, '-').toLowerCase()}`
    const anchorProps = guideDomProps(guidePath)

    return (
      <div
        {...anchorProps}
        className={`setting-item setting-item-slider setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
      >
        <label htmlFor={id} className="setting-label">
          <span className="setting-label-text">
            {label}
            {required && <span className="required">*</span>}
            <SettingTitleGuideEntry title={label} guide={guide} />
            <SettingDefaultChangeTag
              fieldKey={itemKey}
              onApply={(next) => {
                if (disabled || loading) return
                const n = Number(next)
                if (!Number.isNaN(n)) onChange(n)
              }}
            />
            <SettingFieldErrorTag>{error}</SettingFieldErrorTag>
          </span>
          {description && layout === 'vertical' && (
            <span className="setting-description">{description}</span>
          )}
        </label>

        <div className="setting-control">
          <div
            className={[
              'slider-control',
              showValue ? 'has-value' : '',
              showEnds ? 'has-range' : '',
              ticks.length > 0 ? 'has-ticks' : '',
              recommended ? 'has-recommended' : '',
              active ? 'is-active' : '',
              busy ? 'is-busy' : '',
              error ? 'has-error' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            style={
              {
                '--slider-pct': String(pct),
                '--slider-track-w': `${widthRem}rem`,
                '--slider-thumb-px': String(THUMB_PX),
                '--slider-intervals': String(stepIntervals),
              } as React.CSSProperties
            }
            data-intervals={stepIntervals}
            data-ticks={ticks.length}
          >
            <div className="slider-geometry">
              {showEnds && (
                <div className="slider-range-nums" aria-hidden>
                  <span className="slider-range-num is-start">{minText}</span>
                  <span className="slider-range-num is-end">{maxText}</span>
                </div>
              )}

              {ticks.length > 0 && (
                <div className="slider-ticks" aria-hidden>
                  {ticks.map((t) => (
                    <span
                      key={t.index}
                      className={[
                        'slider-tick',
                        t.end ? 'is-end' : '',
                        t.p <= pct + 0.01 ? 'is-passed' : '',
                      ]
                        .filter(Boolean)
                        .join(' ')}
                      style={
                        {
                          '--tick-p': String(t.p),
                        } as React.CSSProperties
                      }
                    />
                  ))}
                </div>
              )}

              <div className="slider-track-wrap">
                <div className="slider-track-fill" aria-hidden>
                  {showValue ? (
                    <span className="slider-value" aria-hidden>
                      <span className="slider-value-text">{display}</span>
                    </span>
                  ) : null}
                </div>
                <input
                  id={id}
                  name={inputName}
                  type="range"
                  min={safeMin}
                  max={safeMax}
                  step={safeStep}
                  value={clamped}
                  onChange={handleChange}
                  onBlur={() => {
                    endActive()
                    onBlur?.()
                  }}
                  onPointerDown={() => {
                    if (!busy) setActive(true)
                  }}
                  onPointerUp={endActive}
                  onPointerCancel={endActive}
                  onKeyDown={() => {
                    if (!busy) setActive(true)
                  }}
                  onKeyUp={endActive}
                  disabled={busy}
                  className="field-slider"
                  aria-label={label}
                  aria-valuemin={safeMin}
                  aria-valuemax={safeMax}
                  aria-valuenow={clamped}
                  aria-valuetext={display}
                />
              </div>

              {(recommended ||
                (showEnds && (startHint || endHint))) && (
                <div className="slider-footer">
                  {showEnds && (startHint || endHint) && (
                    <div className="slider-range-hints">
                      <span className="slider-range-hint is-start">
                        {startHint || '\u00A0'}
                      </span>
                      <span className="slider-range-hint is-end">
                        {endHint || '\u00A0'}
                      </span>
                    </div>
                  )}
                  {recommended && (
                    <div
                      className="slider-recommended"
                      style={
                        {
                          '--slider-rec-pct': String(recommended.pct),
                        } as React.CSSProperties
                      }
                      title={
                        recommended.label
                          ? `${recommended.label} ${recommended.text}`
                          : recommended.text
                      }
                    >
                      <span className="slider-recommended-pin" aria-hidden />
                      <span className="slider-recommended-label">
                        {recommended.label || recommended.text}
                        {recommended.label ? (
                          <span className="slider-recommended-val">
                            {recommended.text}
                          </span>
                        ) : null}
                      </span>
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>

          {hint && !error && <p className="setting-hint">{hint}</p>}
        </div>
      </div>
    )
  },
)

SliderItem.displayName = 'SliderItem'
