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
import { userFacingError } from '../../utils/userFacingError'
import { useAnchoredFloatTip } from '../hooks/useAnchoredFloatTip'
import * as TappApiService from '../services/TappApiService'
import '../../components/ConfigForm.css'
import './InstallTappDialog.css'

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

  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [dragOver, setDragOver] = useState(false)

  const onInstallRef = useRef(onInstall)
  const onSuccessRef = useRef(onSuccess)
  const onCancelRef = useRef(onCancel)
  onInstallRef.current = onInstall
  onSuccessRef.current = onSuccess
  onCancelRef.current = onCancel

  const resetForm = useCallback(() => {
    setError('')
    setLoading(false)
    setDragOver(false)
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
    contentKey: `${error ? 1 : 0}-${loading ? 1 : 0}`,
    onEnter: resetForm,
    canDismiss: !loading,
  })

  const handleCancel = useCallback(() => {
    if (loading) return
    close({ notifyParent: true })
  }, [close, loading])

  const handleFileUpload = useCallback(
    async (file: File) => {
      if (loading) return
      if (!file.name.endsWith('.tapp')) {
        setError(t.tapp.selectTappFile)
        return
      }

      const startedSession = session
      setError('')
      setLoading(true)

      try {
        const result = await TappApiService.installTappFile(file)
        onInstallRef.current()
        onSuccessRef.current?.(
          result.name || file.name.replaceAll(/\.tapp$/ig, ''),
        )
        // 仅当前会话仍 open 时收起 UI。
        if (!isCurrentSession(startedSession)) return
        // 先让父级 isOpen=false，再本地 close（不 notify）。
        onCancelRef.current()
        close({ notifyParent: false })
      } catch (err) {
        if (!isCurrentSession(startedSession)) return
        setError(userFacingError(err, t.tapp.installFailed))
        setLoading(false)
      }
    },
    [
      loading,
      session,
      isCurrentSession,
      t.tapp.selectTappFile,
      t.tapp.installFailed,
      close,
    ],
  )

  const handleDrop = useCallback(
    (e: DragEvent) => {
      e.preventDefault()
      setDragOver(false)
      if (loading) return
      const file = e.dataTransfer.files[0]
      if (file) void handleFileUpload(file)
    },
    [loading, handleFileUpload],
  )

  const handleFileChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      e.target.value = ''
      if (file) void handleFileUpload(file)
    },
    [handleFileUpload],
  )

  if (!isMounted || typeof document === 'undefined') return null

  return createPortal(
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

      {error ? <p className="install-tip-error">{error}</p> : null}

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
}

export default InstallTappDialog
