import type { ReactNode } from 'react'
import type { BaseSettingItemConfig } from '../types'
import React from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { guideDomProps } from '../guides/guideAnchor'
import { SettingDefaultChangeTag } from '../SettingDefaultChangeTag'
import { SettingFieldErrorTag } from '../SettingFieldErrorTag'
import { useSettingsHelp } from '../SettingsHelpContext'
import { SettingTitleGuideEntry } from '../SettingTitleGuideEntry'
import { SettingTitleHelp } from '../SettingTitleHelp'
import './SettingItem.css'

export interface SettingItemWrapperProps extends Partial<BaseSettingItemConfig> {
  children: React.ReactNode
  className?: string
  id?: string
  contentRight?: boolean
  detail?: ReactNode
  guide?: ReactNode
  onApplyDefault?: (newDefault: string) => void
}

export const SettingItemWrapper: React.FC<SettingItemWrapperProps> = ({
  itemKey,
  label,
  detail,
  guide,
  guidePath,
  description,
  hint,
  error,
  required,
  layout = 'vertical',
  size = 'md',
  className = '',
  id,
  children,
  contentRight = false,
  disabled = false,
  onApplyDefault,
}) => {
  const anchorProps = guideDomProps(guidePath)
  const { t, format } = useI18n()
  const expandHelp = Boolean(useSettingsHelp()?.showDetails)
  const detailText = detail != null && detail !== '' ? detail : null
  const expandedExtra =
    expandHelp && detailText ? (
      <span className="setting-description setting-description--detail">
        {detailText}
      </span>
    ) : null

  const labelText = label && (
    <span className="setting-label-text">
      {label}
      {required && <span className="required">*</span>}
      {detailText && !expandHelp && (
        <SettingTitleHelp
          ariaLabel={format(t.config.detailHelpAriaNamed, {
            title: String(label),
          })}
        >
          {detailText}
        </SettingTitleHelp>
      )}
      <SettingTitleGuideEntry title={label} guide={guide} />
      <SettingDefaultChangeTag
        fieldKey={itemKey}
        onApply={onApplyDefault}
      />
      <SettingFieldErrorTag>{error}</SettingFieldErrorTag>
    </span>
  )

  const labelContent = label && (
    <div className="setting-label">
      {labelText}
      {description && (
        <span className="setting-description">{description}</span>
      )}
      {expandedExtra}
    </div>
  )

  if (layout === 'horizontal') {
    return (
      <div
        {...anchorProps}
        className={`setting-item setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
      >
        <div className="setting-item-content">
          {contentRight ? (
            <>
              {labelContent}
              <div className="setting-control">{children}</div>
            </>
          ) : (
            <>
              {labelContent}
              <div className="setting-control">{children}</div>
            </>
          )}
        </div>
        {hint && <p className="setting-hint">{hint}</p>}
      </div>
    )
  }

  return (
    <div
      {...anchorProps}
      className={`setting-item setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
    >
      {label && (
        <label htmlFor={id} className="setting-label">
          {labelText}
          {description && (
            <span className="setting-description">{description}</span>
          )}
          {expandedExtra}
        </label>
      )}

      <div className="setting-control">{children}</div>

      {hint && <p className="setting-hint">{hint}</p>}
      {!label && error ? (
        <SettingFieldErrorTag className="setting-field-error-tag--solo">
          {error}
        </SettingFieldErrorTag>
      ) : null}
    </div>
  )
}
