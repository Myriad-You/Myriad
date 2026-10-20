/** 预览与发布仍走后端渲染。可视层只改 Markdown 原文。 */

import type { NoteEditorProps } from './useNoteEditorSession'
import { useEffect } from 'react'
import { NoteEditorView } from './NoteEditorView'
import { useNoteEditorSession } from './useNoteEditorSession'

export default function NoteEditor({
  framed = false,
  hideArticleLink = false,
  onGuard,
  onUnsaved,
  ...props
}: NoteEditorProps & {
  /** 在创作台窗口里：窗口标题栏有关闭，编辑器自己不再放。 */
  framed?: boolean
  /** 不放「查看文章」：编辑的就是当前所在的这篇。 */
  hideArticleLink?: boolean
  /**
   * 把「离开之前的检查」交出去（问未保存的改动、清掉空的新笔记）：
   * 外面关窗口、换篇之前都先过它，给 true 才关、才换。
   */
  onGuard?: (check: (() => Promise<boolean>) | null) => void
  /** 有没有没保存的改动；不问人，外面据此决定能不能直接刷新页面。 */
  onUnsaved?: (has: boolean) => void
}) {
  const session = useNoteEditorSession(props)
  const { prepareLeave, dirty } = session
  useEffect(() => {
    onGuard?.(prepareLeave)
    return () => onGuard?.(null)
  }, [onGuard, prepareLeave])
  useEffect(() => {
    onUnsaved?.(dirty)
  }, [onUnsaved, dirty])
  useEffect(() => () => onUnsaved?.(false), [onUnsaved])
  return (
    <NoteEditorView
      {...session}
      articleHref={hideArticleLink ? null : session.articleHref}
      framed={framed}
    />
  )
}
