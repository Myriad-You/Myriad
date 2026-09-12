import type { CheckboxSettingConfig } from '../types'
import React, { useCallback } from 'react'
import './SettingItem.css'

export interface CheckboxItemProps extends Omit<
  CheckboxSettingConfig,
  'type'
> {}

export const CheckboxItem = React.memo<CheckboxItemProps>(
  ({
    itemKey,
    label,
    description,
    hint,
    value,
    onChange,
    checkboxLabel,
    disabled = false,
    loading = false,
    size = 'md',
    layout = 'vertical',
    className = '',
  }) => {
    const handleChange = useCallback(
      (e: React.ChangeEvent<HTMLInputElement>) => {
        if (!disabled && !loading) {
          onChange(e.target.checked)
        }
      },
      [onChange, disabled, loading],
    )

    const id = `setting-checkbox-${itemKey || label.replaceAll(/\s+/g, '-').toLowerCase()}`

    return (
      <div
        className={`setting-item setting-item-checkbox setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}`}
      >
        <label htmlFor={id} className="setting-label">
          <span className="setting-label-text">{label}</span>
        </label>

        <div className="setting-control">
          <div className="checkbox-wrapper">
            <input
              id={id}
              type="checkbox"
              checked={value}
              onChange={handleChange}
              disabled={disabled || loading}
              className="field-checkbox"
            />
            {checkboxLabel && (
              <span className="checkbox-hint">{checkboxLabel}</span>
            )}
          </div>
          {description && <p className="setting-description">{description}</p>}
          {hint && <p className="setting-hint">{hint}</p>}
        </div>
      </div>
    )
  },
)

CheckboxItem.displayName = 'CheckboxItem'
