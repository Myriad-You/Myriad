/**
 * 下拉选择设置项组件
 */

import type { SelectSettingConfig } from '../types'
import React, { useCallback } from 'react'
import './SettingItem.css'

export interface SelectItemProps<T = string> extends Omit<SelectSettingConfig<T>, 'type'> {}

function SelectItemComponent<T extends string = string>({
  itemKey,
  label,
  description,
  hint,
  value,
  onChange,
  options,
  disabled = false,
  loading = false,
  required = false,
  error,
  size = 'md',
  layout = 'vertical',
  className = '',
}: SelectItemProps<T>) {
  const handleChange = useCallback((e: React.ChangeEvent<HTMLSelectElement>) => {
    if (!disabled && !loading) {
      onChange(e.target.value as T)
    }
  }, [onChange, disabled, loading])

  const id = `setting-select-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`

  return (
    <div
      className={`setting-item setting-item-select setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}`}
    >
      <label htmlFor={id} className="setting-label">
        <span className="setting-label-text">
          {label}
          {required && <span className="required">*</span>}
        </span>
        {description && layout === 'vertical' && (
          <span className="setting-description">{description}</span>
        )}
      </label>

      <div className="setting-control">
        <select
          id={id}
          value={value as string}
          onChange={handleChange}
          disabled={disabled || loading}
          className={`field-select ${error ? 'has-error' : ''}`}
        >
          {options.map(option => (
            <option
              key={String(option.value)}
              value={option.value as string}
              disabled={option.disabled}
            >
              {option.label}
            </option>
          ))}
        </select>
        {error && <p className="setting-error">{error}</p>}
        {hint && !error && <p className="setting-hint">{hint}</p>}
      </div>
    </div>
  )
}

export const SelectItem = React.memo(SelectItemComponent) as typeof SelectItemComponent
