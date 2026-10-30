/**
 * 监听写栏 / 可视层：选中了字给浮动条锚点（和选区上已有的记号），光标停在空行给
 * 行槛「+」锚点，光标落在表格 / 代码块里给块工具条锚点。
 * `hold` 为真时锚点冻住，焦点挪到菜单或输入框上也不消失；一放开立刻重量一次。
 * 编辑器失焦、切换面板、卸载：全部清空，不留任何浮着的东西。
 */

import type { RefObject } from 'react'
import type { NoteEditorPane } from './NoteEditorChrome'
import type { NoteMark } from './noteMarks'
import type { SelectionAnchor, VisualBlockKind } from './noteSelection'
import { useEffect, useState } from 'react'
import { markdownMarksAt, sameMarks, visualMarksAt } from './noteMarks'
import {
  anchorInContainer,
  releaseTextareaMirror,
  textareaCaretLineRect,
  textareaSelectionRect,
  visualBlockRect,
  visualCaretLineRect,
  visualSelectionRect,
} from './noteSelection'

export interface NoteSelectionState {
  selection: SelectionAnchor | null
  marks: Set<NoteMark>
  caretLine: SelectionAnchor | null
  block: { kind: VisualBlockKind; anchor: SelectionAnchor } | null
}

const NO_MARKS: Set<NoteMark> = new Set()
const EMPTY: NoteSelectionState = {
  selection: null,
  marks: NO_MARKS,
  caretLine: null,
  block: null,
}

function sameAnchor(a: SelectionAnchor | null, b: SelectionAnchor | null): boolean {
  if (a === b) return true
  if (!a || !b) return false
  return a.top === b.top && a.left === b.left && a.width === b.width && a.height === b.height
}

function sameState(a: NoteSelectionState, b: NoteSelectionState): boolean {
  return (
    sameAnchor(a.selection, b.selection) &&
    sameAnchor(a.caretLine, b.caretLine) &&
    (a.block?.kind ?? null) === (b.block?.kind ?? null) &&
    sameAnchor(a.block?.anchor ?? null, b.block?.anchor ?? null) &&
    sameMarks(a.marks, b.marks)
  )
}

export function useNoteSelection(
  pane: NoteEditorPane,
  scrollRef: RefObject<HTMLElement | null>,
  textareaRef: RefObject<HTMLTextAreaElement | null>,
  visualRef: RefObject<HTMLElement | null>,
  active: boolean,
  hold: boolean,
  /** 焦点从正文移进这里（块工具条的输入框）不算离开：条子要留着，不然按下鼠标它就没了，输入框点不中。 */
  keepRef?: RefObject<HTMLElement | null>,
): NoteSelectionState {
  const [state, setState] = useState<NoteSelectionState>(EMPTY)

  useEffect(() => {
    if (!active || pane === 'preview') {
      setState(EMPTY)
      return
    }
    if (hold) return
    const container = scrollRef.current
    const textarea = textareaRef.current
    const visual = visualRef.current
    if (!container) return
    let frame = 0
    const commit = (next: NoteSelectionState) => {
      setState((current) => (sameState(current, next) ? current : next))
    }
    const measure = () => {
      // 焦点在块工具条里：正文的选区没变，条子照旧。
      if (keepRef?.current?.contains(document.activeElement)) return
      let selection: DOMRect | null = null
      let caretLine: DOMRect | null = null
      let block: { kind: VisualBlockKind; rect: DOMRect } | null = null
      let marks: Set<NoteMark> = NO_MARKS
      if (pane === 'visual') {
        if (visual && document.activeElement === visual) {
          selection = visualSelectionRect(visual)
          caretLine = visualCaretLineRect(visual)
          block = visualBlockRect(visual)
          if (selection) marks = visualMarksAt(visual)
        }
      } else if (textarea && document.activeElement === textarea) {
        selection = textareaSelectionRect(textarea)
        caretLine = textareaCaretLineRect(textarea)
        if (selection) {
          marks = markdownMarksAt(textarea.value, textarea.selectionStart, textarea.selectionEnd)
        }
      }
      commit({
        selection: selection ? anchorInContainer(selection, container) : null,
        marks,
        caretLine: caretLine ? anchorInContainer(caretLine, container) : null,
        block: block
          ? { kind: block.kind, anchor: anchorInContainer(block.rect, container) }
          : null,
      })
    }
    const schedule = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(measure)
    }
    const hide = () => {
      cancelAnimationFrame(frame)
      setState(EMPTY)
    }
    const leaveEditor = (event: FocusEvent) => {
      const next = event.relatedTarget
      if (next instanceof Node && keepRef?.current?.contains(next)) return
      hide()
    }
    const editor: HTMLElement | null = pane === 'visual' ? visual : textarea
    document.addEventListener('selectionchange', schedule)
    window.addEventListener('resize', schedule)
    window.addEventListener('blur', hide)
    editor?.addEventListener('input', schedule)
    editor?.addEventListener('keyup', schedule)
    editor?.addEventListener('mouseup', schedule)
    editor?.addEventListener('focus', schedule)
    editor?.addEventListener('blur', leaveEditor)
    schedule()
    return () => {
      cancelAnimationFrame(frame)
      document.removeEventListener('selectionchange', schedule)
      window.removeEventListener('resize', schedule)
      window.removeEventListener('blur', hide)
      editor?.removeEventListener('input', schedule)
      editor?.removeEventListener('keyup', schedule)
      editor?.removeEventListener('mouseup', schedule)
      editor?.removeEventListener('focus', schedule)
      editor?.removeEventListener('blur', leaveEditor)
      releaseTextareaMirror()
    }
  }, [pane, active, hold, scrollRef, textareaRef, visualRef, keepRef])

  return state
}
