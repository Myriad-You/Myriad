/** 预览与发布仍走后端渲染。可视层只改 Markdown 原文。 */

import type { NoteEditorProps } from './useNoteEditorSession'
import { useEffect } from 'react'
import { NoteEditorView } from './NoteEditorView'
import { useNoteEditorSession } from './useNoteEditorSession'

export default function NoteEditor({
  framed = false,
  hideArticleLink = false,
  onCloseHandle,
  onLeaveHandle,
  ...props
}: NoteEditorProps & {
  /** 在创作台窗口里：窗口标题栏有关闭，编辑器自己不再放。 */
  framed?: boolean
  /** 不放「查看文章」：编辑的就是当前所在的这篇。 */
  hideArticleLink?: boolean
  /** 把编辑器自己的关闭交出去：窗口的关闭也要先问未保存的改动、清掉空的新笔记。 */
  onCloseHandle?: (close: (() => void) | null) => void
  /** 把「换篇之前的检查」交出去：通过了才调 `next`，编辑器本身不关。 */
  onLeaveHandle?: (leave: ((next: () => void) => void) | null) => void
}) {
  const session = useNoteEditorSession(props)
  const { requestClose } = session
  useEffect(() => {
    onCloseHandle?.(requestClose)
    return () => onCloseHandle?.(null)
  }, [onCloseHandle, requestClose])
  const { prepareLeave } = session
  useEffect(() => {
    onLeaveHandle?.((next) => {
      void prepareLeave().then((ok) => {
        if (ok) next()
      })
    })
    return () => onLeaveHandle?.(null)
  }, [onLeaveHandle, prepareLeave])
  return (
    <NoteEditorView
      {...session}
      articleHref={hideArticleLink ? null : session.articleHref}
      framed={framed}
    />
  )
}
