/**
 * 开关设置项组件
 */

import type { SwitchSettingConfig } from '../types'
import React, { useCallback } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { guideDomProps } from '../guides/guideAnchor'
import { SettingDefaultChangeTag } from '../SettingDefaultChangeTag'
import { useSettingsHelp } from '../SettingsHelpContext'
import { SettingTitleGuideEntry } from '../SettingTitleGuideEntry'
import { SettingTitleHelp } from '../SettingTitleHelp'
import { ToggleSwitch } from './ToggleSwitch'
import './SettingItem.css'

export interface SwitchItemProps extends Omit<SwitchSettingConfig, 'type'> {}

export const SwitchItem = React.memo<SwitchItemProps>(
  ({
    itemKey,
    label,
    detail,
    guide,
    guidePath,
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
    const { t } = useI18n()
    const expandHelp = Boolean(useSettingsHelp()?.showDetails)
    const detailText = detail != null && detail !== '' ? detail : null
    const anchorProps = guideDomProps(guidePath)

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
        {...anchorProps}
        className={`setting-item setting-item-switch setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
      >
        <div className="setting-item-content">
          <label htmlFor={id} className="setting-label">
            <span className="setting-label-text">
              {label}
              {detailText && !expandHelp && (
                <SettingTitleHelp
                  ariaLabel={t.config.detailHelpAriaNamed.replace(
                    '{title}',
                    label,
                  )}
                >
                  {detailText}
                </SettingTitleHelp>
              )}
              <SettingTitleGuideEntry title={label} guide={guide} />
              <SettingDefaultChangeTag
                fieldKey={itemKey}
                onApply={(next) => {
                  if (disabled || loading) return
                  const normalized = next.trim().toLowerCase()
                  onChange(
                    normalized === 'true' ||
                      normalized === '1' ||
                      normalized === 'yes',
                  )
                }}
              />
            </span>
            {description && (
              <span className="setting-description">{description}</span>
            )}
            {expandHelp && detailText && (
              <span className="setting-description setting-description--detail">
                {detailText}
              </span>
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
