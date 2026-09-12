import type { ChangeEvent, CSSProperties } from 'react'
import type { HomeDashboardLayouts, HomeLayoutMode } from '../../utils/homeLayout'
import type { HomeLayoutAssetMap } from '../../utils/homeLayoutTransfer'
import { FaDownload, FaUpload } from '@lib/icons'
import { useCallback, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { fetchStickerAssets } from '../../utils/homeLayoutStickerAssets'
import {
  buildHomeLayoutExport,
  downloadJsonFile,
  HOME_LAYOUT_IMPORT_MAX_BYTES,

  homeLayoutExportFilename,
  parseHomeLayoutImportText,
} from '../../utils/homeLayoutTransfer'
import { showError, showSuccess, showWarning } from '../../utils/toastManager'

export interface HomeLayoutImportPayload {
  layouts: HomeDashboardLayouts
  mode: HomeLayoutMode | null
  assets: HomeLayoutAssetMap
}

interface HomeLayoutTransferButtonsProps {
  buttonClassName: string
  buttonStyle?: CSSProperties
  layouts: HomeDashboardLayouts
  mode: HomeLayoutMode
  disabled?: boolean
  onImport: (payload: HomeLayoutImportPayload) => void | Promise<void>
}

export function HomeLayoutTransferButtons({
  buttonClassName,
  buttonStyle,
  layouts,
  mode,
  disabled,
  onImport,
}: HomeLayoutTransferButtonsProps) {
  const { t, format } = useI18n()
  const fileInputRef = useRef<HTMLInputElement>(null)
  const [busy, setBusy] = useState(false)
  const locked = disabled || busy

  const handleExport = useCallback(async () => {
    if (locked) return
    setBusy(true)
    try {
      const { assets, missing } = await fetchStickerAssets(layouts)
      downloadJsonFile(
        homeLayoutExportFilename(),
        buildHomeLayoutExport(layouts, mode, assets),
      )
      if (missing.length > 0) {
        showWarning(
          format(t.home.exportLayoutPartial, { count: missing.length }),
        )
      } else {
        showSuccess(t.home.exportLayoutSuccess)
      }
    } catch (error) {
      console.error('Export home layout failed:', error)
      showError(t.home.exportLayoutFailed)
    } finally {
      setBusy(false)
    }
  }, [format, layouts, locked, mode, t])

  const handleImportClick = useCallback(() => {
    if (locked) return
    fileInputRef.current?.click()
  }, [locked])

  const handleFileChange = useCallback(
    async (event: ChangeEvent<HTMLInputElement>) => {
      const input = event.currentTarget
      const file = input.files?.[0]
      input.value = ''
      if (!file || locked) return
      if (file.size > HOME_LAYOUT_IMPORT_MAX_BYTES) {
        showError(t.home.importLayoutTooLarge)
        return
      }
      try {
        const parsed = parseHomeLayoutImportText(await file.text(), file.size)
        if (!parsed.ok) {
          showError(
            parsed.reason === 'settings-backup'
              ? t.home.importLayoutSettingsBackup
              : parsed.reason === 'too-large'
                ? t.home.importLayoutTooLarge
                : parsed.reason === 'too-many'
                  ? t.home.importLayoutTooMany
                  : t.home.importLayoutInvalid,
          )
          return
        }
        if (!window.confirm(t.home.importLayoutConfirm)) return
        setBusy(true)
        try {
          await onImport({
            layouts: parsed.layouts,
            mode: parsed.mode,
            assets: parsed.assets,
          })
        } finally {
          setBusy(false)
        }
      } catch (error) {
        console.error('Import home layout failed:', error)
        showError(t.home.importLayoutFailed)
        setBusy(false)
      }
    },
    [locked, onImport, t],
  )

  return (
    <>
      <button
        type="button"
        className={buttonClassName}
        style={buttonStyle}
        disabled={locked}
        onClick={handleImportClick}
        aria-label={t.home.importLayout}
        title={t.home.importLayout}
      >
        <FaUpload size={12} />
        {t.home.importLayout}
      </button>
      <button
        type="button"
        className={buttonClassName}
        style={buttonStyle}
        disabled={locked}
        onClick={() => {
          void handleExport()
        }}
        aria-label={t.home.exportLayout}
        title={t.home.exportLayout}
      >
        <FaDownload size={12} />
        {t.home.exportLayout}
      </button>
      <input
        ref={fileInputRef}
        type="file"
        accept=".json,application/json"
        className="hidden"
        title={t.home.importLayout}
        onChange={(event) => {
          void handleFileChange(event)
        }}
      />
    </>
  )
}
