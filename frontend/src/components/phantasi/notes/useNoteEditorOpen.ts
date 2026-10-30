import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type { NoteCloudFields, NoteCloudSaveHandle } from './useNoteCloudSave'
import { useEffect } from 'react'
import { userFacingError } from '../../../utils/userFacingError'
import { showNoteNotice } from '../phantasiNotice'
import { sameCloudFields } from './noteCloudFields'
import { isNoteDocGone } from './noteCollab'
import {
  readNoteRecovery,
  recoverNoteFields,
} from './noteDraft'
import { normalizeNoteCover, normalizeNoteTopic } from './noteFields'
import { expandJammedDefinitions } from './noteVisual'
import { openNoteCloudDoc } from './useNoteCloud'

export function useNoteEditorOpen({
  userId,
  noteId,
  docId,
  cloudAck,
  loadRecovery,
  restorePending,
  applyServerDoc,
  applyMergedFields,
  setCloudId,
  setSaved,
  setLoading,
  onGone,
  loadFailed,
  scheduleFailed,
}: {
  userId: number | null
  noteId?: number
  docId?: number
  cloudAck: (server: NoteCloudFields) => void
  loadRecovery?: NoteCloudSaveHandle['loadRecovery']
  restorePending?: NoteCloudSaveHandle['restorePending']
  applyServerDoc: (doc: PhantasiNoteDoc) => void
  applyMergedFields: (next: NoteCloudFields) => void
  setCloudId: (id: number) => void
  setSaved: (next: NoteCloudFields) => void
  setLoading: (loading: boolean) => void
  /**
   * 要打开的那篇已经不在了（浏览器里记着的是删掉的 id）。交给外面退出这篇；
   * 不报「加载失败」，也不能留着这个 id 去连协作、自动保存。
   */
  onGone?: () => void
  loadFailed: string
  scheduleFailed: string
}): void {
  useEffect(() => {
    if (userId == null) return
    const controller = new AbortController()
    const run = async () => {
      try {
        const doc = await openNoteCloudDoc({
          noteId,
          docId,
          signal: controller.signal,
        })
        if (controller.signal.aborted) return
        const server: NoteCloudFields = {
          title: doc.title,
          contentMd: doc.content_md,
          topic: normalizeNoteTopic(doc.topic),
          cover: normalizeNoteCover(doc.image),
          publishedAt: doc.published_at ?? null,
          slug: doc.slug?.trim() || null,
        }
        const recovery = (loadRecovery ?? readNoteRecovery)({ userId, docId: doc.id })
        const recovered = recoverNoteFields(recovery, server)
        const next = { ...recovered, contentMd: expandJammedDefinitions(recovered.contentMd) }
        cloudAck(server)
        applyServerDoc(doc)
        setCloudId(doc.id)
        applyMergedFields(next)
        setSaved(server)
        if (recovery?.pending && !sameCloudFields(recovery.pending.expectedFields ?? recovery.pending.fields, server)) {
          restorePending?.(recovery.pending, { fields: recovery.base, revision: recovery.revision }, doc)
        }
        if (doc.last_error) {
          showNoteNotice(userFacingError(doc.last_error, scheduleFailed))
        }
      } catch (err) {
        if (!controller.signal.aborted) {
          if ((docId != null || noteId != null) && isNoteDocGone(err) && onGone) onGone()
          else showNoteNotice(userFacingError(err, loadFailed))
        }
      } finally {
        if (!controller.signal.aborted) setLoading(false)
      }
    }
    void run()
    return () => {
      controller.abort()
    }
  }, [noteId, docId, userId])
}
