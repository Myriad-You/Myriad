import type {
  Dispatch,
  MutableRefObject,
  MouseEvent as ReactMouseEvent,
  RefObject,
  SetStateAction,
} from 'react'
import type { ShellTranslationKeys } from '../../../i18n/assembleLocale'
import type { WidgetType } from '../../widgetGridTypes'
import type { NoteEditorPane } from './NoteEditorChrome'
import { useCallback, useEffect, useLayoutEffect } from 'react'
import * as phantasiApi from '../../../services/phantasiApi'
import { userFacingError } from '../../../utils/userFacingError'
import { showNoteNotice } from '../phantasiNotice'
import { caretFromPoint } from './noteEditorCaret'
import { prepareNoteReaderHtml } from './noteImageUrl'
import { noteWidgetTypesInMarkdown } from './noteLayout'
import { blockSourceRange, previewClickToMarkdownIndex } from './notePreviewEdit'
import { decorateNoteReadSurface } from './noteReadSurface'
import { captureNoteSelection, restoreNoteSelection } from './noteRemoteSelection'
import {
  blockIndexAt,
  expandJammedDefinitions,
  markdownToVisualHtml,
  markdownToVisualHtmlAsync,
  placeCaretAtTextOffset,
  setNoteWidgetConfig,
  VISUAL_HTML_SYNC_CHARS,
  visualMarkdownStamp,
} from './noteVisual'
import { preloadNoteWidgets } from './noteWidgetCatalog'
import { noteWidgetTypesInHtml } from './noteWidgetHtml'
import { replaceNoteHtml, useNoteWidgetHydration } from './noteWidgetMount'
import { hydrateVisualMath } from './renderMath'

interface Jump {
  pane: 'write' | 'visual'
  index: number
  plainOffset: number
}

export function useNoteEditorPreview(host: {
  t: ShellTranslationKeys
  pane: NoteEditorPane
  loading: boolean
  contentMd: string
  html: string
  htmlRef: MutableRefObject<string>
  previewMdRef: MutableRefObject<string>
  contentMdRef: MutableRefObject<string>
  previewRef: RefObject<HTMLDivElement | null>
  visualRef: RefObject<HTMLDivElement | null>
  textareaRef: RefObject<HTMLTextAreaElement | null>
  visualEditing: MutableRefObject<boolean>
  lastEditPaneRef: MutableRefObject<'write' | 'visual'>
  pendingJumpRef: MutableRefObject<Jump | null>
  widgetCatalog: WidgetType[]
  setHtml: Dispatch<SetStateAction<string>>
  setContentMd: Dispatch<SetStateAction<string>>
  setPreviewing: Dispatch<SetStateAction<boolean>>
  setPane: Dispatch<SetStateAction<NoteEditorPane>>
}) {
  const {
    t,
    pane,
    loading,
    contentMd,
    html,
    htmlRef,
    previewMdRef,
    contentMdRef,
    previewRef,
    visualRef,
    textareaRef,
    visualEditing,
    lastEditPaneRef,
    pendingJumpRef,
    widgetCatalog,
    setHtml,
    setContentMd,
    setPreviewing,
    setPane,
  } = host

  // 预览只在预览栏打。原文对得上就复用，不对就丢掉旧稿再 POST。
  useEffect(() => {
    if (loading || pane !== 'preview') return
    if (!contentMd.trim()) {
      previewMdRef.current = ''
      setHtml('')
      setPreviewing(false)
      return
    }
    const source = expandJammedDefinitions(contentMd)
    if (source !== contentMd) {
      setContentMd(source)
      return
    }
    if (previewMdRef.current === source && htmlRef.current) return
    const controller = new AbortController()
    setPreviewing(true)
    void (async () => {
      try {
        const rendered = await phantasiApi.previewNote(source, controller.signal)
        if (!controller.signal.aborted && contentMdRef.current === source) {
          previewMdRef.current = source
          setHtml(rendered)
        }
      } catch (err) {
        if (!controller.signal.aborted) {
          showNoteNotice(userFacingError(err, t.phantasi.notePreviewFailed))
        }
      } finally {
        if (!controller.signal.aborted) setPreviewing(false)
      }
    })()
    return () => {
      controller.abort()
    }
  }, [
    contentMd,
    contentMdRef,
    htmlRef,
    loading,
    pane,
    previewMdRef,
    setContentMd,
    setHtml,
    setPreviewing,
    t.phantasi.notePreviewFailed,
  ])

  useLayoutEffect(() => {
    if (!html || previewMdRef.current === contentMd) return
    previewMdRef.current = ''
    setHtml('')
  }, [contentMd, html, previewMdRef, setHtml])

  useLayoutEffect(() => {
    if (pane !== 'visual') return
    const el = visualRef.current
    if (!el) return
    // 水合后 face 会改 innerHTML。用原文指纹，打字 / 失焦 / 写栏往返都不整树重挂。
    if (visualEditing.current) {
      visualMarkdownStamp.set(el, contentMd)
      return
    }
    if (visualMarkdownStamp.get(el) === contentMd) return
    const paint = (html: string) => {
      if (visualRef.current !== el || visualEditing.current) return
      if (contentMdRef.current !== contentMd) return
      const selection = captureNoteSelection(el)
      replaceNoteHtml(el, html)
      el.querySelectorAll<HTMLElement>('pre[data-raw-markdown]').forEach((block) => {
        block.title = t.phantasi.noteEditInMarkdown
        block.setAttribute('aria-label', t.phantasi.noteEditInMarkdown)
      })
      restoreNoteSelection(el, selection)
      visualMarkdownStamp.set(el, contentMd)
      void hydrateVisualMath(el)
    }
    if (contentMd.length <= VISUAL_HTML_SYNC_CHARS) {
      paint(markdownToVisualHtml(contentMd))
      return
    }
    const controller = new AbortController()
    void (async () => {
      try {
        const html = await markdownToVisualHtmlAsync(contentMd, controller.signal)
        if (controller.signal.aborted) return
        paint(html)
      } catch {
        if (!controller.signal.aborted) paint(markdownToVisualHtml(contentMd))
      }
    })()
    return () => controller.abort()
  }, [contentMd, contentMdRef, pane, visualEditing, visualRef, t.phantasi.noteEditInMarkdown])

  useLayoutEffect(() => {
    if (pane !== 'preview') return
    const el = previewRef.current
    if (!el) return
    const stale = Boolean(html) && previewMdRef.current !== contentMd
    if (stale) {
      previewMdRef.current = ''
      setHtml('')
    }
    const source = html && !stale ? prepareNoteReaderHtml(html, '') : ''
    const stamp = stale ? '' : html
    if (el.dataset.noteRead === stamp) return
    replaceNoteHtml(el, source)
    if (source) decorateNoteReadSurface(el, t.phantasi.copyCode, t.phantasi.copyTex)
    el.dataset.noteRead = stamp
  }, [contentMd, html, pane, previewMdRef, previewRef, setHtml, t.phantasi.copyCode, t.phantasi.copyTex])

  if (pane === 'preview') preloadNoteWidgets(noteWidgetTypesInHtml(html))
  else preloadNoteWidgets(noteWidgetTypesInMarkdown(contentMd))

  const persistVisualWidgetConfig = useCallback(
    (hostEl: HTMLElement, config: Parameters<typeof setNoteWidgetConfig>[2]) => {
      const root = visualRef.current
      if (!root) return
      visualEditing.current = true
      setContentMd(setNoteWidgetConfig(root, hostEl, config))
    },
    [setContentMd, visualEditing, visualRef],
  )
  const visualWidgets = useNoteWidgetHydration(
    visualRef,
    widgetCatalog,
    pane,
    pane !== 'preview',
    {
      editable: true,
      onConfigChange: persistVisualWidgetConfig,
    },
  )
  const previewWidgets = useNoteWidgetHydration(
    previewRef,
    widgetCatalog,
    pane,
    pane === 'preview',
  )

  if (pane !== 'preview') lastEditPaneRef.current = pane

  /**
   * 点预览即编辑：预览里点到哪一块的哪个字，就切回上一种输入法，把光标放到
   * Markdown 里对应的位置。块级靠后端打的原文区间，字级靠纯文本对齐。
   */
  const jumpFromPreview = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>) => {
      const root = previewRef.current
      const target = event.target as HTMLElement
      if (!root || !root.contains(target)) return
      // 水合后面的字不在原文里。点小组件只回到这一块的 :::widget，不对齐 face。
      const widget = target.closest<HTMLElement>('.note-widget')
      const block = widget?.hasAttribute('data-md-start')
        ? widget
        : target.closest<HTMLElement>('[data-md-start]')
      if (!block || !root.contains(block)) return
      const range = blockSourceRange(contentMdRef.current, {
        start: block.dataset.mdStart,
        end: block.dataset.mdEnd,
      })
      if (!range) return
      event.preventDefault()
      const caret = caretFromPoint(document, event.clientX, event.clientY)
      let plainPrefix = ''
      if (!widget && caret && block.contains(caret.node)) {
        const measure = document.createRange()
        measure.setStart(block, 0)
        measure.setEnd(caret.node, caret.offset)
        plainPrefix = measure.toString()
      }
      const index = previewClickToMarkdownIndex(contentMdRef.current, range, plainPrefix)
      pendingJumpRef.current = {
        pane: lastEditPaneRef.current,
        index,
        plainOffset: plainPrefix.length,
      }
      setPane(lastEditPaneRef.current)
    },
    [contentMdRef, lastEditPaneRef, pendingJumpRef, previewRef, setPane],
  )

  // 换栏后只放光标，不改滚动。默认看得见的那一截也可以溢出去。
  useLayoutEffect(() => {
    const jump = pendingJumpRef.current
    if (!jump || jump.pane !== pane) return
    pendingJumpRef.current = null
    if (jump.pane === 'write') {
      const el = textareaRef.current
      if (!el) return
      el.focus({ preventScroll: true })
      el.setSelectionRange(jump.index, jump.index)
      return
    }
    const root = visualRef.current
    if (!root) return
    const block = root.children[blockIndexAt(contentMdRef.current, jump.index)] as
      | HTMLElement
      | undefined
    root.focus({ preventScroll: true })
    if (block) placeCaretAtTextOffset(root, block, jump.plainOffset)
  }, [contentMdRef, pane, pendingJumpRef, textareaRef, visualRef])

  return { visualWidgets, previewWidgets, jumpFromPreview }
}
