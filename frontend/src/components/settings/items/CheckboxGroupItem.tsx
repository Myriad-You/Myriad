import type { ReactNode } from 'react'
import React, { useCallback } from 'react'
import { guideDomProps } from '../guides/guideAnchor'
import { SettingTitleGuideEntry } from '../SettingTitleGuideEntry'
import { CheckboxCard } from './CheckboxCard'
import './SettingItem.css'

export interface CheckboxGroupOption {
  key: string
  label: string
  description?: string
  icon?: React.ReactNode
  value: boolean
}

export interface CheckboxGroupItemProps {
  label: string
  description?: string
  guide?: ReactNode
  guidePath?: string
  hint?: string
  options: CheckboxGroupOption[]
  onChange: (key: string, value: boolean) => void
  disabled?: boolean
  className?: string
}

export const CheckboxGroupItem = React.memo<CheckboxGroupItemProps>(
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
      (key: string) => (value: boolean) => {
        if (!disabled) onChange(key, value)
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
        <div className="checkbox-group-options">
          {options.map((option) => (
            <CheckboxCard
              key={option.key}
              label={option.label}
              checked={option.value}
              onChange={handleChange(option.key)}
              description={option.description}
              icon={option.icon}
              disabled={disabled}
            />
          ))}
        </div>
        {hint && <p className="setting-hint">{hint}</p>}
      </div>
    )
  },
)

CheckboxGroupItem.displayName = 'CheckboxGroupItem'
