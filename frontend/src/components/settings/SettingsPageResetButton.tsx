/**
 * 设置页右上角「重置本页」：CheckboxCard 原地二次确认（默认灰 / hover 红 / 无弹窗）
 */

import { FaUndo } from '@lib/icons'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { CheckboxCard } from './items/CheckboxCard'
import './SettingsPageResetButton.css'

const CONFIRM_TIMEOUT_MS = 4000

export interface SettingsPageResetButtonProps {
  onReset: () => void | Promise<void>
  disabled?: boolean
  className?: string
}

export const SettingsPageResetButton: React.FC<SettingsPageResetButtonProps> = ({
  onReset,
  disabled = false,
  className = '',
}) => {
  const { t } = useI18n()
  const [armed, setArmed] = useState(false)
  const [busy, setBusy] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const title = t.config.resetCurrentPage
  const description = t.config.resetCurrentPageDesc
  const confirmTitle =
    t.config.resetCurrentPageAction ?? t.config.resetCurrentPage
  const confirmDesc = t.config.resetCurrentPageConfirm

  const clearTimer = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }, [])

  const disarm = useCallback(() => {
    clearTimer()
    setArmed(false)
  }, [clearTimer])

  const arm = useCallback(() => {
    setArmed(true)
    clearTimer()
    timerRef.current = setTimeout(() => {
      setArmed(false)
      timerRef.current = null
    }, CONFIRM_TIMEOUT_MS)
  }, [clearTimer])

  const runReset = useCallback(async () => {
    if (busy) return
    clearTimer()
    setBusy(true)
    try {
      await onReset()
      setArmed(false)
    } finally {
      setBusy(false)
    }
  }, [busy, clearTimer, onReset])

  const handlePress = useCallback(() => {
    if (disabled || busy) return
    if (armed) {
      void runReset()
      return
    }
    arm()
  }, [disabled, busy, armed, arm, runReset])

  // 点外侧取消武装
  useEffect(() => {
    if (!armed) return
    const onPointerDown = (e: PointerEvent) => {
      const el = rootRef.current
      if (!el) return
      if (e.target instanceof Node && !el.contains(e.target)) {
        disarm()
      }
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') disarm()
    }
    document.addEventListener('pointerdown', onPointerDown, true)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [armed, disarm])

  useEffect(() => () => clearTimer(), [clearTimer])

  return (
    <div
      ref={rootRef}
      className={['settings-page-reset-wrap', className].filter(Boolean).join(' ')}
    >
      <CheckboxCard
        variant="action"
        tone="danger"
        label={armed ? confirmTitle : title}
        description={armed ? confirmDesc : description}
        icon={<FaUndo />}
        checked={false}
        onChange={handlePress}
        disabled={disabled}
        loading={busy}
        title={armed ? confirmDesc : description}
        className={[
          'settings-page-reset',
          armed ? 'is-confirming' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        aria-label={armed ? confirmTitle : title}
        aria-expanded={armed}
      />
    </div>
  )
}

SettingsPageResetButton.displayName = 'SettingsPageResetButton'

export default SettingsPageResetButton
