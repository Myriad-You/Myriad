/** One document owns its write queue, confirmed baseline and HTTP/WS receipts. */
import type { MutableRefObject } from 'react'
import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type { NoteCloudFields } from './noteCloudFields'
import type { NotePendingWrite } from './noteDraft'
import type { NoteRemoteSnapshot } from './noteSnapshotBuffer'
import { useCallback, useEffect, useRef } from 'react'
import { ApiError } from '../../../services/api'
import * as phantasiApi from '../../../services/phantasiApi'
import { userFacingError } from '../../../utils/userFacingError'
import { cloudFieldsOf, mergeCloudFields, sameCloudFields } from './noteCloudFields'
import { isOldPathTakenError, oldPathsFromField } from './noteOldPaths'
import { NoteRecoveryWriter } from './noteRecoveryStore'
import { isSlugTakenError, noteSlugWire } from './noteSlug'
import { mergeNoteSnapshot, NoteSnapshotBuffer } from './noteSnapshotBuffer'

export { cloudFieldsOf, mergeCloudFields, sameCloudFields } from './noteCloudFields'
export type { NoteCloudFields } from './noteCloudFields'
export const CLOUD_SAVE_DEBOUNCE_MS = 800

export interface NoteCloudSaveIo {
  updateNoteDoc: typeof phantasiApi.updateNoteDoc
  getNoteDoc: typeof phantasiApi.getNoteDoc
}
export interface UseNoteCloudSaveOptions {
  userId: number | null
  cloudId: number | null
  loading: boolean
  fields: NoteCloudFields
  revisionRef: MutableRefObject<number>
  onServerDoc: (doc: PhantasiNoteDoc) => void
  onMerged: (fields: NoteCloudFields) => void
  onSaved: (acked: NoteCloudFields) => void
  onError: (message: string | null) => void
  labels: { saveFailed: string; conflict: string; uncertain?: string; slugTaken?: string; oldPathTaken?: string }
  io?: NoteCloudSaveIo
}

export interface NoteWriteReceipt {
  requestId: string
  readonly confirmed: boolean
  receiveDoc: (doc: PhantasiNoteDoc) => Promise<boolean>
  reject: () => void
}
export interface NoteCloudSaveHandle {
  loadRecovery: NoteRecoveryWriter['load']
  discardRecovery: () => void
  ack: (fields: NoteCloudFields) => void
  baseRef: MutableRefObject<NoteCloudFields>
  restorePending: (pending: NotePendingWrite, baseline?: { fields: NoteCloudFields; revision: number }, remote?: PhantasiNoteDoc) => void
  receiveRemote: (fields: NoteCloudFields, revision: number, requestId?: string | null) => boolean
  receiveDoc: (doc: PhantasiNoteDoc, requestId?: string | null) => boolean
  compositionStart: () => void
  compositionEnd: () => void
  /** Track the exact fields submitted by each actual request, after queue admission. */
  runWrite: <T>(operation: (
    fields: NoteCloudFields,
    track: (submitted?: NoteCloudFields, expectedFields?: NoteCloudFields) => NoteWriteReceipt,
  ) => Promise<T>) => Promise<T>
}
interface PendingReceipt {
  requestId: string
  fields: NoteCloudFields
  confirmed: boolean
  expectedFields?: NoteCloudFields
}

export function useNoteCloudSave({
  userId, cloudId, loading, fields, revisionRef,
  onServerDoc, onMerged, onSaved, onError, labels, io = phantasiApi,
}: UseNoteCloudSaveOptions): NoteCloudSaveHandle {
  const recoveryOwner = useRef(userId)
  const recoveryWriter = useRef(new NoteRecoveryWriter())
  const recoveryDiscarded = useRef(false)
  const loadRecovery = useCallback<NoteRecoveryWriter['load']>((scope) => { recoveryDiscarded.current = false; return recoveryWriter.current.load(scope) }, [])
  const discardRecovery = useCallback(() => { recoveryDiscarded.current = true; recoveryWriter.current.discard() }, [])
  const latestRef = useRef(fields)
  latestRef.current = fields
  const baseRef = useRef(fields)
  const writeTailRef = useRef<Promise<unknown>>(Promise.resolve())
  const composingRef = useRef(false)
  const compositionEpochRef = useRef(0)
  const compositionQueueRef = useRef(new NoteSnapshotBuffer())
  const uncertainRef = useRef(false)
  const resumeRef = useRef<() => void>(() => {})
  const compositionWaiters = useRef<Array<() => void>>([])
  const wakeWaiters = useCallback(() => {
    for (const resolve of compositionWaiters.current.splice(0)) resolve()
  }, [])
  const pendingRef = useRef<PendingReceipt | null>(null)
  const receiptsRef = useRef(new Map<string, PendingReceipt>())
  const deferredRef = useRef<NoteRemoteSnapshot | null>(null)
  const recoveryFlushRef = useRef<() => void>(() => {})
  const aliveRef = useRef(true)
  const activeIdRef = useRef(cloudId)
  activeIdRef.current = cloudId
  const activeUserRef = useRef(userId)
  activeUserRef.current = userId
  const callbacks = useRef({ onServerDoc, onMerged, onSaved, onError, labels, io })
  callbacks.current = { onServerDoc, onMerged, onSaved, onError, labels, io }

  useEffect(() => {
    aliveRef.current = true
    return () => { recoveryFlushRef.current(); aliveRef.current = false; wakeWaiters() }
  }, [wakeWaiters])

  const ack = useCallback((next: NoteCloudFields) => {
    baseRef.current = next
  }, [])

  const persistRecovery = useCallback(() => {
    const id = activeIdRef.current
    const user = activeUserRef.current
    if (recoveryDiscarded.current || !aliveRef.current || id == null || user == null || user !== recoveryOwner.current) return
    recoveryFlushRef.current = () => {}
    const pending = pendingRef.current
    recoveryWriter.current.write({ userId: user, docId: id }, latestRef.current, baseRef.current, revisionRef.current,
      pending && !pending.confirmed ? { requestId: pending.requestId, fields: pending.fields, ...(pending.expectedFields ? { expectedFields: pending.expectedFields } : {}) } : undefined)
  }, [revisionRef])

  const receiveSnapshot = useCallback((snapshot: NoteRemoteSnapshot): boolean => {
    const { fields: remote, revision } = snapshot
    let { requestId } = snapshot
    if (!aliveRef.current || revision <= revisionRef.current) return false
    if (composingRef.current) {
      compositionQueueRef.current.push(snapshot, pendingRef.current?.requestId)
      return false
    }
    const pending = pendingRef.current
    if (uncertainRef.current && pending && sameCloudFields(remote, pending.expectedFields ?? pending.fields)) requestId = pending.requestId
    const receipt = requestId ? receiptsRef.current.get(requestId) : undefined
    // Arrival order does not establish whether another snapshot includes our write.
    if (pending && !pending.confirmed && requestId !== pending.requestId) {
      if (!deferredRef.current || revision >= deferredRef.current.revision) {
        deferredRef.current = mergeNoteSnapshot(deferredRef.current, snapshot)
      }
      if (uncertainRef.current) callbacks.current.onError(callbacks.current.labels.uncertain ?? callbacks.current.labels.conflict)
      return false
    }
    const resuming = uncertainRef.current && receipt != null
    if (receipt) {
      receipt.confirmed = true
      uncertainRef.current = false
    }
    const ancestor = receipt?.fields ?? baseRef.current
    const result = mergeCloudFields(ancestor, latestRef.current, remote)
    revisionRef.current = revision
    ack(remote)
    latestRef.current = result
    persistRecovery()
    recoveryWriter.current.confirmRecovered(remote)
    callbacks.current.onMerged(result)
    callbacks.current.onSaved(remote)
    if (snapshot.doc) callbacks.current.onServerDoc(snapshot.doc)
    if (requestId) receiptsRef.current.delete(requestId)
    const deferred = deferredRef.current
    deferredRef.current = null
    if (deferred) receiveSnapshot(deferred)
    if (resuming) queueMicrotask(() => resumeRef.current())
    return true
  }, [ack, persistRecovery, revisionRef])

  const receiveRemote = useCallback((fields: NoteCloudFields, revision: number, requestId?: string | null) =>
    receiveSnapshot({ fields, revision, requestId }), [receiveSnapshot])

  const receiveDoc = useCallback((doc: PhantasiNoteDoc, requestId?: string | null): boolean => {
    if (!aliveRef.current) return false
    const previousRevision = revisionRef.current
    receiveSnapshot({ fields: cloudFieldsOf(doc), revision: doc.revision, requestId, doc })
    // WS contains fields; HTTP at the same revision still supplies publication metadata.
    if (doc.revision !== revisionRef.current) return false
    if (doc.revision === previousRevision) callbacks.current.onServerDoc(doc)
    return true
  }, [receiveSnapshot, revisionRef])

  const restorePending = useCallback((pending: NotePendingWrite, baseline?: { fields: NoteCloudFields; revision: number }, remote?: PhantasiNoteDoc) => {
    const receipt = { ...pending, confirmed: false }
    pendingRef.current = receipt
    receiptsRef.current.set(receipt.requestId, receipt)
    uncertainRef.current = true
    if (baseline) {
      ack(baseline.fields)
      revisionRef.current = baseline.revision
    }
    if (remote) receiveDoc(remote)
    callbacks.current.onError(callbacks.current.labels.uncertain ?? callbacks.current.labels.conflict)
  }, [ack, receiveDoc, revisionRef])

  const finishPending = useCallback(() => {
    if (uncertainRef.current) return
    const pending = pendingRef.current
    pendingRef.current = null
    if (pending && !composingRef.current) receiptsRef.current.delete(pending.requestId)
    const deferred = deferredRef.current
    deferredRef.current = null
    if (deferred) receiveSnapshot(deferred)
    persistRecovery()
  }, [persistRecovery, receiveSnapshot])

  const settleFailure = useCallback((error: unknown) => {
    const pending = pendingRef.current
    const rejected = error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408
    if (pending && !pending.confirmed && !rejected) {
      uncertainRef.current = true
      return
    }
    finishPending()
  }, [finishPending])

  const runWrite = useCallback(<T>(operation: (
    fields: NoteCloudFields,
    track: (submitted?: NoteCloudFields, expectedFields?: NoteCloudFields) => NoteWriteReceipt,
  ) => Promise<T>, resumeAutosave = true): Promise<T> => {
    const ownerId = activeIdRef.current
    const ownerUser = activeUserRef.current
    const current = () => aliveRef.current && ownerId === activeIdRef.current && ownerUser === activeUserRef.current
    const pending = writeTailRef.current.then(async () => {
      while (composingRef.current && current()) {
        await new Promise<void>((resolve) => compositionWaiters.current.push(resolve))
      }
      if (!current()) throw new DOMException('Note editor closed', 'AbortError')
      if (uncertainRef.current) throw new Error(callbacks.current.labels.uncertain ?? callbacks.current.labels.conflict)
      const track = (submitted?: NoteCloudFields, expectedFields?: NoteCloudFields): NoteWriteReceipt => {
        if (!current()) throw new DOMException('Note editor closed', 'AbortError')
        finishPending()
        const receipt: PendingReceipt = { requestId: crypto.randomUUID(), fields: submitted ?? baseRef.current, expectedFields, confirmed: false }
        pendingRef.current = receipt
        receiptsRef.current.set(receipt.requestId, receipt)
        persistRecovery()
        return {
          requestId: receipt.requestId,
          get confirmed() { return receipt.confirmed },
          receiveDoc: async (doc) => {
            if (!current()) return false
            receiveDoc(doc, receipt.requestId)
            // An explicit command may close the editor. Settle its acknowledgement
            // and recovery baseline before permitting that while IME is active.
            while (composingRef.current && current()) {
              await new Promise<void>((resolve) => compositionWaiters.current.push(resolve))
            }
            return current() && receiveDoc(doc, receipt.requestId)
          },
          reject: () => { if (current()) finishPending() },
        }
      }
      try { return await operation(latestRef.current, track) }
      catch (error) {
        if (!current()) throw error
        settleFailure(error)
        if (uncertainRef.current) throw new Error(callbacks.current.labels.uncertain ?? callbacks.current.labels.conflict, { cause: error })
        throw error
      }
      finally {
        if (current()) finishPending()
        if (resumeAutosave && current() && !uncertainRef.current && !sameCloudFields(latestRef.current, baseRef.current)) {
          queueMicrotask(() => resumeRef.current())
        }
      }
    })
    // Errors finish their receipt and release the queue for subsequent commands.
    writeTailRef.current = pending.catch(() => {})
    return pending
  }, [finishPending, persistRecovery, receiveDoc, settleFailure])

  const flush = useCallback(async (id: number): Promise<void> => {
    if (composingRef.current || uncertainRef.current) return
    const succeeded = await runWrite(async (_fields, track) => {
      if (composingRef.current || activeIdRef.current !== id || sameCloudFields(latestRef.current, baseRef.current)) return false
      const { io: api, labels: text } = callbacks.current
      const current = () => aliveRef.current && activeIdRef.current === id
      try {
        for (let attempt = 0; attempt < 2; attempt += 1) {
          const snapshot = latestRef.current
          const receipt = track(snapshot)
          try {
            const slug = noteSlugWire(snapshot.slug)
            // 旧地址只有已发布的才有这一项；没改就不带，省得每次自动保存都去查重。
            const oldPathsChanged = typeof snapshot.oldPaths === 'string' &&
              snapshot.oldPaths !== (baseRef.current.oldPaths ?? null)
            const doc = await api.updateNoteDoc(id, {
              title: snapshot.title, content_md: snapshot.contentMd, topic: snapshot.topic,
              image: snapshot.cover, published_at: snapshot.publishedAt,
              // 未发布不能带 slug；空值省略，避免把「不改」写成清空。
              ...(typeof slug === 'string' ? { slug } : {}),
              ...(oldPathsChanged ? { old_paths: oldPathsFromField(snapshot.oldPaths) } : {}),
              revision: revisionRef.current, client_request_id: receipt.requestId,
            })
            if (!current()) return false
            await receipt.receiveDoc(doc)
            if (!doc.last_error) callbacks.current.onError(null)
            return receipt.confirmed
          } catch (err) {
            if (!current()) return false
            if (isSlugTakenError(err)) {
              callbacks.current.onError(text.slugTaken ?? text.saveFailed)
              return false
            }
            if (isOldPathTakenError(err)) {
              callbacks.current.onError(text.oldPathTaken ?? text.saveFailed)
              return false
            }
            const conflict = err instanceof ApiError && err.status === 409 || userFacingError(err, text.saveFailed) === text.conflict
            if (!conflict || attempt === 1) throw err
            receipt.reject()
            const fresh = await api.getNoteDoc(id)
            if (!current()) return false
            receiveDoc(fresh)
          }
        }
      } catch (err) {
        if (current()) {
          settleFailure(err)
          callbacks.current.onError(uncertainRef.current ? text.uncertain ?? text.conflict : userFacingError(err, text.saveFailed))
        }
      }
      return false
    }, false)
    // Queue follow-up edits behind any already admitted explicit command.
    if (succeeded && aliveRef.current && !composingRef.current && activeIdRef.current === id &&
      !sameCloudFields(latestRef.current, baseRef.current)) {
      void flush(id).catch(() => {})
    }
  }, [receiveDoc, revisionRef, runWrite, settleFailure])

  resumeRef.current = () => {
    if (aliveRef.current && activeIdRef.current != null) void flush(activeIdRef.current).catch(() => {})
  }

  const compositionStart = useCallback(() => {
    compositionEpochRef.current += 1
    composingRef.current = true
  }, [])
  const compositionEnd = useCallback(() => {
    const epoch = compositionEpochRef.current
    setTimeout(() => {
      if (!aliveRef.current || epoch !== compositionEpochRef.current) return
      composingRef.current = false
      for (const snapshot of compositionQueueRef.current.drain()) receiveSnapshot(snapshot)
      receiptsRef.current.clear()
      if (pendingRef.current) receiptsRef.current.set(pendingRef.current.requestId, pendingRef.current)
      wakeWaiters()
      if (activeIdRef.current != null) void flush(activeIdRef.current).catch(() => {})
    }, 0)
  }, [flush, receiveSnapshot, wakeWaiters])

  useEffect(() => () => recoveryFlushRef.current(), [cloudId, userId])

  useEffect(() => {
    if (loading || cloudId == null || userId == null || userId !== recoveryOwner.current) return
    const snapshot = fields
    const base = baseRef.current
    const revision = revisionRef.current
    const pending = pendingRef.current
    const flush = () => {
      if (recoveryDiscarded.current || recoveryFlushRef.current !== flush) return
      recoveryFlushRef.current = () => {}
      recoveryWriter.current.write({ userId, docId: cloudId }, snapshot, base, revision,
        pending && !pending.confirmed ? { requestId: pending.requestId, fields: pending.fields, ...(pending.expectedFields ? { expectedFields: pending.expectedFields } : {}) } : undefined)
    }
    recoveryFlushRef.current = flush
    let idle: number | undefined
    const timer = setTimeout(() => {
      // Composition can remain open for seconds. Its durable emergency copy
      // still flushes on pagehide; ordinary writes wait for committed text.
      if (composingRef.current) return
      if (typeof window.requestIdleCallback === 'function')
        idle = window.requestIdleCallback(() => { if (!composingRef.current) flush() }, { timeout: 1000 })
      else flush()
    }, 300)
    return () => {
      clearTimeout(timer)
      if (idle !== undefined) window.cancelIdleCallback?.(idle)
    }
  }, [loading, cloudId, userId, fields.title, fields.contentMd, fields.topic, fields.cover, fields.publishedAt, fields.slug, fields.oldPaths, persistRecovery])

  useEffect(() => {
    const flush = () => recoveryFlushRef.current()
    const visibility = () => { if (document.visibilityState === 'hidden') flush() }
    window.addEventListener('pagehide', flush)
    document.addEventListener('visibilitychange', visibility)
    return () => {
      window.removeEventListener('pagehide', flush)
      document.removeEventListener('visibilitychange', visibility)
    }
  }, [])

  useEffect(() => {
    if (loading || cloudId == null || sameCloudFields(fields, baseRef.current)) return
    const timer = setTimeout(() => { void flush(cloudId).catch(() => {}) }, CLOUD_SAVE_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [cloudId, loading, fields.title, fields.contentMd, fields.topic, fields.cover, fields.publishedAt, fields.slug, fields.oldPaths, flush])

  return { loadRecovery, discardRecovery, ack, baseRef, restorePending, runWrite, receiveRemote, receiveDoc, compositionStart, compositionEnd }
}
