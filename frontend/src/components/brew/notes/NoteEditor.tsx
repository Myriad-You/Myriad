/**
 * 手记编辑器：工具栏 + 纯文本框 + 服务端预览。
 *
 * 刻意**不做**所见即所得。理由是全站只允许存在一套 Markdown 语法实现 ——
 * 预览调的是后端渲染接口，和发布时用的是同一个函数，所以「预览好看、发出去
 * 变形」在结构上不可能发生。前端一行 Markdown 解析代码都没有。
 *
 * 预览用 `dangerouslySetInnerHTML` 是安全的：那段 HTML 由后端白名单消毒后
 * 才返回，与阅读器里将来渲染的是同一份字节。
 */

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
import './NoteEditor.css'

/** 预览请求的防抖。停手大约四分之一秒后才发，打字过程中不发。 */
const PREVIEW_DEBOUNCE_MS = 260
/** 草稿自动保存的节流。 */
const DRAFT_SAVE_MS = 800

export interface NoteEditorProps {
  /** 要改的那篇；不传就是写新的。 */
  noteId?: number
  onClose: () => void
  /** 保存成功。`id` 是这篇手记的条目 id。 */
  onSaved: (id: number) => void
  /** 删除成功。只有改稿时才可能触发。 */
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
  /** 服务端上已保存的那份，用来判断「有没有未保存的改动」。 */
  const [saved, setSaved] = useState({ title: '', contentMd: '' })
  const [loading, setLoading] = useState(Boolean(noteId))
  const [saving, setSaving] = useState(false)
  const [uploading, setUploading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [html, setHtml] = useState('')
  const [previewing, setPreviewing] = useState(false)
  /** 窄屏一次只显示一栏。宽屏两栏并排，这个值不参与布局。 */
  const [pane, setPane] = useState<Pane>('write')

  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  // 载入原文。新写的直接看有没有本地草稿。
  useEffect(() => {
    let cancelled = false
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
        const note = await brewApi.getNoteDraft(noteId)
        if (cancelled) return
        setSaved({ title: note.title, contentMd: note.content_md })
        // 本地草稿比服务端的新才用它 —— 上次没保存就关掉了标签页
        const draft = readNoteDraft(noteId)
        const useDraft = draftDiffersFrom(draft, {
          title: note.title,
          contentMd: note.content_md,
        })
        setTitle(useDraft && draft ? draft.title : note.title)
        setContentMd(useDraft && draft ? draft.contentMd : note.content_md)
      } catch (err) {
        if (!cancelled) setError(userFacingError(err, t.brew.errorLoadFailed))
      } finally {
        if (!cancelled) setLoading(false)
      }
    }
    void run()
    return () => {
      cancelled = true
    }
  }, [noteId, t.brew.errorLoadFailed])

  // 草稿自动保存
  useEffect(() => {
    if (loading) return
    const timer = setTimeout(writeNoteDraft, DRAFT_SAVE_MS, draftKey, { title, contentMd })
    return () => clearTimeout(timer)
  }, [draftKey, title, contentMd, loading])

  // 预览。防抖 + AbortController：连着打字只会有最后一次请求落地。
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
        // 预览失败不打断写作，保留上一次渲染结果
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

  /** 工具栏统一入口：改文本、把焦点和选区还给文本框。 */
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
      // setState 之后 DOM 还没更新，选区要等下一帧再设
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
      // 新写的那篇发布之后，`new` 草稿位要空出来给下一篇
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

  // Esc 关闭；有未保存改动时先确认
  const requestClose = useCallback(() => {
    if (dirty && !window.confirm(t.brew.noteDiscardConfirm)) return
    onClose()
  }, [dirty, onClose, t.brew.noteDiscardConfirm])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') requestClose()
      // Ctrl/Cmd + S 保存 —— 写字的人手会自己按下去
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault()
        void handleSave()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [requestClose, handleSave])

  const toolbarButton =
    'flex h-8 w-8 items-center justify-center rounded-lg text-gray-500 transition-colors hover:bg-black/5 hover:text-gray-800 dark:text-gray-400 dark:hover:bg-white/8 dark:hover:text-gray-100'

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
    <div className="fixed inset-0 z-9999 flex flex-col bg-black/50 backdrop-blur-sm p-0 sm:p-6">
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-none glass-surface glass-90 shadow-2xl sm:rounded-2xl">
        {/* 标题行 */}
        <div className="flex items-center gap-2 border-b border-gray-200/60 px-3 py-2 dark:border-neutral-700/60 sm:px-4">
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder={t.brew.noteTitlePlaceholder}
            aria-label={t.brew.noteTitlePlaceholder}
            className="min-w-0 flex-1 bg-transparent px-1 py-1.5 text-base font-semibold text-gray-800 outline-hidden placeholder:font-normal placeholder:text-gray-400 dark:text-gray-100"
          />
          {noteId !== undefined && (
            <button
              type="button"
              onClick={handleDelete}
              disabled={saving}
              className={`${toolbarButton} hover:text-red-500! hover:bg-red-500/10!`}
              title={t.brew.noteDelete}
              aria-label={t.brew.noteDelete}
            >
              <Trash2 className="h-4 w-4" />
            </button>
          )}
          <button
            type="button"
            onClick={handleSave}
            disabled={saving || loading}
            className="flex h-8 items-center gap-1.5 rounded-lg bg-linear-to-r from-orange-500 to-amber-500 px-3.5 text-xs font-medium text-white shadow-sm transition-all hover:from-orange-600 hover:to-amber-600 disabled:opacity-60"
          >
            {saving ? <Spinner size="sm" /> : null}
            {t.brew.notePublish}
          </button>
          <button
            type="button"
            onClick={requestClose}
            className={toolbarButton}
            title={t.brew.close}
            aria-label={t.brew.close}
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* 工具栏 */}
        <div className="flex flex-wrap items-center gap-0.5 border-b border-gray-200/60 px-2 py-1 dark:border-neutral-700/60 sm:px-3">
          {tools.map((tool) => (
            <button
              key={tool.key}
              type="button"
              onClick={tool.run}
              className={toolbarButton}
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
            className={toolbarButton}
            title={t.brew.noteToolImage}
            aria-label={t.brew.noteToolImage}
          >
            {uploading ? <Spinner size="sm" /> : <Image className="h-4 w-4" />}
          </button>
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            className="hidden"
            onChange={(e) => {
              const file = e.target.files?.[0]
              // 同一张图连传两次也要触发 change，先把 value 清掉
              e.target.value = ''
              if (file) void handleUpload(file)
            }}
          />

          {/* 窄屏切栏。宽屏两栏并排，这组按钮藏起来 */}
          <div className="ml-auto flex items-center gap-0.5 lg:hidden">
            {(['write', 'preview'] as Pane[]).map((value) => (
              <button
                key={value}
                type="button"
                onClick={() => setPane(value)}
                aria-pressed={pane === value}
                className={`h-8 rounded-lg px-2.5 text-xs font-medium transition-colors ${
                  pane === value
                    ? 'bg-orange-500/10 text-orange-500'
                    : 'text-gray-500 hover:bg-black/5 dark:text-gray-400 dark:hover:bg-white/8'
                }`}
              >
                {value === 'write' ? t.brew.noteTabWrite : t.brew.noteTabPreview}
              </button>
            ))}
          </div>
        </div>

        {error ? (
          <div className="border-b border-red-500/20 bg-red-500/8 px-4 py-2 text-xs text-red-600 dark:text-red-400">
            {error}
          </div>
        ) : null}

        {loading ? (
          <div className="flex flex-1 items-center justify-center">
            <Spinner size="lg" className="text-orange-500" />
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 lg:divide-x lg:divide-gray-200/60 lg:dark:divide-neutral-700/60">
            <div
              className={`min-h-0 flex-1 ${pane === 'write' ? 'flex' : 'hidden'} lg:flex`}
            >
              <textarea
                ref={textareaRef}
                value={contentMd}
                onChange={(e) => setContentMd(e.target.value)}
                placeholder={t.brew.noteBodyPlaceholder}
                aria-label={t.brew.noteBodyPlaceholder}
                spellCheck={false}
                className="h-full w-full resize-none bg-transparent px-4 py-4 font-mono text-sm leading-7 text-gray-700 outline-hidden placeholder:text-gray-400 dark:text-gray-200 sm:px-6"
              />
            </div>
            <div
              className={`min-h-0 flex-1 overflow-y-auto px-4 py-4 sm:px-6 ${
                pane === 'preview' ? 'block' : 'hidden'
              } lg:block`}
            >
              {previewing && !html ? (
                <div className="flex justify-center py-8">
                  <Spinner size="md" className="text-orange-500" />
                </div>
              ) : html ? (
                // 后端已按白名单消毒，这里渲染的就是发布后的那份 HTML
                <div
                  className="brew-note-preview"
                  dangerouslySetInnerHTML={{ __html: html }}
                />
              ) : (
                <p className="text-sm text-gray-400 dark:text-gray-500">
                  {t.brew.notePreviewEmpty}
                </p>
              )}
            </div>
          </div>
        )}
      </div>
    </div>,
    document.body,
  )
}
