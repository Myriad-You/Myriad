import type { ReactNode } from 'react'
import type { SettingOption } from './types'
import { FieldSelect } from './items/FieldSelect'
import './SettingTitleSelect.css'

export interface SettingTitleSelectProps<T extends string = string> {
  value: T
  options: SettingOption<T>[]
  onChange: (value: T) => void
  icon?: ReactNode
  label?: string
  'aria-label'?: string
  disabled?: boolean
  className?: string
  variant?: 'title' | 'field'
  searchable?: boolean
  searchPlaceholder?: string
  emptySearchText?: string
}

export function SettingTitleSelect<T extends string = string>({
  value,
  options,
  onChange,
  icon,
  label,
  'aria-label': ariaLabel,
  disabled = false,
  className = '',
  variant = 'title',
  searchable = false,
  searchPlaceholder,
  emptySearchText,
}: SettingTitleSelectProps<T>) {
  const classes = [
    'setting-title-select',
    variant === 'title'
      ? 'setting-title-select--title'
      : 'setting-title-select--field',
    className,
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <span
      className={classes}
      onClick={(e) => {
        // stop filter clicks from toggling a button heading
        e.stopPropagation()
      }}
      onKeyDown={(e) => e.stopPropagation()}
    >
      {icon ? (
        <span className="setting-title-select-icon" aria-hidden>
          {icon}
        </span>
      ) : null}
      {label ? (
        <span className="setting-title-select-prefix">{label}</span>
      ) : null}
      <FieldSelect
        size="sm"
        value={value}
        options={options}
        onChange={onChange}
        disabled={disabled}
        aria-label={ariaLabel || label}
        className="setting-title-select-field"
        searchable={searchable}
        searchPlaceholder={searchPlaceholder}
        emptySearchText={emptySearchText}
      />
    </span>
  )
}

SettingTitleSelect.displayName = 'SettingTitleSelect'
