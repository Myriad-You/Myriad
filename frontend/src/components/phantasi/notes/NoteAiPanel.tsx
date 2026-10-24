import type { CSSProperties } from 'react'
import type { useNoteAiEdit } from './useNoteAiEdit'
import { useEffect, useId, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { NoteButton, NoteSwitch } from './NoteControls'
import { prepareNoteReaderHtml } from './noteImageUrl'
import { decorateNoteReadSurface } from './noteReadSurface'
import './NoteAiPanel.css'

export function NoteAiPanel({ ai, previewClass, previewStyle }: {
  ai: ReturnType<typeof useNoteAiEdit>
  previewClass: string
  previewStyle: CSSProperties
}) {
  const { t, format } = useI18n()
  const copy = t.phantasi
  const labelId = useId()
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const previewRef = useRef<HTMLDivElement>(null)
  const [view, setView] = useState<'preview' | 'source'>('preview')
  useEffect(() => { if (ai.isOpen) inputRef.current?.focus() }, [ai.isOpen])
  useEffect(() => {
    if (!ai.result || !previewRef.current || view !== 'preview') return
    decorateNoteReadSurface(previewRef.current, copy.copyCode, copy.copyTex)
  }, [ai.result, view, copy.copyCode, copy.copyTex])
  if (!ai.isOpen || !ai.snapshot) return null
  const errors: Record<string, string> = {
    note_ai_failed: copy.noteAiFailed,
    note_ai_timeout: copy.noteAiTimeout,
    note_ai_too_long: copy.noteAiTooLong,
    note_ai_context_limit: copy.noteAiContextLimit,
    note_ai_empty: copy.noteAiEmpty,
    note_ai_incomplete: copy.noteAiIncomplete,
    note_ai_protected_content: copy.noteAiProtected,
    note_ai_invalid_request: copy.noteAiSelectionUnavailable,
  }
  const unavailable = ai.scope === 'selection' && (!ai.snapshot.selection || ai.selectionUnavailable)
  return (
    <section
      className="note-ai"
      aria-labelledby={labelId}
      onKeyDown={event => {
      if (event.key === 'Escape') { event.stopPropagation(); ai.close() }
    }}
    >
      <div className="note-ai__heading">
        <h2 id={labelId}>{copy.noteAiTitle}</h2>
        <NoteButton variant="quiet" onClick={ai.close}>{copy.close}</NoteButton>
      </div>
      <p className="note-ai__hint">{format(copy.noteAiContext, { count: [...ai.snapshot.document.contentMd].length })}</p>
      <NoteSwitch
        value={ai.scope}
        onChange={ai.setScope}
        ariaLabel={copy.noteAiScope}
        options={[
        { value: 'document', label: copy.noteAiWhole },
        { value: 'selection', label: copy.noteAiSelection },
      ]}
      />
      {ai.scope === 'selection' && ai.snapshot.selection && !unavailable ? (
        <pre className="note-ai__selection">{ai.snapshot.document.contentMd.slice(ai.snapshot.selection.start, ai.snapshot.selection.end)}</pre>
      ) : null}
      {unavailable ? <p role="status">{copy.noteAiSelectionUnavailable}</p> : null}
      <label className="note-ai__instruction">
        <span>{copy.noteAiInstruction}</span>
        <textarea
          ref={inputRef}
          rows={3}
          maxLength={4000}
          value={ai.instruction}
          disabled={ai.busy}
          placeholder={copy.noteAiPlaceholder}
          onChange={event => ai.setInstruction(event.target.value)}
        />
      </label>
      {ai.stale ? <div role="status" className="note-ai__notice">
        <p>{copy.noteAiStale}</p>
        <NoteButton onClick={() => ai.open(null)}>{copy.noteAiRefresh}</NoteButton>
      </div> : null}
      {ai.error ? <p role="alert" className="note-ai__notice">{errors[ai.error] ?? (ai.error === t.errors.aiNotConfigured ? ai.error : copy.noteAiFailed)}</p> : null}
      <div className="note-ai__actions">
        <NoteButton variant="tint" loading={ai.busy} disabled={ai.stale || unavailable || !ai.snapshot.document.contentMd.trim()} onClick={() => void ai.generate()}>
          {ai.busy ? copy.noteAiGenerating : copy.noteAiGenerate}
        </NoteButton>
        {ai.busy ? <NoteButton onClick={ai.cancel}>{copy.noteAiCancel}</NoteButton> : null}
        {ai.result ? <NoteButton disabled={ai.stale} onClick={ai.applyResult}>{copy.noteAiApply}</NoteButton> : null}
      </div>
      {ai.result ? <>
        <NoteSwitch
          value={view}
          onChange={setView}
          ariaLabel={copy.noteAiResult}
          options={[
          { value: 'preview', label: copy.noteAiPreview }, { value: 'source', label: copy.noteAiSource },
        ]}
        />
        <div className="note-ai__comparison">
          <div><h3>{copy.noteAiOriginal}</h3><pre className="note-ai__source">{ai.snapshot.document.contentMd}</pre></div>
          <div><h3>{copy.noteAiResult}</h3>
            {view === 'source' ? <pre className="note-ai__source">{ai.result.content_md}</pre> : (
              <div className={`note-ai__preview ${previewClass}`} style={previewStyle}>
                <div ref={previewRef} dangerouslySetInnerHTML={{ __html: prepareNoteReaderHtml(ai.result.html, '') }} />
              </div>
            )}
          </div>
        </div>
      </> : null}
    </section>
  )
}
