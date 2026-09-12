import type { ProviderSettingConfig, SettingOption } from '../types'
import React, { useCallback } from 'react'
import { guideDomProps } from '../guides/guideAnchor'
import { SettingTitleGuideEntry } from '../SettingTitleGuideEntry'
import './SettingItem.css'

export interface ProviderItemProps<T = string> extends Omit<
  ProviderSettingConfig<T>,
  'type'
> {}

function ProviderItemComponent<T extends string = string>({
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
  allowDeselect = true,
  size = 'md',
  layout = 'horizontal',
  className = '',
}: ProviderItemProps<T>) {
  const handleSelect = useCallback(
    (optionValue: T) => {
      if (disabled || loading) return
      if (allowDeselect && value === optionValue) {
        onChange('' as T)
        return
      }
      onChange(optionValue)
    },
    [allowDeselect, onChange, disabled, loading, value],
  )

  const renderIcon = (icon: SettingOption['icon']) => {
    if (!icon) return null
    if (typeof icon === 'string') {
      return <span className="provider-icon">{icon}</span>
    }
    return <span className="provider-icon">{icon}</span>
  }

  const anchorProps = guideDomProps(guidePath)

  return (
    <div
      {...anchorProps}
      className={`setting-item setting-item-provider setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
    >
      <div className="setting-item-content">
        <div className="setting-label">
          <span className="setting-label-text">
            {label}
            <SettingTitleGuideEntry title={label} guide={guide} />
          </span>
          {description && (
            <span className="setting-description">{description}</span>
          )}
        </div>

        <div className="setting-control">
          <div className="provider-selector" role="group">
            {options.map((option) => {
              const selected = value === option.value
              return (
                <button
                  key={String(option.value)}
                  type="button"
                  onClick={() => handleSelect(option.value as T)}
                  disabled={disabled || loading || option.disabled}
                  aria-pressed={selected}
                  className={`provider-option ${selected ? 'active' : ''}`}
                >
                  {renderIcon(option.icon)}
                  <span className="provider-name">{option.label}</span>
                  {option.badge && (
                    <span className="provider-badge">{option.badge}</span>
                  )}
                </button>
              )
            })}
          </div>
        </div>
      </div>
      {hint && <p className="setting-hint">{hint}</p>}
    </div>
  )
}

export const ProviderItem = React.memo(
  ProviderItemComponent,
) as typeof ProviderItemComponent
