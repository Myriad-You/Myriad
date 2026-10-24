import type { NoteEditorDefaultView, NoteHistoryEntry, NoteHistorySummary } from '../../../services/phantasiApi'
import { useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { getNoteHistory, getNoteHistoryEntry } from '../../../services/phantasiApi'
import { NoteButton, NoteSection, NoteSwitch } from './NoteControls'

interface NoteEditorSettingsProps {
  cloudId: number | null
  defaultView: NoteEditorDefaultView
  preferenceBusy: boolean
  onDefaultView: (view: NoteEditorDefaultView) => Promise<void>
  onRestore: (entry: NoteHistoryEntry) => Promise<void>
  busy: boolean
}

/** 两块一起：默认视图（跟账号走）和这篇的编辑历史。 */
export function NoteEditorSettings(props: NoteEditorSettingsProps) {
  return (
    <>
      <NoteDefaultViewSection {...props} />
      <NoteHistorySection {...props} />
    </>
  )
}

/** 打开编辑器时先进哪个视图。不属于这篇文章，随账号保存。 */
export function NoteDefaultViewSection({ defaultView, preferenceBusy, onDefaultView }: Pick<NoteEditorSettingsProps, 'defaultView' | 'preferenceBusy' | 'onDefaultView'>) {
  const { t } = useI18n()
  return (
    <NoteSection title={t.phantasi.noteDefaultView} hint={t.phantasi.noteDefaultViewHint}>
      <NoteSwitch
        className="phantasi-note__default-view"
        ariaLabel={t.phantasi.noteDefaultView}
        value={defaultView}
        disabled={preferenceBusy}
        options={[
          { value: 'visual', label: t.phantasi.noteTabVisual },
          { value: 'write', label: t.phantasi.noteTabWrite },
          { value: 'preview', label: t.phantasi.noteTabPreview },
        ]}
        onChange={(value) => { void onDefaultView(value as NoteEditorDefaultView) }}
      />
    </NoteSection>
  )
}

/** 这篇笔记最近的旧版本：点开看全文，再决定恢复。 */
export function NoteHistorySection({ cloudId, onRestore, busy }: Pick<NoteEditorSettingsProps, 'cloudId' | 'onRestore' | 'busy'>) {
  const { t, locale, format } = useI18n()
  const [history, setHistory] = useState<NoteHistorySummary[]>([])
  const [selected, setSelected] = useState<NoteHistoryEntry | null>(null)
  const [selection, setSelection] = useState<{ docId: number; revision: number } | null>(null)
  const selectedRevision = selection?.docId === cloudId ? selection.revision : null
  const [previewLoading, setPreviewLoading] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(false)
  const [refresh, setRefresh] = useState(0)
  const [restoring, setRestoring] = useState(false)
  useEffect(() => {
    setHistory([])
    setSelection(null)
    setSelected(null)
    if (cloudId == null) { setLoading(false); return }
    const controller = new AbortController()
    setLoading(true)
    setError(false)
    void getNoteHistory(cloudId, controller.signal).then((rows) => {
      if (!controller.signal.aborted) setHistory(rows)
    }).catch(() => {
      if (!controller.signal.aborted) setError(true)
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false)
    })
    return () => controller.abort()
  }, [cloudId, refresh])
  useEffect(() => {
    setSelected(null)
    if (cloudId == null || selectedRevision == null) { setPreviewLoading(false); return }
    const controller = new AbortController()
    setPreviewLoading(true)
    setError(false)
    void getNoteHistoryEntry(cloudId, selectedRevision, controller.signal).then((entry) => {
      if (!controller.signal.aborted) setSelected(entry)
    }).catch(() => {
      if (!controller.signal.aborted) setError(true)
    }).finally(() => {
      if (!controller.signal.aborted) setPreviewLoading(false)
    })
    return () => controller.abort()
  }, [cloudId, selectedRevision])
  const restore = async () => {
    if (!selected || selected.revision !== selectedRevision || previewLoading || restoring || busy) return
    setRestoring(true)
    setError(false)
    try {
      await onRestore(selected)
      setSelected(null)
      setSelection(null)
      setRefresh((value) => value + 1)
    } catch { setError(true) }
    finally { setRestoring(false) }
  }
  return (
    <NoteSection
      title={<span className="phantasi-note__history-heading">
        <span>{t.phantasi.noteHistory}</span>
        <NoteButton variant="quiet" className="phantasi-note__history-refresh" onClick={() => setRefresh((value) => value + 1)} disabled={loading || restoring}>
          {t.phantasi.noteHistoryRefresh}
        </NoteButton>
      </span>}
      hint={t.phantasi.noteHistoryHint}
    >
        {loading || previewLoading ? <p className="phantasi-note__history-message" role="status">{t.phantasi.noteHistoryLoading}</p> : null}
        {error ? <p className="phantasi-note__history-message is-error" role="alert">{t.phantasi.noteHistoryFailed}</p> : null}
        {!loading && !error && history.length === 0 ? <p className="phantasi-note__history-message">{t.phantasi.noteHistoryEmpty}</p> : null}
        <ul className="phantasi-note__history-list">
          {history.map((entry) => (
            <li key={entry.revision}>
              <button type="button" className="phantasi-note__history-entry" aria-pressed={selectedRevision === entry.revision} onClick={() => { if (cloudId != null) setSelection({ docId: cloudId, revision: entry.revision }) }} disabled={restoring}>
                <strong>{entry.snapshot.title || t.phantasi.noteHistoryUntitled}</strong>
                <time dateTime={new Date(entry.saved_at).toISOString()}>{new Date(entry.saved_at).toLocaleString(locale)}</time>
                <span>{entry.actor_name || t.phantasi.noteHistoryUnknown} · {format(t.phantasi.noteHistoryVersion, { revision: entry.revision })}</span>
              </button>
            </li>
          ))}
        </ul>
        {selected ? (
          <div className="phantasi-note__history-preview">
            <h3>{selected.snapshot.title || t.phantasi.noteHistoryUntitled}</h3>
            {selected.snapshot.topic ? <p>{selected.snapshot.topic}</p> : null}
            <pre>{selected.snapshot.content_md}</pre>
            <p>{t.phantasi.noteHistoryRestoreHint}</p>
            <NoteButton variant="solid" onClick={() => { void restore() }} disabled={busy || restoring}>
              {restoring ? t.phantasi.noteHistoryRestoring : t.phantasi.noteHistoryRestore}
            </NoteButton>
          </div>
        ) : null}
      </NoteSection>
  )
}
