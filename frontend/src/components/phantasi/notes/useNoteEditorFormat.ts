import type { Dispatch, MutableRefObject, RefObject, SetStateAction } from 'react'
import type { ShellTranslationKeys } from '../../../i18n/assembleLocale'
import type { WidgetType } from '../../widgetGridTypes'
import type { InlineLink } from './noteDraft'
import type { NoteInsertMenuState, TableAlign } from './NoteEditorChrome'
import type { SelectionAnchor } from './noteSelection'
import { useCallback, useEffect } from 'react'
import { mediaAssetSrc, uploadMedia } from '../../../services/mediaApi'
import * as phantasiApi from '../../../services/phantasiApi'
import { userFacingError } from '../../../utils/userFacingError'
import { showNoteNotice } from '../phantasiNotice'
import {
  collectNoteCategories,
  normalizeNoteCategory,
} from './noteCategory'
import {
  linkAtCursor,
  openLineBelow,
  replaceLink,
  setHeadingLevel,
} from './noteDraft'
import { NOTE_URL_LIKE, revealInContainer } from './noteEditorCaret'
import { anchorInContainer } from './noteSelection'
import {
  beginVisualMathEdit,
  currentColumnAlign,
  insertImage,
  insertWidgetMarkdown,
  insertWidgetVisual,
  setImageSrc,
  setCodeLang as setVisualCodeLang,
  toggleVisualHeading,
  toggleVisualInlineCode,
  visualClosest,
  visualClosestClass,
  visualOpenBlockBelow,
  wrapVisualMath,
} from './noteVisual'
import { hydrateVisualMath } from './renderMath'

type PaneRef = MutableRefObject<'write' | 'visual' | 'preview'>

export function useNoteEditorFormat(host: {
  t: ShellTranslationKeys
  visualRef: RefObject<HTMLDivElement | null>
  textareaRef: RefObject<HTMLTextAreaElement | null>
  scrollRef: RefObject<HTMLDivElement | null>
  paneRef: PaneRef
  selectedImageRef: MutableRefObject<HTMLImageElement | null>
  selectedWidgetRef: MutableRefObject<HTMLElement | null>
  activeBlockRef: MutableRefObject<HTMLElement | null>
  savedRangeRef: MutableRefObject<Range | null>
  editingLinkRef: MutableRefObject<InlineLink | null>
  overlayOpenRef: MutableRefObject<boolean>
  selectionRef: MutableRefObject<SelectionAnchor | null>
  widgetCatalog: WidgetType[]
  visualWidgets: { refresh: () => void }
  commitVisualMd: (md: string) => void
  runVisual: (command: string, value?: string) => void
  visualHistoryStep: (dir: 'undo' | 'redo') => void
  insertText: (before: string, after: string, placeholder: string) => void
  applyEdit: (
    fn: (
      value: string,
      start: number,
      end: number,
    ) => { value: string; selectionStart: number; selectionEnd: number },
  ) => void
  wrap: (left: string, right: string, placeholder: string) => void
  prefix: (mark: string) => void
  runTool: (markdown: () => void, visual?: { command: string; value?: string }) => void
  focusBody: () => void
  requestClose: () => void
  handleSave: () => void | Promise<void>
  setContentMd: Dispatch<SetStateAction<string>>
  setCover: Dispatch<SetStateAction<string | null>>
  setUploading: Dispatch<SetStateAction<boolean>>
  setTopic: Dispatch<SetStateAction<string | null>>
  setCategoryNames: Dispatch<SetStateAction<string[]>>
  setSettingsOpen: Dispatch<SetStateAction<boolean>>
  setInsertMenu: Dispatch<SetStateAction<NoteInsertMenuState>>
  setWidgetSettingsOpen: Dispatch<SetStateAction<boolean>>
  setLinkOpen: Dispatch<SetStateAction<boolean>>
  setLinkInitial: Dispatch<SetStateAction<string>>
  setSelectedImage: Dispatch<SetStateAction<{ anchor: SelectionAnchor; alt: string; src: string; href: string } | null>>
  setSelectedWidget: Dispatch<SetStateAction<{ anchor: SelectionAnchor; type: string; size: string } | null>>
  setCodeLang: Dispatch<SetStateAction<string>>
  setColumnAlign: Dispatch<SetStateAction<TableAlign>>
  block: { kind: string } | null
  contentMd: string
}) {
  const {
    t, visualRef, textareaRef, scrollRef, paneRef, selectedImageRef, selectedWidgetRef,
    activeBlockRef, savedRangeRef, editingLinkRef, overlayOpenRef, selectionRef,
    widgetCatalog, visualWidgets, commitVisualMd, runVisual, visualHistoryStep,
    insertText, applyEdit, wrap, prefix, runTool, focusBody, requestClose, handleSave,
    setContentMd, setCover, setUploading, setTopic, setCategoryNames, setSettingsOpen,
    setInsertMenu, setWidgetSettingsOpen, setLinkOpen, setLinkInitial,
    setSelectedImage, setSelectedWidget, setCodeLang, setColumnAlign, block, contentMd,
  } = host

  /** 往正文插一张图：可视层插 `<img>`，写栏插 Markdown。 */
  const placeImage = useCallback(
    (src: string, alt: string) => {
      const root = visualRef.current
      if (paneRef.current === 'visual' && root) {
        commitVisualMd(insertImage(root, src, alt))
        return
      }
      insertText(`![${alt}](${src})`, '', '')
    },
    [insertText, commitVisualMd],
  )

  const handleUpload = useCallback(
    async (file: File, as: 'body' | 'cover' | 'replace') => {
      setUploading(true)
      try {
        const uploaded = await uploadMedia(file)
        const src = mediaAssetSrc(uploaded)
        if (as === 'cover') {
          setCover(src)
        } else if (as === 'replace') {
          const root = visualRef.current
          const img = selectedImageRef.current
          if (root && img) commitVisualMd(setImageSrc(root, img, src))
        } else {
          placeImage(src, file.name)
        }
      } catch (err) {
        showNoteNotice(userFacingError(err, t.phantasi.errorSaveFailed))
      } finally {
        setUploading(false)
      }
    },
    [placeImage, t.phantasi.errorSaveFailed, commitVisualMd],
  )

  // ---- 格式动作：写栏改 Markdown，可视层改 DOM。快捷键、浮动条、菜单都走这几只 ----

  const heading = useCallback(
    (level: number) => {
      const root = visualRef.current
      if (paneRef.current === 'visual' && root) {
        commitVisualMd(toggleVisualHeading(root, level))
        return
      }
      applyEdit((v, s, e) => setHeadingLevel(v, s, e, level))
    },
    [applyEdit, commitVisualMd],
  )

  const inlineCode = useCallback(() => {
    const root = visualRef.current
    if (paneRef.current === 'visual' && root) {
      commitVisualMd(toggleVisualInlineCode(root))
      return
    }
    wrap('`', '`', t.phantasi.noteToolInlineCode)
  }, [wrap, t.phantasi.noteToolInlineCode, commitVisualMd])

  const finishMath = useCallback(
    (root: HTMLElement, md: string) => {
      commitVisualMd(md)
      void hydrateVisualMath(root)
    },
    [commitVisualMd],
  )

  const inlineMath = useCallback(() => {
    const root = visualRef.current
    if (paneRef.current === 'visual' && root) {
      commitVisualMd(wrapVisualMath(root, false))
      const last = [...root.querySelectorAll<HTMLElement>('.note-math-inline')].at(-1)
      if (last && !last.dataset.tex) {
        beginVisualMathEdit(root, last, (md) => finishMath(root, md))
      } else {
        void hydrateVisualMath(root)
      }
      return
    }
    wrap('$', '$', t.phantasi.noteMathPlaceholder)
  }, [wrap, t.phantasi.noteMathPlaceholder, commitVisualMd, finishMath])

  const displayMath = useCallback(() => {
    const root = visualRef.current
    if (paneRef.current === 'visual' && root) {
      commitVisualMd(wrapVisualMath(root, true))
      const last = [...root.querySelectorAll<HTMLElement>('.note-math-display')].at(-1)
      if (last && !last.dataset.tex) {
        beginVisualMathEdit(root, last, (md) => finishMath(root, md))
      } else {
        void hydrateVisualMath(root)
      }
      return
    }
    insertText('\n$$\n', '\n$$\n', t.phantasi.noteMathPlaceholder)
  }, [insertText, t.phantasi.noteMathPlaceholder, commitVisualMd, finishMath])

  const bold = useCallback(
    () => runTool(() => wrap('**', '**', t.phantasi.noteToolBold), { command: 'bold' }),
    [runTool, wrap, t.phantasi.noteToolBold],
  )
  const italic = useCallback(
    () => runTool(() => wrap('*', '*', t.phantasi.noteToolItalic), { command: 'italic' }),
    [runTool, wrap, t.phantasi.noteToolItalic],
  )
  const strike = useCallback(
    () =>
      runTool(() => wrap('~~', '~~', t.phantasi.noteToolStrike), {
        command: 'strikeThrough',
      }),
    [runTool, wrap, t.phantasi.noteToolStrike],
  )
  const bulletList = useCallback(
    () => runTool(() => prefix('- '), { command: 'insertUnorderedList' }),
    [runTool, prefix],
  )
  const orderedList = useCallback(
    () => runTool(() => prefix('1. '), { command: 'insertOrderedList' }),
    [runTool, prefix],
  )
  const taskList = useCallback(
    () =>
      runTool(() => prefix('- [ ] '), {
        command: 'insertHTML',
        value: '<ul data-task="1"><li data-task="0"><br></li></ul>',
      }),
    [runTool, prefix],
  )

  /** 链接地址栏。可视层先记住选区，输入框抢焦点后还能放回去。 */
  const openLink = useCallback(() => {
    const root = visualRef.current
    editingLinkRef.current = null
    if (paneRef.current === 'visual' && root) {
      const selection = document.getSelection()
      savedRangeRef.current =
        selection && selection.rangeCount > 0
          ? selection.getRangeAt(0).cloneRange()
          : null
      setLinkInitial(visualClosest(root, 'a')?.getAttribute('href') ?? '')
    } else {
      const el = textareaRef.current
      const existing = el ? linkAtCursor(el.value, el.selectionStart) : null
      editingLinkRef.current = existing
      setLinkInitial(existing?.url ?? '')
    }
    setLinkOpen(true)
  }, [])

  const rememberVisualRange = useCallback(() => {
    const selection = document.getSelection()
    savedRangeRef.current =
      selection && selection.rangeCount > 0
        ? selection.getRangeAt(0).cloneRange()
        : null
  }, [])

  const restoreVisualRange = useCallback(() => {
    const selection = document.getSelection()
    const range = savedRangeRef.current
    if (selection && range) {
      selection.removeAllRanges()
      selection.addRange(range)
    }
  }, [])

  const applyLink = useCallback(
    (url: string | null) => {
      setLinkOpen(false)
      if (paneRef.current === 'visual') {
        restoreVisualRange()
        if (url) runVisual('createLink', url)
        else runVisual('unlink')
        return
      }
      const editing = editingLinkRef.current
      if (editing) {
        editingLinkRef.current = null
        applyEdit((v) => replaceLink(v, editing, url))
        return
      }
      if (url) insertText('[', `](${url})`, t.phantasi.noteToolLink)
      else textareaRef.current?.focus()
    },
    [restoreVisualRange, runVisual, applyEdit, insertText, t.phantasi.noteToolLink],
  )

  /** 选中了字再粘一个网址：直接变链接，不是替换文字。 */
  const pasteAsLink = useCallback(
    (text: string): boolean => {
      const url = text.trim()
      if (!NOTE_URL_LIKE.test(url)) return false
      const root = visualRef.current
      if (paneRef.current === 'visual' && root) {
        const selection = document.getSelection()
        if (!selection || selection.isCollapsed) return false
        runVisual('createLink', url)
        return true
      }
      const el = textareaRef.current
      if (!el || el.selectionStart === el.selectionEnd) return false
      insertText('[', `](${url})`, '')
      return true
    },
    [runVisual, insertText],
  )

  /** 点中一张图：记住它，块工具条切到图片。 */
  const selectImage = useCallback((img: HTMLImageElement | null) => {
    selectedImageRef.current?.classList.remove('is-selected')
    img?.classList.add('is-selected')
    selectedImageRef.current = img
    const container = scrollRef.current
    if (!img || !container) {
      setSelectedImage(null)
      return
    }
    setSelectedImage({
      anchor: anchorInContainer(img.getBoundingClientRect(), container),
      alt: img.alt,
      src: img.dataset.src ?? img.getAttribute('src') ?? '',
      href: img.closest('a')?.getAttribute('href') ?? '',
    })
  }, [])

  const selectWidget = useCallback((widget: HTMLElement | null) => {
    selectedWidgetRef.current?.classList.remove('is-selected')
    widget?.classList.add('is-selected')
    selectedWidgetRef.current = widget
    const container = scrollRef.current
    if (!widget || !container) {
      setSelectedWidget(null)
      setWidgetSettingsOpen(false)
      visualWidgets.refresh()
      return
    }
    setSelectedWidget({
      anchor: anchorInContainer(widget.getBoundingClientRect(), container),
      type: widget.dataset.widget ?? '',
      size: widget.dataset.size ?? '2x2',
    })
    visualWidgets.refresh()
  }, [visualWidgets.refresh])

  // 正文变了（撤销、远端合并、换图）：选中的图要么已经不在树上了，要么位置挪了。
  useEffect(() => {
    const img = selectedImageRef.current
    if (!img) return
    if (!img.isConnected) {
      selectImage(null)
      return
    }
    const container = scrollRef.current
    if (!container) return
    const anchor = anchorInContainer(img.getBoundingClientRect(), container)
    const src = img.dataset.src ?? img.getAttribute('src') ?? ''
    const href = img.closest('a')?.getAttribute('href') ?? ''
    setSelectedImage((current) =>
      current && current.anchor.top === anchor.top && current.anchor.left === anchor.left &&
      current.anchor.width === anchor.width && current.anchor.height === anchor.height &&
      current.alt === img.alt && current.src === src && current.href === href
        ? current : { anchor, alt: img.alt, src, href },
    )
  }, [contentMd, selectImage])

  const imageOp = useCallback(
    (fn: (root: HTMLElement, img: HTMLImageElement) => string, keep = true) => {
      const root = visualRef.current
      const img = selectedImageRef.current
      if (!root || !img || !root.contains(img)) return
      commitVisualMd(fn(root, img))
      selectImage(keep ? img : null)
    },
    [selectImage, commitVisualMd],
  )

  const footnoteJump = useCallback((root: HTMLElement, target: HTMLElement) => {
    const ref = target.closest<HTMLElement>('sup[data-fnref]')
    if (!ref) return false
    const definition = root.querySelector<HTMLElement>(`p[data-fn="${ref.dataset.fnref}"]`)
    if (definition && scrollRef.current) {
      revealInContainer(scrollRef.current, definition.getBoundingClientRect())
    }
    return Boolean(definition)
  }, [])

  const closeOverlays = useCallback(() => {
    setSettingsOpen(false)
    setInsertMenu(null)
    setWidgetSettingsOpen(false)
    setLinkOpen(false)
    if (paneRef.current === 'visual') restoreVisualRange()
    focusBody()
  }, [restoreVisualRange, focusBody])

  // 光标进了表格 / 代码块，记住那个元素；语言输入框抢焦点时选区已经不在里面了。
  useEffect(() => {
    const root = visualRef.current
    if (!block || !root) {
      activeBlockRef.current = null
      setCodeLang('')
      return
    }
    const el =
      block.kind === 'columns'
        ? visualClosestClass(root, 'note-columns')
        : block.kind === 'widget'
          ? visualClosestClass(root, 'note-widget')
          : visualClosest(root, block.kind as 'table' | 'pre')
    activeBlockRef.current = el
    setCodeLang(el?.dataset.lang ?? '')
    setColumnAlign(el instanceof HTMLTableElement ? currentColumnAlign(root, el) : null)
  }, [block])

  const tableOp = useCallback(
    (fn: (root: HTMLElement, table: HTMLTableElement) => string) => {
      const root = visualRef.current
      const table = activeBlockRef.current
      if (!root || !(table instanceof HTMLTableElement)) return
      commitVisualMd(fn(root, table))
    },
    [commitVisualMd],
  )

  const columnsOp = useCallback(
    (fn: (root: HTMLElement, columns: HTMLElement) => string) => {
      const root = visualRef.current
      const columns = activeBlockRef.current
      if (!root || !columns?.classList.contains('note-columns')) return
      commitVisualMd(fn(root, columns))
    },
    [commitVisualMd],
  )

  const widgetOp = useCallback(
    (fn: (root: HTMLElement, widget: HTMLElement) => string, keep = true) => {
      const root = visualRef.current
      const widget = selectedWidgetRef.current ?? activeBlockRef.current
      if (!root || !widget?.classList.contains('note-widget')) return
      commitVisualMd(fn(root, widget))
      if (!keep) selectWidget(null)
      else visualWidgets.refresh()
    },
    [selectWidget, commitVisualMd, visualWidgets.refresh],
  )

  const changeCodeLang = useCallback((lang: string) => {
    setCodeLang(lang)
    const root = visualRef.current
    const pre = activeBlockRef.current
    if (root && pre instanceof HTMLPreElement) {
      commitVisualMd(setVisualCodeLang(root, pre, lang))
    }
  }, [commitVisualMd])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopImmediatePropagation()
        if (overlayOpenRef.current) {
          closeOverlays()
          return
        }
        requestClose()
        return
      }
      const mod = e.ctrlKey || e.metaKey
      if (!mod) return
      const key = e.key.toLowerCase()
      // 可视层的撤销走自己的栈，把绕过 execCommand 的改动也管住。
      if (key === 'z' && !e.altKey && paneRef.current === 'visual') {
        e.preventDefault()
        visualHistoryStep(e.shiftKey ? 'redo' : 'undo')
        return
      }
      if (!e.shiftKey && !e.altKey) {
        if (key === 's') {
          e.preventDefault()
          void handleSave()
        } else if (key === 'b') {
          e.preventDefault()
          bold()
        } else if (key === 'i') {
          e.preventDefault()
          italic()
        } else if (key === 'e') {
          e.preventDefault()
          inlineCode()
        } else if (key === 'k') {
          e.preventDefault()
          if (selectionRef.current) openLink()
          else if (paneRef.current === 'visual') runVisual('createLink', 'https://')
          else wrap('[', '](https://)', t.phantasi.noteToolLink)
        }
        return
      }
      if (e.shiftKey && !e.altKey) {
        if (key === 'x') {
          e.preventDefault()
          strike()
        } else if (key === 'm') {
          e.preventDefault()
          inlineMath()
        } else if (e.code === 'Digit7') {
          e.preventDefault()
          orderedList()
        } else if (e.code === 'Digit8') {
          e.preventDefault()
          bulletList()
        } else if (e.code === 'Digit9') {
          e.preventDefault()
          taskList()
        }
        return
      }
      if (e.altKey && !e.shiftKey) {
        const level = /^Digit([1-6])$/.exec(e.code)?.[1]
        if (level) {
          e.preventDefault()
          heading(Number(level))
        }
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [
    requestClose,
    closeOverlays,
    handleSave,
    bold,
    italic,
    inlineCode,
    inlineMath,
    strike,
    orderedList,
    bulletList,
    taskList,
    heading,
    openLink,
    visualHistoryStep,
    wrap,
    runVisual,
    t.phantasi.noteToolLink,
  ])

  /**
   * 「+」在有字的行上也能用：标题 / 列表 / 引用这类作用在当前行；
   * 图片 / 代码块 / 表格 / 分隔线 / 脚注这类块级插入先在下面开一行再落。
   */
  const openBlockBelow = useCallback(() => {
    const root = visualRef.current
    if (paneRef.current === 'visual' && root) {
      visualOpenBlockBelow(root)
      return
    }
    const el = textareaRef.current
    if (!el) return
    const opened = openLineBelow(el.value, el.selectionStart)
    if (opened.value !== el.value) {
      // 直接改 DOM 值，后面的工具读的是 el.value，不用等一帧。
      el.setRangeText('\n', opened.caret - 1, opened.caret - 1, 'end')
      setContentMd(el.value)
    }
    el.setSelectionRange(opened.caret, opened.caret)
  }, [])

  const placeWidget = useCallback(
    (type: string, size: string) => {
      const root = visualRef.current
      if (paneRef.current === 'visual' && root) {
        restoreVisualRange()
        openBlockBelow()
        commitVisualMd(insertWidgetVisual(root, type, size))
        const last = [...root.querySelectorAll<HTMLElement>('.note-widget')].at(-1)
        if (last) {
          selectWidget(last)
          const entry = widgetCatalog.find((item) => item.id === type)
          if (entry?.settings?.length) setWidgetSettingsOpen(true)
        }
        return
      }
      openBlockBelow()
      insertText(insertWidgetMarkdown(type, size), '', '')
    },
    [insertText, openBlockBelow, restoreVisualRange, selectWidget, widgetCatalog, commitVisualMd],
  )

  const handleCreateTopic = useCallback((name: string) => {
    const next = normalizeNoteCategory(name)
    if (!next) return
    setTopic(next)
    setCategoryNames((prev) =>
      collectNoteCategories([...prev.map((item) => ({ topic: item })), { topic: next }]),
    )
    void phantasiApi
      .createCategory({ name: next })
      .catch((err) => {
        showNoteNotice(
          userFacingError(err, t.phantasi.errorSaveFailed),
        )
      })
  }, [t.phantasi.errorSaveFailed])

  return {
    placeImage,
    handleUpload,
    heading,
    finishMath,
    displayMath,
    bold,
    italic,
    strike,
    inlineCode,
    inlineMath,
    openLink,
    rememberVisualRange,
    restoreVisualRange,
    applyLink,
    pasteAsLink,
    selectImage,
    selectWidget,
    imageOp,
    footnoteJump,
    closeOverlays,
    tableOp,
    columnsOp,
    widgetOp,
    changeCodeLang,
    openBlockBelow,
    placeWidget,
    handleCreateTopic,
    bulletList,
    orderedList,
    taskList,
  }
}
