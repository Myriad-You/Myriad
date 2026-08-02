/**
 * 滑动条设置项
 * - 粗轨道 + 主色进度；当前值浮在拇指上方
 * - 刻度强制显示：沿轨道均匀分配，最多 11 个，与 step 解耦
 * - 两端可显示对应数值 + 可选 startLabel / endLabel 说明
 * - 可选 recommendedValue：进度条下方推荐标记
 * - 宽度按可移动间隔自适应
 *
 * 几何约定（刻度 / 两端数值 / 进度填充 / 推荐标记共用）：
 * 全部落在同一 `.slider-geometry` 宽度内，用同一 CSS 行程公式
 *   left/width = thumb/2 + pct/100 * (100% − thumb)
 * 避免用 input 实测像素去定位兄弟层（两者宽度常不一致）。
 */

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
/** 刻度上限：与步进无关，最多 11 个（含两端） */
const MAX_TICKS = 11
/** 与 CSS `--slider-thumb` 一致（border-box，含边框） */
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
  return value.toFixed(decimals).replace(/\.?0+$/, '')
}

/** 轨道宽度仍可参考步进间隔，让短区间更短 */
function trackWidthRem(stepIntervals: number): number {
  const n = Math.max(1, stepIntervals)
  const segment =
    n <= 6 ? 2.45 : n <= 10 ? 2.05 : n <= 16 ? 1.75 : n <= 32 ? 1.35 : 1.05
  return clamp(THUMB_REM + n * segment, TRACK_MIN_REM, TRACK_MAX_REM)
}

/**
 * 动态分配刻度数量（与 step 无关）：
 * - 始终至少 2（两端）
 * - 最多 MAX_TICKS（11，覆盖常见 0–10 共 11 档）
 * - 按轨道视觉长度略作增减，短条少刻、长条多刻
 */
function allocateTickCount(widthRem: number): number {
  // ~每 2rem 一档，夹在 2…11
  const byWidth = Math.round(widthRem / 2)
  return clamp(byWidth, 2, MAX_TICKS)
}

/** 在 0–100% 上均匀铺开 tickCount 个点（含 0% 与 100%） */
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

    /** 仅用于轨道宽度估算；刻度不依赖它 */
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

    const id = `setting-slider-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`
    const inputName = `myriad-slider-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`
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
            {/*
              几何盒：刻度 / 两端数值 / 轨道 / 推荐标记 共享同一宽度。
              定位全部用 CSS 行程公式，不再用 input 实测 px。
            */}
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
                {/*
                  填充右缘 = 拇指中心行程；气泡挂在填充末端 translateX(50%)，
                  与刻度同一套 CSS 公式，天然对齐进度数值。
                */}
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

              {/* 轨道下方：两侧说明与推荐底部对齐 */}
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
