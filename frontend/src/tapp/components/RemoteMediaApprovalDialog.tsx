import { FaImages } from '@lib/icons'
import { useCallback, useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { SettingsButton } from '../../components/settings'
import { useI18n } from '../../contexts/I18nContext'
import { useAnchoredFloatTip } from '../hooks/useAnchoredFloatTip'
import '../../components/ConfigForm.css'
import './OverwriteInstallDialog.css'

export interface RemoteMediaApprovalDialogProps {
  isOpen: boolean
  anchorEl?: HTMLElement | null
  appName: string
  /** Declared `remoteMedia` hosts not approved yet; all start checked. */
  hosts: string[]
  busy?: boolean
  onSkip: () => void
  onApprove: (approvedHosts: string[]) => void
}

/** 新装后确认 remoteMedia 域名。默认全勾，但每个域名都摆在安装者眼前。 */
export function RemoteMediaApprovalDialog({
  isOpen,
  anchorEl = null,
  appName,
  hosts,
  busy = false,
  onSkip,
  onApprove,
}: RemoteMediaApprovalDialogProps) {
  const { t, format } = useI18n()
  const titleId = useId()

  const hostsRef = useRef(hosts)
  hostsRef.current = hosts
  const onApproveRef = useRef(onApprove)
  onApproveRef.current = onApprove

  const [accepted, setAccepted] = useState<Record<string, boolean>>({})

  const resetForm = useCallback(() => {
    setAccepted(Object.fromEntries(hostsRef.current.map((host) => [host, true])))
  }, [])

  const { panelRef, isMounted, close, handlePanelTransitionEnd, className } =
    useAnchoredFloatTip({
      isOpen,
      anchorEl,
      onRequestClose: onSkip,
      contentKey: `${appName}:${hosts.join(',')}`,
      onEnter: resetForm,
      canDismiss: !busy,
    })

  const handleSkip = useCallback(() => {
    if (busy) return
    close({ notifyParent: true })
  }, [busy, close])

  const handleApprove = useCallback(() => {
    if (busy) return
    onApproveRef.current(
      hostsRef.current.filter((host) => accepted[host] !== false),
    )
  }, [accepted, busy])

  if (!isMounted || typeof document === 'undefined') return null

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
        <FaImages className="overwrite-install-tip-icon" aria-hidden />
        <h3 id={titleId} className="overwrite-install-tip-title">
          {format(t.tapp.remoteMediaApproveTitle, { name: appName })}
        </h3>
      </div>

      <p className="overwrite-install-tip-desc">{t.tapp.remoteMediaDesc}</p>

      <div className="overwrite-install-tip-perms">
        <ul className="overwrite-install-tip-perms-list">
          {hosts.map((host) => (
            <li key={host}>
              <label className="overwrite-install-tip-perm">
                <input
                  type="checkbox"
                  checked={accepted[host] !== false}
                  disabled={busy}
                  onChange={(e) =>
                    setAccepted((prev) => ({
                      ...prev,
                      [host]: e.target.checked,
                    }))
                  }
                />
                <code>{host}</code>
              </label>
            </li>
          ))}
        </ul>
      </div>

      <div className="overwrite-install-tip-actions">
        <SettingsButton
          variant="secondary"
          size="sm"
          onClick={handleSkip}
          disabled={busy}
        >
          {t.tapp.remoteMediaSkipBtn}
        </SettingsButton>
        <SettingsButton
          variant="primary"
          size="sm"
          onClick={handleApprove}
          loading={busy}
          disabled={busy}
        >
          {t.tapp.remoteMediaApproveBtn}
        </SettingsButton>
      </div>
    </div>,
    document.body,
  )
}

export default RemoteMediaApprovalDialog
