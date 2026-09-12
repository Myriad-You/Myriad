import type { SelectSettingConfig } from '../types'
import React, { useCallback } from 'react'
import { guideDomProps } from '../guides/guideAnchor'
import { SettingDefaultChangeTag } from '../SettingDefaultChangeTag'
import { SettingFieldErrorTag } from '../SettingFieldErrorTag'
import { SettingTitleGuideEntry } from '../SettingTitleGuideEntry'
import { FieldSelect } from './FieldSelect'
import './SettingItem.css'

export interface SelectItemProps<T = string> extends Omit<
  SelectSettingConfig<T>,
  'type'
> {}

function SelectItemComponent<T extends string = string>({
  itemKey,
  label,
  guide,
  guidePath,
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
  const handleChange = useCallback(
    (next: T) => {
      if (!disabled && !loading) {
        onChange(next)
      }
    },
    [onChange, disabled, loading],
  )

  const id = `setting-select-${itemKey || label.replaceAll(/\s+/g, '-').toLowerCase()}`
  const anchorProps = guideDomProps(guidePath)

  return (
    <div
      {...anchorProps}
      className={`setting-item setting-item-select setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
    >
      <label htmlFor={id} className="setting-label">
        <span className="setting-label-text">
          {label}
          {required && <span className="required">*</span>}
          <SettingTitleGuideEntry title={label} guide={guide} />
          <SettingDefaultChangeTag
            fieldKey={itemKey}
            onApply={(next) => {
              if (!disabled && !loading) onChange(next as T)
            }}
          />
          <SettingFieldErrorTag>{error}</SettingFieldErrorTag>
        </span>
        {description && layout === 'vertical' && (
          <span className="setting-description">{description}</span>
        )}
      </label>

      <div className="setting-control">
        <FieldSelect
          id={id}
          value={value as T & string}
          options={options as Array<{ value: T & string; label: string; disabled?: boolean }>}
          onChange={handleChange as (v: T & string) => void}
          disabled={disabled || loading}
          className={error ? 'has-error' : ''}
          aria-label={label}
        />
        {hint && !error && <p className="setting-hint">{hint}</p>}
      </div>
    </div>
  )
}

export const SelectItem = React.memo(
  SelectItemComponent,
) as typeof SelectItemComponent
