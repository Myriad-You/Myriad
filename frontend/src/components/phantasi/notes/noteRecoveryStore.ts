import type { NoteCloudFields } from './noteCloudFields'
import type { NotePendingWrite, NoteRecovery, NoteRecoveryScope } from './noteRecoveryRecord'
import { safeRandomUUID } from '../../../utils/uuid'
import { sameCloudFields } from './noteCloudFields'
import { decodeNoteRecovery, noteRecoveryKey } from './noteRecoveryRecord'

export interface NoteRecoveryCopy {
  key: string
  raw: string
  recovery: NoteRecovery
}

function storageKey(scope: NoteRecoveryScope): string {
  return `phantasi:note-draft:${noteRecoveryKey(scope)}`
}

function dismissalKey(copy: NoteRecoveryCopy): string {
  // Immutable keys identify one snapshot; a legacy mutable key also needs its contents.
  return copy.key.includes(':copy:') ? copy.key : JSON.stringify([copy.key, copy.raw])
}

/**
 * Each immutable key names one snapshot. Removing it can never remove a
 * newer snapshot written concurrently by another editor.
 */
export function listNoteRecoveryCopies(scope: NoteRecoveryScope, now = Date.now()): NoteRecoveryCopy[] {
  try {
    const storage = globalThis.localStorage
    if (!storage) return []
    const canonical = storageKey(scope)
    const keys = new Set([canonical])
    for (let index = 0; index < storage.length; index += 1) {
      const key = storage.key(index)
      if (key?.startsWith(`${canonical}:copy:`)) keys.add(key)
    }
    const copies: NoteRecoveryCopy[] = []
    for (const key of keys) {
      const raw = storage.getItem(key)
      if (!raw || (key === canonical && storage.getItem(`${canonical}:consumed`) === raw)) continue
      const recovery = decodeNoteRecovery(raw, now)
      if (recovery) copies.push({ key, raw, recovery })
    }
    return copies.sort((a, b) => b.recovery.savedAt - a.recovery.savedAt || a.key.localeCompare(b.key))
  } catch { return [] }
}

function samePending(a?: NotePendingWrite, b?: NotePendingWrite): boolean {
  if (!a || !b) return a === b
  return a.requestId === b.requestId && sameCloudFields(a.fields, b.fields)
    && sameCloudFields(a.expectedFields ?? a.fields, b.expectedFields ?? b.fields)
}

let lastTimestamp = 0

export class NoteRecoveryWriter {
  private current: NoteRecoveryCopy | null = null
  private ownsCurrent = false
  private recovered: NoteRecoveryCopy | null = null
  private scope: NoteRecoveryScope | null = null

  load(scope: NoteRecoveryScope): NoteRecovery | null {
    this.scope = scope
    const dismissed = this.dismissedKeys()
    this.recovered = listNoteRecoveryCopies(scope).find(copy => !dismissed.has(dismissalKey(copy))) ?? null
    this.current = this.recovered
    this.ownsCurrent = false
    return this.recovered?.recovery ?? null
  }

  write(scope: NoteRecoveryScope, fields: NoteCloudFields, base: NoteCloudFields, revision: number, pending?: NotePendingWrite): boolean {
    if (this.scope && noteRecoveryKey(this.scope) !== noteRecoveryKey(scope)) {
      // Leave the previous document's durable copy intact when reusing a writer.
      this.current = null
      this.ownsCurrent = false
      this.recovered = null
    }
    this.scope = scope
    if (!pending && sameCloudFields(fields, base)) {
      this.clear()
      const prior = this.recovered?.recovery
      if (prior && (sameCloudFields(prior.fields, base) || (!prior.pending && sameCloudFields(prior.fields, prior.base)))) this.confirmRecovered(prior.fields)
      return true
    }
    const prior = this.current?.recovery
    if (prior && prior.revision === revision && sameCloudFields(prior.fields, fields)
      && sameCloudFields(prior.base, base) && samePending(prior.pending, pending)) {
      return true
    }
    try {
      const storage = globalThis.localStorage
      if (!storage) return false
      const savedAt = lastTimestamp = Math.max(Date.now(), lastTimestamp + 1)
      const recovery: NoteRecovery = { version: 2, fields, base, revision, savedAt, ...(pending ? { pending } : {}) }
      const raw = JSON.stringify(recovery)
      const key = `${storageKey(scope)}:copy:${safeRandomUUID()}`
      storage.setItem(key, raw)
      const previous = this.ownsCurrent ? this.current : null
      this.current = { key, raw, recovery }
      this.ownsCurrent = true
      if (previous) NoteRecoveryWriter.consume(previous)
      return true
    } catch { return false }
  }

  /** Only an exact cloud snapshot proves this particular recovery was saved. */
  confirmRecovered(fields: NoteCloudFields): void {
    if (this.recovered && sameCloudFields(this.recovered.recovery.fields, fields)) {
      NoteRecoveryWriter.consume(this.recovered)
      this.recovered = null
    }
  }

  private dismissedKeys(): Set<string> {
    try {
      if (!this.scope) return new Set()
      const parsed: unknown = JSON.parse(globalThis.sessionStorage?.getItem(`${storageKey(this.scope)}:dismissed`) ?? '[]')
      return new Set(Array.isArray(parsed) ? parsed.filter((key): key is string => typeof key === 'string') : [])
    } catch { return new Set() }
  }

  /**
   * Discarding a recovered foreign copy hides that exact snapshot in this tab;
   * the originating window retains its independent recovery.
   */
  discard(): void {
    if (this.recovered && this.scope) {
      try {
        const available = new Set(listNoteRecoveryCopies(this.scope).map(dismissalKey))
        const dismissed = [...this.dismissedKeys(), dismissalKey(this.recovered)].filter(key => available.has(key))
        globalThis.sessionStorage?.setItem(`${storageKey(this.scope)}:dismissed`, JSON.stringify([...new Set(dismissed)]))
      } catch { /* Failure to remember dismissal preserves the recoverable copy. */ }
    }
    this.clear()
    this.recovered = null
  }

  /** Discard only this editor's fork; never discard another editor's copy. */
  clear(): void {
    if (this.ownsCurrent && this.current) NoteRecoveryWriter.consume(this.current)
    this.current = null
    this.ownsCurrent = false
  }

  static consume(copy: NoteRecoveryCopy): void {
    try {
      if (copy.key.includes(':copy:')) globalThis.localStorage?.removeItem(copy.key)
      // Old canonical writers used a mutable key. Record exactly what was
      // consumed rather than deleting a possible concurrent legacy update.
      else globalThis.localStorage?.setItem(`${copy.key}:consumed`, copy.raw)
    } catch { /* A failed cleanup retains a copy; it never deletes another one. */ }
  }
}
