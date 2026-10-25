import type { RefObject } from 'react'
import type { NoteAgentDoc } from './noteAgentPage'
import { useEffect } from 'react'
import { setCurrentPageContent } from '../../../contexts/currentPage'
import { rememberAgentSelection } from '../../agent-panel/agentSelection'
import { noteAgentPageContent, noteAgentSelection } from './noteAgentPage'

/** Title keystrokes settle before the Agent's page context follows them. */
const PAGE_SETTLE_MS = 300

/**
 * Makes the open note the global Agent's page. The visual pane's selections reach the Agent
 * through the document selection it already watches; the source textarea's do not, so they
 * are handed over here.
 */
export function useNoteAgentPage(doc: NoteAgentDoc, textareaRef: RefObject<HTMLTextAreaElement | null>): void {
  const { docId, title, topic, status } = doc
  useEffect(() => {
    const timer = window.setTimeout(
      () => setCurrentPageContent(noteAgentPageContent({ docId, title, topic, status })),
      PAGE_SETTLE_MS,
    )
    return () => window.clearTimeout(timer)
  }, [docId, title, topic, status])

  // Leaving the editor leaves no stale note behind as "the page".
  useEffect(() => () => setCurrentPageContent(null), [])

  useEffect(() => {
    const capture = () => {
      const el = textareaRef.current
      if (!el || document.activeElement !== el) return
      const text = noteAgentSelection(el.value, el.selectionStart, el.selectionEnd)
      if (text) rememberAgentSelection(text)
    }
    document.addEventListener('selectionchange', capture)
    document.addEventListener('select', capture, true)
    return () => {
      document.removeEventListener('selectionchange', capture)
      document.removeEventListener('select', capture, true)
    }
  }, [textareaRef])
}
