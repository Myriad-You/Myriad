import type { ReactNode } from 'react'
import type { SettingHoverTooltipTone } from './useSettingHoverTooltip'
import { LuInfo } from '@lib/icons'
import React from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useSettingHoverTooltip } from './useSettingHoverTooltip'
import './SettingTitleHelp.css'

export type SettingTitleHelpTone = SettingHoverTooltipTone

export interface SettingTitleHelpProps {
  children: ReactNode
  ariaLabel?: string
  tone?: SettingTitleHelpTone
  placement?: 'top' | 'bottom'
  className?: string
}

export const SettingTitleHelp: React.FC<SettingTitleHelpProps> = ({
  children,
  ariaLabel: ariaLabelProp,
  tone = 'default',
  placement = 'bottom',
  className = '',
}) => {
  const { t } = useI18n()
  const ariaLabel = ariaLabelProp ?? t.config.detailHelpAria
  const { triggerRef, tooltip, open, show, hide, tooltipId } =
    useSettingHoverTooltip<HTMLButtonElement>({
      content: children,
      placement,
      tone,
    })

  if (children == null || children === false || children === '') return null

  return (
    <span
      className={[
        'setting-title-help',
        `setting-title-help--${tone}`,
        className,
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <button
        ref={triggerRef}
        type="button"
        className="setting-title-help-trigger"
        aria-label={ariaLabel}
        aria-describedby={open ? tooltipId : undefined}
        aria-expanded={open}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
      >
        <LuInfo className="setting-title-help-icon" aria-hidden />
      </button>
      {tooltip}
    </span>
  )
}

SettingTitleHelp.displayName = 'SettingTitleHelp'

export default SettingTitleHelp
