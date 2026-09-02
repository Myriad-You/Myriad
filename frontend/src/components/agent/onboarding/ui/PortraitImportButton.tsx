import type { DragEvent } from 'react'
import { LuUpload } from '@lib/icons'
import { useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { uploadSitePortrait } from '../../../../features/merope/api'
import { notifyFaceUpdated } from '../../../../features/merope/events'
import { userFacingError } from '../../../../utils/userFacingError'
import { SettingsButton } from '../../../settings'

interface Props {
  appearance?: 'onboarding' | 'settings'
  disabled?: boolean
  previewUrl?: string | null
  onUploaded: (portraitUrl: string) => void | Promise<void>
  onError?: (message: string) => void
}

const ACCEPT = new Set(['image/png', 'image/jpeg', 'image/webp'])

export default function PortraitImportButton({
  appearance = 'onboarding',
  disabled = false,
  previewUrl = null,
  onUploaded,
  onError,
}: Props) {
  const { t } = useI18n()
  const o = t.agentPersona.onboarding
  const inputRef = useRef<HTMLInputElement>(null)
  const [busy, setBusy] = useState(false)
  const [over, setOver] = useState(false)

  const blocked = disabled || busy

  const pickFile = () => {
    if (blocked) return
    inputRef.current?.click()
  }

  const onFile = async (file: File | undefined) => {
    if (!file || blocked) return
    if (file.type && !ACCEPT.has(file.type)) {
      onError?.(o.importPortraitFailed)
      return
    }
    setBusy(true)
    onError?.('')
    try {
      const uploaded = await uploadSitePortrait(file)
      notifyFaceUpdated()
      await onUploaded(uploaded.portraitUrl)
    } catch (reason) {
      onError?.(userFacingError(reason, o.importPortraitFailed))
    } finally {
      setBusy(false)
      if (inputRef.current) inputRef.current.value = ''
    }
  }

  const onDragOver = (event: DragEvent<HTMLButtonElement>) => {
    event.preventDefault()
    if (blocked) return
    setOver(true)
  }

  const onDragLeave = (event: DragEvent<HTMLButtonElement>) => {
    const next = event.relatedTarget
    if (next instanceof Node && event.currentTarget.contains(next)) return
    setOver(false)
  }

  const onDrop = (event: DragEvent<HTMLButtonElement>) => {
    event.preventDefault()
    setOver(false)
    void onFile(event.dataTransfer.files?.[0])
  }

  const label = busy ? o.importPortraitBusy : o.importPortrait

  return (
    <>
      <input
        ref={inputRef}
        type="file"
        accept="image/png,image/jpeg,image/webp"
        hidden
        onChange={(event) => void onFile(event.target.files?.[0])}
      />
      {appearance === 'settings' ? (
        <SettingsButton
          type="button"
          size="sm"
          variant="secondary"
          disabled={disabled}
          loading={busy}
          onClick={pickFile}
        >
          {label}
        </SettingsButton>
      ) : (
        <button
          type="button"
          className={`merope-ob-import__stage${previewUrl ? ' has-image' : ''}${over ? ' is-over' : ''}${busy ? ' is-busy' : ''}`}
          disabled={blocked}
          aria-label={previewUrl ? o.importPortraitReplace : o.importPortrait}
          onClick={pickFile}
          onDragOver={onDragOver}
          onDragLeave={onDragLeave}
          onDrop={onDrop}
        >
          {previewUrl ? (
            <>
              <img
                className="merope-ob-import__portrait"
                src={previewUrl}
                alt=""
                draggable={false}
              />
              <span className="merope-ob-import__stage-overlay">
                {busy ? o.importPortraitBusy : o.importPortraitReplace}
              </span>
            </>
          ) : (
            <span className="merope-ob-import__stage-empty">
              <LuUpload aria-hidden />
              <span className="merope-ob-import__stage-title">{label}</span>
              <span className="merope-ob-import__stage-hint">
                {o.importPortraitHint}
              </span>
            </span>
          )}
        </button>
      )}
    </>
  )
}
