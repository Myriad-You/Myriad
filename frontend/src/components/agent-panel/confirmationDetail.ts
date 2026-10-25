/**
 * The structured side of a confirmation question (backend `UserQuestion.detail`): which
 * capability it is, and for a blog note save, the changes it makes. The card renders these
 * in the reader's language; the question's plain text is only the fallback.
 */

export type BlogNoteChangeField = 'title' | 'topic' | 'body'

export interface BlogNoteChange {
  field: BlogNoteChangeField
  before: string
  after: string
}

export interface ConfirmationDetail {
  capabilityId?: string
  blogNote?: {
    title: string
    changes: BlogNoteChange[]
    /** All changes, of which `changes` is the first few. */
    total: number
  }
}

const FIELDS: readonly BlogNoteChangeField[] = ['title', 'topic', 'body']

function text(value: unknown): string {
  if (typeof value === 'string') return value
  return value == null ? '' : String(value)
}

/** Accepts only the shape the backend sends; anything else is no detail at all. */
export function readConfirmationDetail(raw: unknown): ConfirmationDetail | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const data = raw as Record<string, unknown>
  const detail: ConfirmationDetail = {}
  if (typeof data.capabilityId === 'string' && data.capabilityId) {
    detail.capabilityId = data.capabilityId
  }
  const note = data.blogNote
  if (note && typeof note === 'object') {
    const value = note as Record<string, unknown>
    const changes = Array.isArray(value.changes)
      ? value.changes.flatMap((item): BlogNoteChange[] => {
          if (!item || typeof item !== 'object') return []
          const change = item as Record<string, unknown>
          const field = FIELDS.find((name) => name === change.field)
          return field ? [{ field, before: text(change.before), after: text(change.after) }] : []
        })
      : []
    const total = typeof value.total === 'number' && value.total >= changes.length
      ? value.total
      : changes.length
    detail.blogNote = { title: text(value.title), changes, total }
  }
  return detail.capabilityId || detail.blogNote ? detail : undefined
}

/**
 * Work's own bookkeeping (loading tools, paging a long result, planning). When one of these
 * fails the Agent recovers by itself; it is not a failure of what the user asked for.
 */
const HOUSEKEEPING = ['discover_tools', 'read_result', 'update_plan', 'load_skill', 'list_recipes', 'ask_user']

export function isHousekeepingStep(stepId: string): boolean {
  return HOUSEKEEPING.some((name) => stepId === name || stepId.startsWith(`${name}_`))
}
