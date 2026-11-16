import type { NoteCloudFields } from './noteCloudFields'

export interface NoteRecoveryScope {
  userId: number
  docId: number
}

export function noteRecoveryKey({ userId, docId }: NoteRecoveryScope): `user:${number}:doc:${number}` {
  return `user:${userId}:doc:${docId}`
}

export interface NoteRecovery {
  version: 2
  fields: NoteCloudFields
  base: NoteCloudFields
  revision: number
  savedAt: number
  pending?: NotePendingWrite
}

export interface NotePendingWrite { requestId: string; fields: NoteCloudFields; expectedFields?: NoteCloudFields }

export function isCloudFields(value: unknown): value is NoteCloudFields {
  if (!value || typeof value !== 'object') return false
  const fields = value as Partial<NoteCloudFields>
  return typeof fields.title === 'string' && typeof fields.contentMd === 'string' &&
    (fields.topic === null || typeof fields.topic === 'string') &&
    (fields.cover === null || typeof fields.cover === 'string') &&
    (fields.publishedAt === null || (typeof fields.publishedAt === 'number' && Number.isFinite(fields.publishedAt))) &&
    (fields.slug === undefined || fields.slug === null || typeof fields.slug === 'string') &&
    (fields.oldPaths === undefined || fields.oldPaths === null || typeof fields.oldPaths === 'string')
}

export const NOTE_DRAFT_TTL_MS = 7 * 24 * 60 * 60 * 1000

export function decodeNoteRecovery(raw: string, now = Date.now()): NoteRecovery | null {
  try {
    const entry = JSON.parse(raw) as Partial<NoteRecovery> | null
    if (!entry || entry.version !== 2 || !isCloudFields(entry.fields) || !isCloudFields(entry.base)
      || !Number.isInteger(entry.revision) || entry.revision! < 1
      || typeof entry.savedAt !== 'number' || !Number.isFinite(entry.savedAt)
      || (entry.pending !== undefined && (typeof entry.pending?.requestId !== 'string' || !isCloudFields(entry.pending.fields) || (entry.pending.expectedFields !== undefined && !isCloudFields(entry.pending.expectedFields))))
      || now - entry.savedAt > NOTE_DRAFT_TTL_MS) {
      return null
    }
    return entry as NoteRecovery
  } catch { return null }
}
