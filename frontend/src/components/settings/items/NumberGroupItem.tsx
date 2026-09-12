import type { ReactNode } from 'react'
import React, { useCallback } from 'react'
import { guideDomProps } from '../guides/guideAnchor'
import { SettingTitleGuideEntry } from '../SettingTitleGuideEntry'

import './SettingItem.css'

export interface NumberGroupOption {
  key: string
  label: string
  description?: string
  value: number
  min?: number
  max?: number
  step?: number
  unit?: string
}

export interface NumberGroupItemProps {
  label: string
  description?: string
  guide?: ReactNode
  guidePath?: string
  hint?: string
  options: NumberGroupOption[]
  onChange: (key: string, value: number) => void
  disabled?: boolean
  className?: string
}

export const NumberGroupItem = React.memo<NumberGroupItemProps>(
  ({
    label,
    description,
    guide,
    guidePath,
    hint,
    options,
    onChange,
    disabled = false,
    className = '',
  }) => {
    const handleChange = useCallback(
      (key: string) => (e: React.ChangeEvent<HTMLInputElement>) => {
        if (disabled) return
        const raw = e.target.value.trim()
        if (raw === '') {
          onChange(key, 0)
          return
        }
        const numValue = Number.parseFloat(raw)
        if (Number.isFinite(numValue)) onChange(key, numValue)
      },
      [onChange, disabled],
    )

    const showLabel = Boolean(label) || Boolean(description) || Boolean(guide)
    const anchorProps = guideDomProps(guidePath)

    return (
      <div
        {...anchorProps}
        className={`setting-item setting-vertical ${className} ${disabled ? 'disabled' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
      >
        {showLabel && (
          <div className="setting-label">
            {label ? (
              <span className="setting-label-text">
                {label}
                <SettingTitleGuideEntry title={label} guide={guide} />
              </span>
            ) : (
              <SettingTitleGuideEntry title={label} guide={guide} />
            )}
            {description && (
              <span className="setting-description">{description}</span>
            )}
          </div>
        )}
        <div className="number-group-options">
          {options.map((option) => (
            <div key={option.key} className="number-group-card">
              <span className="number-group-card-label">{option.label}</span>
              {option.description && (
                <span className="number-group-card-desc">
                  {option.description}
                </span>
              )}
              <div className="number-group-card-input">
                <input
                  type="number"
                  value={option.value}
                  onChange={handleChange(option.key)}
                  min={option.min}
                  max={option.max}
                  step={option.step ?? 1}
                  disabled={disabled}
                  className="field-input"
                  aria-label={option.label}
                  autoComplete="one-time-code"
                  data-form-type="other"
                  data-lpignore="true"
                  data-1p-ignore="true"
                />
                {option.unit && (
                  <span className="number-group-card-unit">{option.unit}</span>
                )}
              </div>
            </div>
          ))}
        </div>
        {hint && <p className="setting-hint">{hint}</p>}
      </div>
    )
  },
)

NumberGroupItem.displayName = 'NumberGroupItem'
