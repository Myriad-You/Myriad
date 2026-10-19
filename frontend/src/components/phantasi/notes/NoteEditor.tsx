/** 预览与发布仍走后端渲染。可视层只改 Markdown 原文。 */

import type { NoteEditorProps } from './useNoteEditorSession'
import { useEffect } from 'react'
import { NoteEditorView } from './NoteEditorView'
import { useNoteEditorSession } from './useNoteEditorSession'

export default function NoteEditor({
  framed = false,
  onCloseHandle,
  ...props
}: NoteEditorProps & {
  /** 在创作台窗口里：窗口标题栏有关闭，编辑器自己不再放。 */
  framed?: boolean
  /** 把编辑器自己的关闭交出去：窗口的关闭也要先问未保存的改动、清掉空的新笔记。 */
  onCloseHandle?: (close: (() => void) | null) => void
}) {
  const session = useNoteEditorSession(props)
  const { requestClose } = session
  useEffect(() => {
    onCloseHandle?.(requestClose)
    return () => onCloseHandle?.(null)
  }, [onCloseHandle, requestClose])
  return <NoteEditorView {...session} framed={framed} />
}
