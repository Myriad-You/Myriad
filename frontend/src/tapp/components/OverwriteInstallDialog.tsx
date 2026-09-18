import type { TappPermission } from '../types'
import { FaUpload } from '@lib/icons'
import { useCallback, useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { SettingsButton } from '../../components/settings'
import { useI18n } from '../../contexts/I18nContext'
import { PERMISSION_COPY } from '../constants/permissionCopy'
import { useAnchoredFloatTip } from '../hooks/useAnchoredFloatTip'
import '../../components/ConfigForm.css'
import './OverwriteInstallDialog.css'

export interface OverwriteInstallDialogProps {
  isOpen: boolean
  anchorEl?: HTMLElement | null
  appName: string
  installedVersion: string
  incomingVersion: string
  /** Permissions the new package declares that the install has not approved yet. */
  newPermissions: string[]
  busy?: boolean
  onCancel: () => void
  onConfirm: (acceptedPermissions: string[]) => void
}

function permissionLabel(
  permission: string,
  t: Record<string, string>,
  unknownTemplate: string,
): string {
  const copy = PERMISSION_COPY[permission as TappPermission]
  if (!copy) return unknownTemplate.replaceAll('{permission}', permission)
  return t[copy.labelKey] ?? permission
}

export function OverwriteInstallDialog({
  isOpen,
  anchorEl = null,
  appName,
  installedVersion,
  incomingVersion,
  newPermissions,
  busy = false,
  onCancel,
  onConfirm,
}: OverwriteInstallDialogProps) {
  const { t, format } = useI18n()
  const titleId = useId()

  const newPermissionsRef = useRef(newPermissions)
  newPermissionsRef.current = newPermissions
  const onConfirmRef = useRef(onConfirm)
  onConfirmRef.current = onConfirm

  const [accepted, setAccepted] = useState<Record<string, boolean>>({})

  const resetForm = useCallback(() => {
    setAccepted(
      Object.fromEntries(newPermissionsRef.current.map((name) => [name, true])),
    )
  }, [])

  const {
    panelRef,
    isMounted,
    close,
    handlePanelTransitionEnd,
    className,
  } = useAnchoredFloatTip({
    isOpen,
    anchorEl,
    onRequestClose: onCancel,
    contentKey: `${appName}:${incomingVersion}`,
    onEnter: resetForm,
    canDismiss: !busy,
  })

  const handleCancel = useCallback(() => {
    if (busy) return
    close({ notifyParent: true })
  }, [busy, close])

  const handleConfirm = useCallback(() => {
    if (busy) return
    const acceptedList = newPermissionsRef.current.filter(
      (name) => accepted[name] !== false,
    )
    onConfirmRef.current(acceptedList)
  }, [accepted, busy])

  if (!isMounted || typeof document === 'undefined') return null

  const tappCopy = t.tapp as unknown as Record<string, string>
  const versionLine = `${installedVersion} → ${incomingVersion}`

  return createPortal(
    <div
      ref={panelRef}
      className={className('overwrite-install-tip')}
      role="alertdialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
      onTransitionEnd={handlePanelTransitionEnd}
    >
      <div className="overwrite-install-tip-header">
        <FaUpload className="overwrite-install-tip-icon" aria-hidden />
        <h3 id={titleId} className="overwrite-install-tip-title">
          {format(t.tapp.confirmOverwriteInstall, { name: appName })}
        </h3>
      </div>

      <p className="overwrite-install-tip-desc">
        {format(t.tapp.confirmOverwriteInstallDesc, {
          installed: installedVersion,
          incoming: incomingVersion,
        })}
      </p>
      <p className="overwrite-install-tip-versions" aria-label={versionLine}>
        {versionLine}
      </p>

      {newPermissions.length > 0 && (
        <div className="overwrite-install-tip-perms">
          <span className="overwrite-install-tip-perms-label">
            {t.tapp.newPermissionsLabel}
          </span>
          <ul className="overwrite-install-tip-perms-list">
            {newPermissions.map((permission) => (
              <li key={permission}>
                <label className="overwrite-install-tip-perm">
                  <input
                    type="checkbox"
                    checked={accepted[permission] !== false}
                    disabled={busy}
                    onChange={(e) =>
                      setAccepted((prev) => ({
                        ...prev,
                        [permission]: e.target.checked,
                      }))
                    }
                  />
                  <span>
                    {permissionLabel(
                      permission,
                      tappCopy,
                      t.tapp.unknownPermission,
                    )}
                  </span>
                </label>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="overwrite-install-tip-actions">
        <SettingsButton
          variant="secondary"
          size="sm"
          onClick={handleCancel}
          disabled={busy}
        >
          {t.common.cancel}
        </SettingsButton>
        <SettingsButton
          variant="primary"
          size="sm"
          onClick={handleConfirm}
          loading={busy}
          disabled={busy}
        >
          {busy ? t.tapp.overwriting : t.tapp.confirmOverwriteInstallBtn}
        </SettingsButton>
      </div>
    </div>,
    document.body,
  )
}

export default OverwriteInstallDialog
