import type { SettingDefaultChangeNotice } from './settingDefaultChanges'
import { LuSparkles } from '@lib/icons'
import React, { useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  dismissSettingDefaultChange,
  getSettingDefaultChangeNotice,

  subscribeSettingDefaultChanges,
} from './settingDefaultChanges'
import { SettingTitleTag } from './SettingTitleTag'

export interface SettingDefaultChangeTagProps {
  fieldKey?: string
  /** click applies the new default; omit for display-only */
  onApply?: (newDefault: string) => void
  className?: string
}

export function SettingDefaultChangeTag({
  fieldKey,
  onApply,
  className = '',
}: SettingDefaultChangeTagProps) {
  const { t, format } = useI18n()
  const [notice, setNotice] = useState<SettingDefaultChangeNotice | null>(
    () => getSettingDefaultChangeNotice(fieldKey),
  )

  useEffect(() => {
    const sync = () => setNotice(getSettingDefaultChangeNotice(fieldKey))
    sync()
    return subscribeSettingDefaultChanges(sync)
  }, [fieldKey])

  const handleDismiss = useCallback(() => {
    dismissSettingDefaultChange(fieldKey)
  }, [fieldKey])

  const handleApply = useCallback(() => {
    if (!notice) return
    onApply?.(notice.to)
    dismissSettingDefaultChange(fieldKey)
  }, [fieldKey, notice, onApply])

  if (!notice) return null

  const canApply = typeof onApply === 'function'
  const label = canApply
    ? t.config.defaultChangedApplyTag
    : t.config.defaultChangedTag
  const detail = canApply
    ? format(t.config.defaultChangedApplyDetail, {
        from: notice.from,
        to: notice.to,
      })
    : format(t.config.defaultChangedDetail, {
        from: notice.from,
        to: notice.to,
      })
  const applyTitle = canApply
    ? format(t.config.defaultChangedApplyAria, { to: notice.to })
    : detail

  return (
    <SettingTitleTag
      variant="muted"
      className={className}
      icon={<LuSparkles />}
      title={applyTitle}
      detail={detail}
      onClick={canApply ? handleApply : undefined}
      onDismiss={handleDismiss}
      dismissAriaLabel={t.config.defaultChangedDismissAria}
      role="status"
    >
      {label}
    </SettingTitleTag>
  )
}

SettingDefaultChangeTag.displayName = 'SettingDefaultChangeTag'
