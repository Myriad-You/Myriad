/**
 * Tapp 卸载确认 — 锚定触发按钮的浮层
 * 生命周期见 useAnchoredFloatTip（点外/Esc 关闭、session 防竞态）
 */

import { FaTrash } from '@lib/icons'
import { useCallback, useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { SettingsButton, ToggleSwitch } from '../../components/settings'
import { useI18n } from '../../contexts/I18nContext'
import { useAnchoredFloatTip } from '../hooks/useAnchoredFloatTip'
import '../../components/ConfigForm.css'
import './UninstallConfirmDialog.css'

export interface UninstallConfirmDialogProps {
  isOpen: boolean
  appName: string
  /** 定位锚点（卸载按钮） */
  anchorEl?: HTMLElement | null
  onCancel: () => void
  onConfirm: (keepData: boolean) => Promise<void>
}

export function UninstallConfirmDialog({
  isOpen,
  appName,
  anchorEl = null,
  onCancel,
  onConfirm,
}: UninstallConfirmDialogProps) {
  const { t, format } = useI18n()
  const titleId = useId()
  const titleText = format(t.tapp.confirmUninstall, { name: appName })

  const [keepData, setKeepData] = useState(false)
  const [uninstalling, setUninstalling] = useState(false)

  const onConfirmRef = useRef(onConfirm)
  onConfirmRef.current = onConfirm

  const resetForm = useCallback(() => {
    setKeepData(false)
    setUninstalling(false)
  }, [])

  const {
    panelRef,
    isMounted,
    session,
    isCurrentSession,
    close,
    handlePanelTransitionEnd,
    className,
  } = useAnchoredFloatTip({
    isOpen,
    anchorEl,
    onRequestClose: onCancel,
    contentKey: appName,
    onEnter: resetForm,
    canDismiss: !uninstalling,
  })

  const handleCancel = useCallback(() => {
    if (uninstalling) return
    close({ notifyParent: true })
  }, [close, uninstalling])

  const handleConfirm = useCallback(async () => {
    if (uninstalling) return
    const startedSession = session
    setUninstalling(true)
    try {
      await onConfirmRef.current(keepData)
      // 父级成功后通常 isOpen=false；仅当前 open 会话再本地收起
      if (!isCurrentSession(startedSession)) return
      close({ notifyParent: false })
    } catch {
      if (isCurrentSession(startedSession)) setUninstalling(false)
    }
  }, [keepData, uninstalling, close, session, isCurrentSession])

  if (!isMounted || typeof document === 'undefined') return null

  return createPortal(
    <div
      ref={panelRef}
      className={className('uninstall-tip')}
      role="alertdialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
      onTransitionEnd={handlePanelTransitionEnd}
    >
      <h3 id={titleId} className="uninstall-tip-title" title={titleText}>
        {titleText}
      </h3>

      <div className="uninstall-tip-keep">
        <div className="uninstall-tip-keep-text">
          <span className="uninstall-tip-keep-label">
            {t.tapp.keepDataOnUninstall}
          </span>
          <span className="uninstall-tip-keep-desc">
            {t.tapp.keepDataOnUninstallDesc}
          </span>
        </div>
        <ToggleSwitch
          checked={keepData}
          onChange={setKeepData}
          disabled={uninstalling}
          aria-label={t.tapp.keepDataOnUninstall}
        />
      </div>

      <div className="uninstall-tip-actions">
        <SettingsButton
          variant="secondary"
          size="sm"
          onClick={handleCancel}
          disabled={uninstalling}
        >
          {t.common.cancel}
        </SettingsButton>
        <SettingsButton
          variant="danger"
          size="sm"
          icon={<FaTrash />}
          onClick={() => void handleConfirm()}
          loading={uninstalling}
          disabled={uninstalling}
        >
          {uninstalling ? t.tapp.uninstalling : t.tapp.confirmUninstallBtn}
        </SettingsButton>
      </div>
    </div>,
    document.body,
  )
}

export default UninstallConfirmDialog
