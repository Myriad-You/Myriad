import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type { ContentIoCopy } from './contentIo/contentIo'
import type { ImportProgress, NoteTransferKind } from './contentIo/types'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import * as phantasiApi from '../../../services/phantasiApi'
import { showError, showSuccess, showWarning } from '../../../utils/toastManager'
import { RequestTurn, unlessAborted } from '../logic/requestTurn'
import { exportTransferFile, importTransferFile } from './contentIo/contentIo'

function copyFrom(phantasi: {
  workbenchContentExportSuccess: string
  errorExportFailed: string
  importStepReading: string
  importStepParsing: string
  importStepImporting: string
  importStepImages: string
  importImagesKept: string
  errorInvalidFormat: string
  importSuccess: string
  errorImportFailed: string
  workbenchNoteUntitled: string
  workbenchNotes: string
}): ContentIoCopy {
  return {
    exportSuccess: phantasi.workbenchContentExportSuccess,
    errorExportFailed: phantasi.errorExportFailed,
    importStepReading: phantasi.importStepReading,
    importStepParsing: phantasi.importStepParsing,
    importStepImporting: phantasi.importStepImporting,
    importStepImages: phantasi.importStepImages,
    importImagesKept: phantasi.importImagesKept,
    errorInvalidFormat: phantasi.errorInvalidFormat,
    importSuccess: phantasi.importSuccess,
    errorImportFailed: phantasi.errorImportFailed,
    untitled: phantasi.workbenchNoteUntitled,
    siteTitle: phantasi.workbenchNotes,
  }
}

export function useNoteTransfer(
  docs: readonly PhantasiNoteDoc[],
  onImported?: () => void,
) {
  const { t } = useI18n()
  const copyRef = useRef<ContentIoCopy>(copyFrom(t.phantasi))
  copyRef.current = copyFrom(t.phantasi)

  const [loading, setLoading] = useState(false)
  const [activeKind, setActiveKind] = useState<NoteTransferKind | null>(null)
  const [progress, setProgress] = useState<ImportProgress | null>(null)
  const turns = useRef(new RequestTurn())

  useEffect(
    () => () => {
      turns.current.cancel()
    },
    [],
  )

  const exportKind = useCallback(
    async (kind: NoteTransferKind) => {
      const signal = turns.current.begin()
      setActiveKind(kind)
      setLoading(true)
      let full: PhantasiNoteDoc[]
      try {
        full = await Promise.all(
          docs.map((doc) => phantasiApi.getNoteDoc(doc.id, signal)),
        )
      } catch (err) {
        if (signal.aborted) {
          setLoading(false)
          setActiveKind(null)
          return
        }
        showError(
          err instanceof Error ? err.message : copyRef.current.errorExportFailed,
        )
        setLoading(false)
        setActiveKind(null)
        return
      }
      if (signal.aborted) {
        setLoading(false)
        setActiveKind(null)
        return
      }
      const result = await exportTransferFile(
        kind,
        full,
        copyRef.current,
        signal,
      )
      if (signal.aborted) {
        setLoading(false)
        setActiveKind(null)
        return
      }
      if (result.ok) showSuccess(result.message)
      else showError(result.error)
      setLoading(false)
      setActiveKind(null)
    },
    [docs],
  )

  const importKind = useCallback(
    async (kind: NoteTransferKind, file: File) => {
      const signal = turns.current.begin()
      setActiveKind(kind)
      setLoading(true)
      const result = await importTransferFile(
        kind,
        file,
        copyRef.current,
        (next) => {
          unlessAborted(signal, () => setProgress(next))
        },
        signal,
      )
      if (signal.aborted) {
        if (result.ok) onImported?.()
        setLoading(false)
        setActiveKind(null)
        setProgress(null)
        return
      }
      if (result.ok) {
        showSuccess(result.message)
        if (result.warning) showWarning(result.warning)
        onImported?.()
      } else {
        showError(result.error)
      }
      setLoading(false)
      setActiveKind(null)
      setProgress(null)
    },
    [onImported],
  )

  return {
    loading,
    activeKind,
    progress,
    exportKind,
    importKind,
  }
}
