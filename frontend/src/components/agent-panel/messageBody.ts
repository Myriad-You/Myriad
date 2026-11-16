import { authSubject } from '../../utils/authSubject'
import { safeRandomUUID } from '../../utils/uuid'

export const BODY_PAGE_CHARS = 8192
export const BODY_INLINE_CHARS = 4096
/** Own the bounded text instead of retaining a slice of a large input backing store. */
export const detachedPrefix = (text: string, chars: number) => text.slice(0, chars).split('').join('')
const MAX_STORED_BYTES = 64 * 1024 * 1024
const MAX_BODY_AGE_MS = 24 * 60 * 60 * 1000
const tabId = safeRandomUUID()
const owner = () => `${tabId}:${authSubject.revision}`
export interface MessageBodyRef { id: string; owner: string; chars: number }
export interface BodyStorage {
  append: (id: string, text: string, offset: number, signal: AbortSignal) => Promise<void>
}
interface BodyMeta { id: string; owner: string; chars: number; updatedAt: number }
let connection: Promise<IDBDatabase> | undefined

function requestValue<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}
function transactionDone(transaction: IDBTransaction, signal?: AbortSignal): Promise<void> {
  const abort = () => { try { transaction.abort() } catch { /* already complete */ } }
  signal?.addEventListener('abort', abort, { once: true })
  const result = new Promise<void>((resolve, reject) => {
    const finish = () => signal?.removeEventListener('abort', abort)
    transaction.oncomplete = () => { finish(); resolve() }
    transaction.onabort = transaction.onerror = () => { finish(); reject(signal?.aborted ? signal.reason : transaction.error ?? new Error('Body storage unavailable')) }
    if (signal?.aborted) abort()
  })
  void result.catch(() => {})
  return result
}
function pageRange(id: string) { return IDBKeyRange.bound([id, 0], [id, Number.MAX_SAFE_INTEGER]) }
async function database(): Promise<IDBDatabase> {
  if (!connection) {
    connection = new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open('myriad-agent-bodies-v1', 1)
      request.onupgradeneeded = () => {
        request.result.createObjectStore('pages')
        request.result.createObjectStore('bodies', { keyPath: 'id' })
        request.result.createObjectStore('stats')
      }
      request.onsuccess = () => {
        const db = request.result
        db.onversionchange = () => { db.close(); connection = undefined }
        resolve(db)
      }
      request.onerror = () => { connection = undefined; reject(request.error) }
    }).then(async db => { await pruneBodies(db); return db }).catch(error => { connection = undefined; throw error })
  }
  return connection
}
/** Bounded disk cache; expired drafts cannot accumulate across abandoned tabs. */
async function pruneBodies(db: IDBDatabase, discardOwner?: string): Promise<void> {
  const tx = db.transaction(['pages', 'bodies', 'stats'], 'readwrite')
  const done = transactionDone(tx)
  const stats = tx.objectStore('stats')
  const usage = await requestValue<number | undefined>(stats.get('bytes')) ?? 0
  let removed = 0
  const cursor = tx.objectStore('bodies').openCursor()
  cursor.onsuccess = () => {
    const item = cursor.result
    if (!item) { stats.put(Math.max(0, usage - removed), 'bytes'); return }
    const meta = item.value as BodyMeta
    if (meta.owner === discardOwner || Date.now() - meta.updatedAt > MAX_BODY_AGE_MS) {
      removed += meta.chars * 2
      tx.objectStore('pages').delete(pageRange(meta.id))
      item.delete()
    }
    item.continue()
  }
  await done
}
let previousOwner = owner()
authSubject.subscribe(() => {
  const old = previousOwner
  previousOwner = owner()
  if (connection) void connection.then(db => pruneBodies(db, old)).catch(() => {})
})
if (typeof window !== 'undefined') { window.addEventListener('pagehide', event => {
  if (event.persisted) return
  if (connection) void connection.then(db => pruneBodies(db, previousOwner)).catch(() => {})
})
}

export const bodyStorage: BodyStorage = {
  async append(id, text, offset, signal) {
    signal = AbortSignal.any([signal, authSubject.signal])
    signal.throwIfAborted()
    const scope = owner()
    const db = await database()
    signal.throwIfAborted()
    // One transaction/page bounds both browser structured-clone work and writes
    // queued in IndexedDB; the producer awaits every transaction.
    for (let at = 0; at < text.length;) {
      signal.throwIfAborted()
      const position = offset + at
      const page = Math.floor(position / BODY_PAGE_CHARS)
      const fragment = text.slice(at, at + BODY_PAGE_CHARS - position % BODY_PAGE_CHARS)
      const tx = db.transaction(['pages', 'bodies', 'stats'], 'readwrite')
      const done = transactionDone(tx, signal)
      try {
        const stats = tx.objectStore('stats')
        const used = await requestValue<number | undefined>(stats.get('bytes')) ?? 0
        const prior = await requestValue<BodyMeta | undefined>(tx.objectStore('bodies').get(id))
        if ((prior?.chars ?? 0) !== position || (prior && prior.owner !== scope)) throw new Error('Body write superseded')
        if (used + fragment.length * 2 > MAX_STORED_BYTES) throw new Error('Body storage limit reached')
        const pages = tx.objectStore('pages')
        const tail = position % BODY_PAGE_CHARS ? await requestValue<string>(pages.get([id, page])) : ''
        pages.put(tail + fragment, [id, page])
        tx.objectStore('bodies').put({ id, owner: scope, chars: position + fragment.length, updatedAt: Date.now() } satisfies BodyMeta)
        stats.put(used + fragment.length * 2, 'bytes')
        await done
      } catch (error) {
        try { tx.abort() } catch { /* already complete */ }
        await done.catch(() => {})
        throw error
      }
      at += fragment.length
    }
  },
}

export class BodyWriter {
  private preview = ''
  private chars = 0
  private archived = false
  private busy = false
  private watched = false
  private readonly discard = () => {
    this.finish()
    if (!this.archived && this.storage === bodyStorage) void releaseMessageBody({ id: this.id, owner: this.scope, chars: this.chars }).catch(() => {})
  }

  async discardStored() { if (this.storage === bodyStorage) await releaseMessageBody({ id: this.id, owner: this.scope, chars: this.chars }).catch(() => {}) }
  finish() { this.signal.removeEventListener('abort', this.discard); this.watched = false }
  private readonly scope = owner()
  private readonly signal: AbortSignal
  constructor(private readonly id: string, signal: AbortSignal, private readonly storage: BodyStorage = bodyStorage) {
    this.signal = AbortSignal.any([signal, authSubject.signal])
  }

  get retainedChars() { return this.preview.length }
  snapshot(): { content: string; body?: MessageBodyRef } {
    return { content: this.preview, ...(this.archived ? { body: { id: this.id, owner: this.scope, chars: this.chars } } : {}) }
  }

  async append(text: string): Promise<ReturnType<BodyWriter['snapshot']>> {
    this.signal.throwIfAborted()
    if (this.busy) throw new Error('Body writer busy: producer must await append')
    this.busy = true
    try {
      if (!this.archived && this.chars + text.length <= BODY_INLINE_CHARS) {
        this.preview += text
      } else {
        if (!this.watched) { this.signal.addEventListener('abort', this.discard, { once: true }); this.watched = true }
        if (!this.archived && this.chars) await this.storage.append(this.id, this.preview, 0, this.signal)
        await this.storage.append(this.id, text, this.chars, this.signal)
        this.signal.throwIfAborted()
        this.archived = true
        if (this.preview.length < BODY_INLINE_CHARS) this.preview += detachedPrefix(text, BODY_INLINE_CHARS - this.preview.length)
      }
      this.chars += text.length
      return this.snapshot()
    } catch (error) {
      if (!this.archived) await this.discardStored()
      this.finish()
      throw error
    } finally { this.busy = false }
  }
}

export async function prepareMessageBody(content: string, signal = authSubject.signal): Promise<{ content: string; body?: MessageBodyRef }> {
  signal.throwIfAborted()
  if (content.length <= BODY_INLINE_CHARS) return { content, body: undefined }
  const writer = new BodyWriter(safeRandomUUID(), signal)
  try { return await writer.append(content) } catch (error) {
    await writer.discardStored()
    throw error
  } finally { writer.finish() }
}
export async function readBodyPage(body: MessageBodyRef, page: number, signal = authSubject.signal): Promise<string> {
  signal = AbortSignal.any([signal, authSubject.signal])
  signal.throwIfAborted()
  if (body.owner !== owner()) throw new Error('Body identity changed')
  const db = await database()
  signal.throwIfAborted()
  const tx = db.transaction('pages', 'readonly')
  const done = transactionDone(tx, signal)
  const text = await requestValue<string | undefined>(tx.objectStore('pages').get([body.id, page]))
  await done
  signal.throwIfAborted()
  if (text === undefined) throw new Error('Body expired; reopen persisted session history')
  return text.slice(0, Math.max(0, Math.min(BODY_PAGE_CHARS, body.chars - page * BODY_PAGE_CHARS)))
}
/** Only explicit copy/save actions call this; rendering always reads one page. */
export async function readCompleteBody(content: string, body?: MessageBodyRef): Promise<string> {
  if (!body) return content
  const signal = authSubject.signal
  const parts: string[] = []
  for (let page = 0; page < Math.ceil(body.chars / BODY_PAGE_CHARS); page++) parts.push(await readBodyPage(body, page, signal))
  return parts.join('')
}

/** Release superseded/evicted views; persisted session history is their cold source. */
export async function releaseMessageBody(body: MessageBodyRef): Promise<void> {
  const db = await database()
  const tx = db.transaction(['pages', 'bodies', 'stats'], 'readwrite')
  const done = transactionDone(tx)
  const meta = await requestValue<BodyMeta | undefined>(tx.objectStore('bodies').get(body.id))
  if (meta?.owner === body.owner) {
    const stats = tx.objectStore('stats')
    const used = await requestValue<number | undefined>(stats.get('bytes')) ?? 0
    tx.objectStore('pages').delete(pageRange(body.id))
    tx.objectStore('bodies').delete(body.id)
    stats.put(Math.max(0, used - meta.chars * 2), 'bytes')
  }
  await done
}
