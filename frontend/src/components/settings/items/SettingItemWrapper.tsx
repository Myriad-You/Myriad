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
  /** 表单控件 id（htmlFor）；锚点请用 guidePath */
  id?: string
  contentRight?: boolean
  /** 覆盖 BaseSettingItemConfig.detail */
  detail?: ReactNode
  /** 覆盖 BaseSettingItemConfig.guide */
  guide?: ReactNode
  /** 点击「应用新默认」时写入选项值（字符串默认） */
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
  const { t } = useI18n()
  const expandHelp = Boolean(useSettingsHelp()?.showDetails)
  const detailText = detail != null && detail !== '' ? detail : null
  /** 短说明常显；guide 点入口后在标题上方展开 */
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
          ariaLabel={t.config.detailHelpAriaNamed.replace(
            '{title}',
            String(label),
          )}
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
      {/* 无 label 时无法贴标题，退回行下报错 */}
      {!label && error ? (
        <SettingFieldErrorTag className="setting-field-error-tag--solo">
          {error}
        </SettingFieldErrorTag>
      ) : null}
    </div>
  )
}
