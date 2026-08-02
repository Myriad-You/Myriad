/**
 * 设置页右上角「显示说明」：与重置本页同款 CheckboxCard（标题 + 介绍 + 图标）
 */

import { LuInfo } from '@lib/icons'
import React from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { CheckboxCard } from './items/CheckboxCard'
import './SettingsHelpToggle.css'

export interface SettingsHelpToggleProps {
  checked: boolean
  onChange: (checked: boolean) => void
  'aria-label'?: string
  className?: string
}

export const SettingsHelpToggle: React.FC<SettingsHelpToggleProps> = ({
  checked,
  onChange,
  'aria-label': ariaLabel,
  className = '',
}) => {
  const { t } = useI18n()
  const title = t.config.showHelpDetails
  const description = t.config.showHelpDetailsDesc

  return (
    <CheckboxCard
      variant="switch"
      label={title}
      description={description}
      icon={<LuInfo />}
      showIndicator={false}
      checked={checked}
      onChange={onChange}
      title={description}
      aria-label={ariaLabel ?? title}
      className={['settings-help-toggle', className].filter(Boolean).join(' ')}
    />
  )
}

SettingsHelpToggle.displayName = 'SettingsHelpToggle'

export default SettingsHelpToggle
