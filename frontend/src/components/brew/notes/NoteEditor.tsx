/** 不做所见即所得：预览与发布走同一套后端渲染。dangerouslySetInnerHTML 只吃后端白名单消毒后的 HTML。 */

import type { BrewNoteInput } from '../../../types/brew'

import {
  LuBold as Bold,
  LuCode as Code,
  LuHeading as Heading,
  LuImage as Image,
  LuItalic as Italic,
  LuLink as Link,
  LuList as List,
  LuQuote as Quote,
  LuTrash2 as Trash2,
  LuX as X,
} from '@lib/icons'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import { useI18n } from '../../../contexts/I18nContext'
import * as brewApi from '../../../services/brewApi'
import { federationApi } from '../../../services/federationApi'
import { userFacingError } from '../../../utils/userFacingError'
import { Spinner } from '../../Spinner'
import {
  clearNoteDraft,
  draftDiffersFrom,
  prefixLines,
  readNoteDraft,
  wrapSelection,
  writeNoteDraft,
} from './noteDraft'
import '../ui/brew.css'
import './NoteEditor.css'

const PREVIEW_DEBOUNCE_MS = 260
const DRAFT_SAVE_MS = 800

export interface NoteEditorProps {
  noteId?: number
  onClose: () => void
  onSaved: (id: number) => void
  onDeleted?: (id: number) => void
}

type Pane = 'write' | 'preview'

export default function NoteEditor({
  noteId,
  onClose,
  onSaved,
  onDeleted,
}: NoteEditorProps) {
  const { t } = useI18n()
  const draftKey = noteId ?? 'new'

  const [title, setTitle] = useState('')
  const [contentMd, setContentMd] = useState('')
  const [saved, setSaved] = useState({ title: '', contentMd: '' })
  const [loading, setLoading] = useState(Boolean(noteId))
  const [saving, setSaving] = useState(false)
  const [uploading, setUploading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [html, setHtml] = useState('')
  const [previewing, setPreviewing] = useState(false)
  /** 窄屏一栏；宽屏两栏并排，此值不参与布局。 */
  const [pane, setPane] = useState<Pane>('write')

  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const controller = new AbortController()
    const run = async () => {
      if (noteId === undefined) {
        const draft = readNoteDraft('new')
        if (draft) {
          setTitle(draft.title)
          setContentMd(draft.contentMd)
        }
        return
      }
      try {
        const note = await brewApi.getNoteDraft(noteId, controller.signal)
        if (controller.signal.aborted) return
        setSaved({ title: note.title, contentMd: note.content_md })
        // 本地草稿比服务端新才用。
        const draft = readNoteDraft(noteId)
        const useDraft = draftDiffersFrom(draft, {
          title: note.title,
          contentMd: note.content_md,
        })
        setTitle(useDraft && draft ? draft.title : note.title)
        setContentMd(useDraft && draft ? draft.contentMd : note.content_md)
      } catch (err) {
        if (!controller.signal.aborted) {
          setError(userFacingError(err, t.brew.errorLoadFailed))
        }
      } finally {
        if (!controller.signal.aborted) setLoading(false)
      }
    }
    void run()
    return () => {
      controller.abort()
    }
  }, [noteId, t.brew.errorLoadFailed])

  useEffect(() => {
    if (loading) return
    const timer = setTimeout(writeNoteDraft, DRAFT_SAVE_MS, draftKey, { title, contentMd })
    return () => clearTimeout(timer)
  }, [draftKey, title, contentMd, loading])

  // 预览防抖 + AbortController：只落地最后一次。
  useEffect(() => {
    if (loading) return
    if (!contentMd.trim()) {
      setHtml('')
      return
    }
    const controller = new AbortController()
    const timer = setTimeout(async () => {
      setPreviewing(true)
      try {
        const rendered = await brewApi.previewNote(contentMd, controller.signal)
        if (!controller.signal.aborted) setHtml(rendered)
      } catch {
        // 预览失败不打断写作。
      } finally {
        if (!controller.signal.aborted) setPreviewing(false)
      }
    }, PREVIEW_DEBOUNCE_MS)
    return () => {
      clearTimeout(timer)
      controller.abort()
    }
  }, [contentMd, loading])

  const dirty = useMemo(
    () =>
      title.trim() !== saved.title.trim() ||
      contentMd.trim() !== saved.contentMd.trim(),
    [title, contentMd, saved],
  )

  const applyEdit = useCallback(
    (
      fn: (
        value: string,
        start: number,
        end: number,
      ) => { value: string; selectionStart: number; selectionEnd: number },
    ) => {
      const el = textareaRef.current
      if (!el) return
      const result = fn(el.value, el.selectionStart, el.selectionEnd)
      setContentMd(result.value)
      // setState 后等下一帧再设选区。
      requestAnimationFrame(() => {
        el.focus()
        el.setSelectionRange(result.selectionStart, result.selectionEnd)
      })
    },
    [],
  )

  const wrap = useCallback(
    (before: string, after: string, placeholder: string) =>
      applyEdit((v, s, e) => wrapSelection(v, s, e, before, after, placeholder)),
    [applyEdit],
  )

  const prefix = useCallback(
    (mark: string) => applyEdit((v, s, e) => prefixLines(v, s, e, mark)),
    [applyEdit],
  )

  const handleUpload = useCallback(
    async (file: File) => {
      setUploading(true)
      setError(null)
      try {
        const uploaded = await federationApi.uploadMedia(file, {
          filename: file.name,
        })
        wrap(`![${file.name}](${uploaded.url})`, '', '')
      } catch (err) {
        setError(userFacingError(err, t.brew.errorSaveFailed))
      } finally {
        setUploading(false)
      }
    },
    [wrap, t.brew.errorSaveFailed],
  )

  const handleSave = useCallback(async () => {
    if (saving) return
    if (!title.trim()) {
      setError(t.brew.noteTitleRequired)
      return
    }
    setSaving(true)
    setError(null)
    const payload: BrewNoteInput = {
      title: title.trim(),
      content_md: contentMd,
    }
    try {
      const result =
        noteId === undefined
          ? await brewApi.createNote(payload)
          : await brewApi.updateNote(noteId, payload)
      clearNoteDraft(draftKey)
      // 发布后清掉 `new` 草稿位。
      if (noteId === undefined) clearNoteDraft('new')
      onSaved(result.id)
    } catch (err) {
      setError(userFacingError(err, t.brew.errorSaveFailed))
    } finally {
      setSaving(false)
    }
  }, [
    saving,
    title,
    contentMd,
    noteId,
    draftKey,
    onSaved,
    t.brew.noteTitleRequired,
    t.brew.errorSaveFailed,
  ])

  const handleDelete = useCallback(async () => {
    if (noteId === undefined || saving) return

    if (!window.confirm(t.brew.noteDeleteConfirm)) return
    setSaving(true)
    try {
      await brewApi.deleteNote(noteId)
      clearNoteDraft(noteId)
      onDeleted?.(noteId)
    } catch (err) {
      setError(userFacingError(err, t.brew.errorDeleteFailed))
      setSaving(false)
    }
  }, [
    noteId,
    saving,
    onDeleted,
    t.brew.noteDeleteConfirm,
    t.brew.errorDeleteFailed,
  ])

  // Esc 关闭；有未保存改动时先确认。
  const requestClose = useCallback(() => {
    if (dirty && !window.confirm(t.brew.noteDiscardConfirm)) return
    onClose()
  }, [dirty, onClose, t.brew.noteDiscardConfirm])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') requestClose()
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault()
        void handleSave()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [requestClose, handleSave])

  const tools = [
    {
      key: 'heading',
      icon: <Heading className="h-4 w-4" />,
      label: t.brew.noteToolHeading,
      run: () => prefix('## '),
    },
    {
      key: 'bold',
      icon: <Bold className="h-4 w-4" />,
      label: t.brew.noteToolBold,
      run: () => wrap('**', '**', t.brew.noteToolBold),
    },
    {
      key: 'italic',
      icon: <Italic className="h-4 w-4" />,
      label: t.brew.noteToolItalic,
      run: () => wrap('*', '*', t.brew.noteToolItalic),
    },
    {
      key: 'link',
      icon: <Link className="h-4 w-4" />,
      label: t.brew.noteToolLink,
      run: () => wrap('[', '](https://)', t.brew.noteToolLink),
    },
    {
      key: 'code',
      icon: <Code className="h-4 w-4" />,
      label: t.brew.noteToolCode,
      run: () => wrap('\n```\n', '\n```\n', ''),
    },
    {
      key: 'quote',
      icon: <Quote className="h-4 w-4" />,
      label: t.brew.noteToolQuote,
      run: () => prefix('> '),
    },
    {
      key: 'list',
      icon: <List className="h-4 w-4" />,
      label: t.brew.noteToolList,
      run: () => prefix('- '),
    },
  ]

  return createPortal(
    <div className="brew-skin brew-note">
      <div className="brew-note__frame">
        <div className="brew-note__head">
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder={t.brew.noteTitlePlaceholder}
            aria-label={t.brew.noteTitlePlaceholder}
            className="brew-note__title"
          />
          {noteId !== undefined ? (
            <button
              type="button"
              onClick={handleDelete}
              disabled={saving}
              className="brew-note__tool is-danger"
              title={t.brew.noteDelete}
              aria-label={t.brew.noteDelete}
            >
              <Trash2 />
            </button>
          ) : null}
          <button
            type="button"
            onClick={handleSave}
            disabled={saving || loading}
            className="brew-note__publish"
          >
            {saving ? <Spinner size="sm" /> : null}
            {t.brew.notePublish}
          </button>
          <button
            type="button"
            onClick={requestClose}
            className="brew-note__tool"
            title={t.brew.close}
            aria-label={t.brew.close}
          >
            <X />
          </button>
        </div>

        <div className="brew-note__tools">
          {tools.map((tool) => (
            <button
              key={tool.key}
              type="button"
              onClick={tool.run}
              className="brew-note__tool"
              title={tool.label}
              aria-label={tool.label}
            >
              {tool.icon}
            </button>
          ))}
          <button
            type="button"
            onClick={() => fileRef.current?.click()}
            disabled={uploading}
            className="brew-note__tool"
            title={t.brew.noteToolImage}
            aria-label={t.brew.noteToolImage}
          >
            {uploading ? <Spinner size="sm" /> : <Image />}
          </button>
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            className="brew-bar__file"
            onChange={(e) => {
              const file = e.target.files?.[0]
              e.target.value = ''
              if (file) void handleUpload(file)
            }}
          />

          <div className="ml-auto flex items-center gap-0.5 lg:hidden">
            {(['write', 'preview'] as Pane[]).map((value) => (
              <button
                key={value}
                type="button"
                onClick={() => setPane(value)}
                aria-pressed={pane === value}
                className={`brew-note__pane${pane === value ? ' is-on' : ''}`}
              >
                {value === 'write' ? t.brew.noteTabWrite : t.brew.noteTabPreview}
              </button>
            ))}
          </div>
        </div>

        {error ? <div className="brew-note__alert">{error}</div> : null}

        {loading ? (
          <div className="flex flex-1 items-center justify-center">
            <Spinner size="lg" />
          </div>
        ) : (
          <div className="brew-note__body">
            <div
              className={`brew-note__write${pane === 'write' ? '' : ' is-hidden'}`}
            >
              <textarea
                ref={textareaRef}
                value={contentMd}
                onChange={(e) => setContentMd(e.target.value)}
                placeholder={t.brew.noteBodyPlaceholder}
                aria-label={t.brew.noteBodyPlaceholder}
                spellCheck={false}
              />
            </div>
            <div
              className={`brew-note__read${pane === 'preview' ? '' : ' is-hidden'}`}
            >
              {previewing && !html ? (
                <div className="flex justify-center py-8">
                  <Spinner size="md" />
                </div>
              ) : html ? (
                <div
                  className="brew-note-preview"
                  dangerouslySetInnerHTML={{ __html: html }}
                />
              ) : (
                <p className="brew-note__empty">{t.brew.notePreviewEmpty}</p>
              )}
            </div>
          </div>
        )}
      </div>
    </div>,
    document.body,
  )
}
