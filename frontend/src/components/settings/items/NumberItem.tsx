/**
 * 数字输入设置项组件
 */

import type { NumberSettingConfig } from '../types'
import React, { useCallback } from 'react'
import './SettingItem.css'

export interface NumberItemProps extends Omit<NumberSettingConfig, 'type'> {}

export const NumberItem = React.memo<NumberItemProps>(
  ({
    itemKey,
    label,
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
    layout = 'horizontal',
    min,
    max,
    step = 1,
    unit,
    className = '',
  }) => {
    const handleChange = useCallback(
      (e: React.ChangeEvent<HTMLInputElement>) => {
        if (!disabled && !loading) {
          const numValue = Number.parseFloat(e.target.value) || 0
          onChange(numValue)
        }
      },
      [onChange, disabled, loading],
    )

    const id = `setting-number-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`
    const inputName = `myriad-number-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`

    return (
      <div
        className={`setting-item setting-item-number setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}`}
      >
        <div className="setting-item-content">
          <label htmlFor={id} className="setting-label">
            <span className="setting-label-text">
              {label}
              {required && <span className="required">*</span>}
            </span>
            {description && (
              <span className="setting-description">{description}</span>
            )}
          </label>

          <div className="setting-control">
            <div className="number-input-wrapper">
              <input
                id={id}
                name={inputName}
                type="number"
                value={value}
                onChange={handleChange}
                onBlur={onBlur}
                min={min}
                max={max}
                step={step}
                disabled={disabled || loading}
                className={`field-input ${error ? 'has-error' : ''}`}
                aria-label={label}
                autoComplete="one-time-code"
                data-form-type="other"
                data-lpignore="true"
                data-1p-ignore="true"
              />
              {unit && <span className="number-unit">{unit}</span>}
            </div>
          </div>
        </div>
        {error && <p className="setting-error">{error}</p>}
        {hint && !error && <p className="setting-hint">{hint}</p>}
      </div>
    )
  },
)

NumberItem.displayName = 'NumberItem'
