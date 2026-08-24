import { useRef, useState } from 'react'
import { SettingsButton } from '../../../settings'
import { useI18n } from '../../../../contexts/I18nContext'
import { uploadSitePortrait } from '../../../../features/merope/api'
import { notifyFaceUpdated } from '../../../../features/merope/events'
import { userFacingError } from '../../../../utils/userFacingError'
import { GhostButton } from './Chrome'

interface Props {
  appearance?: 'onboarding' | 'settings'
  disabled?: boolean
  onUploaded: (portraitUrl: string) => void | Promise<void>
  onError?: (message: string) => void
}

export default function PortraitImportButton({
  appearance = 'onboarding',
  disabled = false,
  onUploaded,
  onError,
}: Props) {
  const { t } = useI18n()
  const o = t.agentPersona.onboarding
  const inputRef = useRef<HTMLInputElement>(null)
  const [busy, setBusy] = useState(false)

  const pickFile = () => {
    if (disabled || busy) return
    inputRef.current?.click()
  }

  const onFile = async (file: File | undefined) => {
    if (!file || disabled || busy) return
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
        <GhostButton
          label={label}
          disabled={disabled || busy}
          onClick={pickFile}
        />
      )}
    </>
  )
}
