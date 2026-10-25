import type { PageContent } from '../../../contexts/PageContentContext'

/** Longest selection handed to the Agent; longer passages are better read with blog.note. */
export const NOTE_AGENT_SELECTION_MAX = 4000

/** What the editor tells the global Agent about the note it has open. */
export interface NoteAgentDoc {
  /** Cloud document id (blog.note's docId); null for a new note not saved to the cloud yet. */
  docId: number | null
  title: string
  topic: string | null
  status: string
}

/**
 * The open note as the global Agent's page context. The Agent never edits from this snapshot:
 * `noteDocId` points it at blog.note for the current Markdown and revision, and at
 * blog.note.update for the save, which the editor then receives like any collaborator's.
 */
export function noteAgentPageContent(doc: NoteAgentDoc): PageContent {
  return {
    type: 'custom',
    title: doc.title.trim() || undefined,
    summary: doc.docId == null
      ? 'A new blog note open in the editor that has not been saved to the cloud yet.'
      : 'The blog note open in the editor.',
    metadata: {
      surface: 'note_editor',
      noteDocId: doc.docId,
      topic: doc.topic,
      status: doc.status,
    },
  }
}

/** Text selected in the source textarea, bounded like any page selection. */
export function noteAgentSelection(value: string, start: number, end: number): string | null {
  if (end <= start) return null
  const text = value.slice(start, end).trim()
  return text ? text.slice(0, NOTE_AGENT_SELECTION_MAX) : null
}
