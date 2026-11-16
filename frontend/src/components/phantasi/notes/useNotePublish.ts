import type { MutableRefObject } from 'react'
import type { NoteDraftKey } from './noteDraft'
import type { NoteCloudSaveHandle } from './useNoteCloudSave'
import { useCallback } from 'react'
import * as phantasiApi from '../../../services/phantasiApi'
import { userFacingError } from '../../../utils/userFacingError'
import { showNoteNotice } from '../phantasiNotice'
import { isNoteDocGone } from './noteCollab'
import {
  clearNoteDraft,
} from './noteDraft'
import {
  countNoteChars,
  MAX_NOTE_BODY_CHARS,
  MAX_NOTE_TITLE_CHARS,
  noteFieldError,
  noteScheduleError,
  toNoteWritePayload,
} from './noteFields'
import { isOldPathTakenError, oldPathsFromField } from './noteOldPaths'
import { isSlugTakenError, noteSlugWire } from './noteSlug'

export function useNotePublish({
  title,
  contentMd,
  topic,
  cover,
  publishedAt,
  slug = null,
  scheduledAt,
  noteId,
  cloudId,
  gone = false,
  draftKey,
  clearRecovery,
  docStatus,
  saving,
  setSaving,
  setContentMd,
  setPublishedAt,
  revisionRef,
  runWrite,
  onSaved,
  onDeleted,
  onPublished,
  onClose,
  format,
  labels,
}: {
  title: string
  contentMd: string
  topic: string | null
  cover: string | null
  publishedAt: number | null
  slug?: string | null
  scheduledAt: number | null
  noteId?: number
  cloudId: number | null
  /** 这篇已经被删了：不再往它写，离开时也不用再问、不用再删。 */
  gone?: boolean
  draftKey: NoteDraftKey | null
  clearRecovery?: () => void
  docStatus: 'draft' | 'scheduled' | 'published'
  saving: boolean
  setSaving: (value: boolean) => void
  setContentMd: (value: string) => void
  setPublishedAt: (value: number | null) => void
  revisionRef: MutableRefObject<number>
  runWrite: NoteCloudSaveHandle['runWrite']
  onSaved: (id: number) => void
  onDeleted?: (id: number) => void
  onPublished?: (link: string) => void
  onClose: () => void
  format: (template: string, values: Record<string, string | number>) => string
  labels: {
    titleRequired: string
    titleTooLong: string
    bodyTooLong: string
    saveFailed: string
    scheduleNeedTime: string
    schedulePast: string
    deleteConfirm: string
    deleteFailed: string
    discardConfirm: string
    slugInvalid?: string
    slugTaken?: string
    oldPathTaken?: string
  }
}): {
  fieldMessage: (kind: ReturnType<typeof noteFieldError>) => string | null
  handleSave: () => Promise<void>
  handleSchedule: () => Promise<void>
  handleUnschedule: () => Promise<void>
  handleDelete: () => Promise<void>
  requestClose: (dirty: boolean) => Promise<void>
  /** 离开这篇之前的检查：没保存的先问、空的新笔记删掉。返回能不能离开，不关编辑器。 */
  prepareClose: (dirty: boolean) => Promise<boolean>
} {
  const clearLocalRecovery = useCallback(() => {
    if (clearRecovery) clearRecovery()
    else clearNoteDraft(draftKey)
  }, [clearRecovery, draftKey])

  const fieldMessage = useCallback(
    (kind: ReturnType<typeof noteFieldError>) => {
      if (kind === 'empty-title') return labels.titleRequired
      if (kind === 'title-too-long') {
        return format(labels.titleTooLong, {
          max: MAX_NOTE_TITLE_CHARS,
          chars: countNoteChars(title.trim()),
        })
      }
      if (kind === 'body-too-long') {
        return format(labels.bodyTooLong, {
          max: MAX_NOTE_BODY_CHARS,
          chars: countNoteChars(contentMd),
        })
      }
      return null
    },
    [contentMd, format, labels.bodyTooLong, labels.titleRequired, labels.titleTooLong, title],
  )

  const handleSave = useCallback(async () => {
    if (gone) return
    if (saving) return
    const invalid = noteFieldError(title, contentMd)
    if (invalid) {
      showNoteNotice(fieldMessage(invalid))
      return
    }
    const slugWire = noteSlugWire(slug)
    if (slugWire === undefined) {
      showNoteNotice(labels.slugInvalid ?? labels.saveFailed)
      return
    }
    setSaving(true)
    const payload = toNoteWritePayload(title, contentMd, topic, cover, publishedAt)
    try {
      if (cloudId != null) {
        await runWrite(async (current, track) => {
          const latest = toNoteWritePayload(current.title, current.contentMd, current.topic, current.cover, current.publishedAt)
          const currentSlug = noteSlugWire(slug)
          if (currentSlug === undefined) {
            showNoteNotice(labels.slugInvalid ?? labels.saveFailed)
            return
          }
          // 旧地址只有已发布的笔记才有：更新到博客时整组带上，没改的后端不动。
          const oldPaths = typeof current.oldPaths === 'string' ? current.oldPaths : undefined
          const receipt = track({
            title: latest.title, contentMd: latest.content_md, topic: latest.topic,
            cover: latest.image ?? null, publishedAt: latest.published_at ?? null,
            slug: currentSlug,
            ...(oldPaths !== undefined ? { oldPaths } : {}),
          })
          const result = await phantasiApi.publishNoteDoc(cloudId, {
            ...latest,
            slug: currentSlug,
            ...(oldPaths !== undefined ? { old_paths: oldPathsFromField(oldPaths) } : {}),
            client_request_id: receipt.requestId,
            image: latest.image ?? null,
            revision: revisionRef.current,
          })
          if (!await receipt.receiveDoc(result.doc)) return
          onPublished?.(result.link)
          onSaved(result.id)
        })
        return
      }
      const result =
        noteId === undefined
          ? await phantasiApi.createNote(payload)
          : await phantasiApi.updateNote(noteId, payload)
      clearLocalRecovery()
      onSaved(result.id)
    } catch (err) {
      showNoteNotice(
        isSlugTakenError(err)
          ? (labels.slugTaken ?? labels.saveFailed)
          : isOldPathTakenError(err)
            ? (labels.oldPathTaken ?? labels.saveFailed)
            : userFacingError(err, labels.saveFailed),
      )
    } finally {
      setSaving(false)
    }
  }, [
    runWrite,
    cloudId,
    contentMd,
    cover,
    clearLocalRecovery,
    fieldMessage,
    gone,
    labels.saveFailed,
    labels.slugInvalid,
    labels.slugTaken,
    labels.oldPathTaken,
    noteId,
    onPublished,
    onSaved,
    publishedAt,
    revisionRef,
    slug,
    saving,
    setContentMd,
    setSaving,
    title,
    topic,
  ])

  const handleSchedule = useCallback(async () => {
    if (cloudId == null || saving) return
    const invalid = noteFieldError(title, contentMd)
    if (invalid) {
      showNoteNotice(fieldMessage(invalid))
      return
    }
    const timeError = noteScheduleError(scheduledAt)
    if (timeError === 'missing-time') {
      showNoteNotice(labels.scheduleNeedTime)
      return
    }
    if (timeError === 'already-due') {
      showNoteNotice(labels.schedulePast)
      return
    }
    const when = scheduledAt
    if (when == null) return
    setSaving(true)
    try {
      await runWrite(async (current, track) => {
        const receipt = track(current)
        const scheduledSlug = noteSlugWire(current.slug)
        const doc = await phantasiApi.scheduleNoteDoc(cloudId, {
          client_request_id: receipt.requestId,
          title: current.title,
          content_md: current.contentMd,
          topic: current.topic,
          image: current.cover,
          ...(scheduledSlug !== undefined ? { slug: scheduledSlug } : {}),
          scheduled_at: when,
          revision: revisionRef.current,
        })
        if (await receipt.receiveDoc(doc)) setPublishedAt(doc.published_at)
      })
    } catch (err) {
      showNoteNotice(userFacingError(err, labels.saveFailed))
    } finally {
      setSaving(false)
    }
  }, [
    runWrite,
    cloudId,
    contentMd,
    cover,
    fieldMessage,
    labels.saveFailed,
    labels.scheduleNeedTime,
    labels.schedulePast,
    revisionRef,
    saving,
    scheduledAt,
    setPublishedAt,
    setSaving,
    title,
    topic,
  ])

  const handleUnschedule = useCallback(async () => {
    if (cloudId == null || saving) return
    setSaving(true)
    try {
      await runWrite(async (_current, track) => {
        const receipt = track()
        const doc = await phantasiApi.unscheduleNoteDoc(cloudId, {
          client_request_id: receipt.requestId,
          revision: revisionRef.current,
        })
        await receipt.receiveDoc(doc)
      })
    } catch (err) {
      showNoteNotice(userFacingError(err, labels.saveFailed))
    } finally {
      setSaving(false)
    }
  }, [cloudId, labels.saveFailed, revisionRef, runWrite, saving, setSaving])

  const handleDelete = useCallback(async () => {
    if (saving) return
    if (!window.confirm(labels.deleteConfirm)) return
    setSaving(true)
    try {
      if (noteId !== undefined) {
        await runWrite(() => phantasiApi.deleteNote(noteId))
        clearLocalRecovery()
        onDeleted?.(noteId)
        return
      }
      if (cloudId != null && docStatus !== 'published') {
        await runWrite(() => deleteDocIfPresent(cloudId))
        clearLocalRecovery()
        onClose()
      }
    } catch (err) {
      showNoteNotice(userFacingError(err, labels.deleteFailed))
      setSaving(false)
    }
  }, [
    runWrite,
    clearLocalRecovery,
    cloudId,
    docStatus,
    labels.deleteConfirm,
    labels.deleteFailed,
    noteId,
    onClose,
    onDeleted,
    saving,
    setSaving,
  ])

  const prepareClose = useCallback(
    async (dirty: boolean): Promise<boolean> => {
      if (gone) return true
      if (dirty) {
        if (!window.confirm(labels.discardConfirm)) return false
        clearLocalRecovery()
      }
      if (
        cloudId != null &&
        noteId === undefined &&
        docStatus === 'draft' &&
        !title.trim() &&
        !contentMd.trim()
      ) {
        try {
          await runWrite(() => deleteDocIfPresent(cloudId))
        } catch (err) {
          showNoteNotice(userFacingError(err, labels.deleteFailed))
          return false
        }
        clearLocalRecovery()
      }
      return true
    },
    [
      runWrite,
      cloudId,
      contentMd,
      docStatus,
      clearLocalRecovery,
      labels.deleteFailed,
      labels.discardConfirm,
      gone,
      noteId,
      title,
    ],
  )

  const requestClose = useCallback(
    async (dirty: boolean) => {
      if (await prepareClose(dirty)) onClose()
    },
    [onClose, prepareClose],
  )

  return {
    fieldMessage,
    handleSave,
    handleSchedule,
    handleUnschedule,
    handleDelete,
    requestClose,
    prepareClose,
  }
}

/** 删一篇云端文档；已经不在了（别的窗口、别人先删了）也算删掉，不能因此卡住关窗口、换篇。 */
async function deleteDocIfPresent(cloudId: number): Promise<void> {
  try {
    await phantasiApi.deleteNoteDoc(cloudId)
  } catch (error) {
    if (!isNoteDocGone(error)) throw error
  }
}
