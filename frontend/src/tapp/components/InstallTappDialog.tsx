import type { ChangeEvent, DragEvent } from 'react'
import { FaFileAlt, FaUpload } from '@lib/icons'
import {

  useCallback,
  useId,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { SettingsButton } from '../../components/settings'
import { Spinner } from '../../components/Spinner'
import { useI18n } from '../../contexts/I18nContext'
import { showError, showStickyToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import { useAnchoredFloatTip } from '../hooks/useAnchoredFloatTip'
import * as TappApiService from '../services/TappApiService'
import { TappHttpError } from '../services/TappHttpClient'
import { OverwriteInstallDialog } from './OverwriteInstallDialog'
import '../../components/ConfigForm.css'
import './InstallTappDialog.css'

/** Structured 409 body from `POST /api/tapps/install-file` on an existing id. */
interface OverwriteConflictDetails {
  tappId?: string
  name?: string
  installedVersion?: string
  incomingVersion?: string
  newPermissions?: string[]
}

function overwriteConflictDetails(err: unknown): OverwriteConflictDetails | null {
  if (!(err instanceof TappHttpError)) return null
  if (err.status !== 409 || err.code !== 'tapp_already_installed') return null
  const body = err.body as { details?: OverwriteConflictDetails } | undefined
  return body?.details ?? {}
}

export interface InstallTappDialogProps {
  isOpen: boolean
  anchorEl?: HTMLElement | null
  onCancel: () => void
  onInstall: () => void
  onSuccess?: (name: string) => void
}

export function InstallTappDialog({
  isOpen,
  anchorEl = null,
  onCancel,
  onInstall,
  onSuccess,
}: InstallTappDialogProps) {
  const { t } = useI18n()
  const titleId = useId()
  const fileInputRef = useRef<HTMLInputElement>(null)

  const [loading, setLoading] = useState(false)
  const [dragOver, setDragOver] = useState(false)
  const [overwritePrompt, setOverwritePrompt] = useState<{
    file: File
    details: OverwriteConflictDetails
  } | null>(null)
  const [overwriting, setOverwriting] = useState(false)

  const onInstallRef = useRef(onInstall)
  const onSuccessRef = useRef(onSuccess)
  const onCancelRef = useRef(onCancel)
  onInstallRef.current = onInstall
  onSuccessRef.current = onSuccess
  onCancelRef.current = onCancel

  const resetForm = useCallback(() => {
    setLoading(false)
    setDragOver(false)
    setOverwritePrompt(null)
    setOverwriting(false)
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
    contentKey: `${loading ? 1 : 0}`,
    onEnter: resetForm,
    // The overwrite prompt is a second panel; keep this tip mounted while it
    // is open so a press inside that panel does not dismiss both.
    canDismiss: !loading && overwritePrompt === null,
  })

  const handleCancel = useCallback(() => {
    if (loading) return
    close({ notifyParent: true })
  }, [close, loading])

  const runInstall = useCallback(
    async (file: File, permissions?: string[], overwrite?: boolean) => {
      const startedSession = session
      setLoading(true)

      try {
        const result = await TappApiService.installTappFile(
          file,
          permissions,
          overwrite,
        )
        onInstallRef.current()
        onSuccessRef.current?.(
          result.name || file.name.replaceAll(/\.tapp$/gi, ''),
        )
        // 仅当前会话仍 open 时收起 UI。
        if (!isCurrentSession(startedSession)) return
        // 先让父级 isOpen=false，再本地 close（不 notify）。
        onCancelRef.current()
        close({ notifyParent: false })
      } catch (err) {
        if (!isCurrentSession(startedSession)) return
        const details = overwriteConflictDetails(err)
        if (details) {
          // 已安装同 id：不报错，改为询问是否覆盖。
          setLoading(false)
          setOverwriting(false)
          setOverwritePrompt({ file, details })
          return
        }
        showStickyToast({
          message: userFacingError(err, t.tapp.installFailed),
          type: 'error',
          replaceKey: 'tapp-install',
        })
        setLoading(false)
        setOverwriting(false)
        setOverwritePrompt(null)
      }
    },
    [session, isCurrentSession, t.tapp.installFailed, close],
  )

  const handleFileUpload = useCallback(
    (file: File) => {
      if (loading) return
      if (!file.name.endsWith('.tapp')) {
        showError(t.tapp.selectTappFile)
        return
      }
      void runInstall(file)
    },
    [loading, runInstall, t.tapp.selectTappFile],
  )

  const handleConfirmOverwrite = useCallback(
    (acceptedPermissions: string[]) => {
      if (!overwritePrompt || overwriting) return
      setOverwriting(true)
      void runInstall(overwritePrompt.file, acceptedPermissions, true)
    },
    [overwritePrompt, overwriting, runInstall],
  )

  const handleCancelOverwrite = useCallback(() => {
    if (overwriting) return
    setOverwritePrompt(null)
  }, [overwriting])

  const handleDrop = useCallback(
    (e: DragEvent) => {
      e.preventDefault()
      setDragOver(false)
      if (loading) return
      const file = e.dataTransfer.files[0]
      if (file) handleFileUpload(file)
    },
    [loading, handleFileUpload],
  )

  const handleFileChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      e.target.value = ''
      if (file) handleFileUpload(file)
    },
    [handleFileUpload],
  )

  if (!isMounted || typeof document === 'undefined') return null

  const installTip = createPortal(
    <div
      ref={panelRef}
      className={className('install-tip')}
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
      onTransitionEnd={handlePanelTransitionEnd}
    >
      <div className="install-tip-header">
        <FaUpload className="install-tip-header-icon" aria-hidden />
        <h3 id={titleId} className="install-tip-title">
          {t.tapp.installTappTitle}
        </h3>
      </div>

      <div
        className={[
          'install-tip-drop',
          dragOver ? 'is-dragover' : '',
          loading ? 'is-disabled' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        onDragOver={(e) => {
          e.preventDefault()
          if (!loading) setDragOver(true)
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={handleDrop}
        onClick={() => {
          if (!loading) fileInputRef.current?.click()
        }}
        role="button"
        tabIndex={loading ? -1 : 0}
        onKeyDown={(e) => {
          if (loading) return
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            fileInputRef.current?.click()
          }
        }}
        aria-label={t.tapp.selectTappFile}
      >
        <input
          ref={fileInputRef}
          type="file"
          accept=".tapp"
          onChange={handleFileChange}
          className="hidden"
          disabled={loading}
          aria-hidden
          tabIndex={-1}
        />
        {loading ? (
          <Spinner size="md" color="primary" />
        ) : (
          <>
            <FaFileAlt className="install-tip-drop-icon" aria-hidden />
            <p className="install-tip-drop-label">{t.tapp.dropTappFile}</p>
            <p className="install-tip-drop-hint">{t.tapp.orClickToSelect}</p>
          </>
        )}
      </div>

      <div className="install-tip-actions">
        <SettingsButton
          variant="secondary"
          size="sm"
          onClick={handleCancel}
          disabled={loading}
        >
          {t.common.cancel}
        </SettingsButton>
      </div>
    </div>,
    document.body,
  )

  return (
    <>
      {installTip}
      <OverwriteInstallDialog
        isOpen={overwritePrompt !== null}
        anchorEl={anchorEl}
        appName={overwritePrompt?.details.name || overwritePrompt?.file.name || ''}
        installedVersion={overwritePrompt?.details.installedVersion ?? ''}
        incomingVersion={overwritePrompt?.details.incomingVersion ?? ''}
        newPermissions={overwritePrompt?.details.newPermissions ?? []}
        busy={overwriting}
        onCancel={handleCancelOverwrite}
        onConfirm={handleConfirmOverwrite}
      />
    </>
  )
}

export default InstallTappDialog
