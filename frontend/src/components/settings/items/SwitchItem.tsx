/**
 * 开关设置项组件
 */

import type { SwitchSettingConfig } from '../types'
import React, { useCallback } from 'react'
import { ToggleSwitch } from './ToggleSwitch'
import './SettingItem.css'

export interface SwitchItemProps extends Omit<SwitchSettingConfig, 'type'> {}

export const SwitchItem = React.memo<SwitchItemProps>(
  ({
    itemKey,
    label,
    description,
    hint,
    value,
    onChange,
    disabled = false,
    loading = false,
    size = 'md',
    layout = 'horizontal',
    className = '',
  }) => {
    const handleChange = useCallback(
      (checked: boolean) => {
        if (!disabled && !loading) {
          onChange(checked)
        }
      },
      [onChange, disabled, loading],
    )

    const id = `setting-switch-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`

    return (
      <div
        className={`setting-item setting-item-switch setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}`}
      >
        <div className="setting-item-content">
          <label htmlFor={id} className="setting-label">
            <span className="setting-label-text">{label}</span>
            {description && (
              <span className="setting-description">{description}</span>
            )}
          </label>
          <div className="setting-control">
            <ToggleSwitch
              id={id}
              checked={value}
              onChange={handleChange}
              disabled={disabled || loading}
              aria-label={label}
            />
          </div>
        </div>
        {hint && <p className="setting-hint">{hint}</p>}
      </div>
    )
  },
)

SwitchItem.displayName = 'SwitchItem'
