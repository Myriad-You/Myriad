import type { MediaAsset, MediaCursor, MediaFilter, MediaOrigin } from '../../services/mediaApi'
import type { CommentItem } from '../../services/phantasiApi'
import type { PhantasiNoteDoc, PhantasiSourceApplication } from '../../types/phantasi'
import type { WorkbenchPane } from './logic/board'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import * as mediaApi from '../../services/mediaApi'
import * as phantasiApi from '../../services/phantasiApi'
import { userFacingError } from '../../utils/userFacingError'
import { splitWorkbenchUploads, uploadInOrder } from './logic/mediaUpload'
import { RequestTurn } from './logic/requestTurn'
import { workbenchMediaFormatKey } from './logic/workbench'
import { loadNoteDocs } from './pageData'

const ALL_MEDIA: MediaFilter = { origin: 'all', format: 'all', query: '' }

export function usePhantasiWorkbench(
  pane: WorkbenchPane,
  docsEpoch: number,
  labels: {
    loadFailed: string
    noteDeleteFailed: string
    unscheduleFailed: string
    mediaLoadFailed: string
    mediaUploadFailed: string
    /** 后面接「: 文件名」。 */
    mediaUploadSkipped: string
    mediaDeleteFailed: string
    commentDeleteFailed: string
    reviewApproveFailed: string
    reviewRejectFailed: string
    reviewDeleteFailed: string
  },
  setError: (message: string) => void,
  mediaOrigin?: MediaFilter['origin'] | null,
) {
  // blog 页自己取数。笔记页和分类页都要笔记，分类名和占用从笔记来。
  const needsCategories = pane === 'notes' || pane === 'noteCategories'
  const needsNotes = pane === 'home' || pane === 'notesIo' || needsCategories
  // Overview owns real counts, the review queue, and recent media too.
  const needsMedia = pane === 'home' || pane === 'media'
  const needsComments = pane === 'home' || pane === 'comments'
  const needsApplications = pane === 'home' || pane === 'reviews'
  // Mutation completions can retain a reload callback from the previous pane.
  const demandRef = useRef({ needsNotes, needsMedia, needsComments, needsApplications })
  demandRef.current = { needsNotes, needsMedia, needsComments, needsApplications }
  const [docs, setDocs] = useState<PhantasiNoteDoc[]>([])
  const [media, setMedia] = useState<MediaAsset[]>([])
  const [mediaTotal, setMediaTotal] = useState(0)
  const [mediaOrigins, setMediaOrigins] = useState<MediaOrigin[] | undefined>(
    undefined,
  )
  const [mediaFormats, setMediaFormats] = useState<string[] | undefined>(
    undefined,
  )
  const [mediaFilter, setMediaFilter] = useState<MediaFilter>(ALL_MEDIA)
  const [mediaCursor, setMediaCursor] = useState<MediaCursor | null>(null)
  const [mediaEpoch, setMediaEpoch] = useState(0)
  const mediaFetching = useRef(false)
  const effectiveMediaFilter = useMemo<MediaFilter>(
    () => (mediaOrigin ? { ...mediaFilter, origin: mediaOrigin } : mediaFilter),
    [mediaFilter, mediaOrigin],
  )
  const activeMediaFilter = pane === 'media' ? effectiveMediaFilter : ALL_MEDIA
  const [comments, setComments] = useState<CommentItem[]>([])
  const [applications, setApplications] = useState<PhantasiSourceApplication[]>(
    [],
  )
  const [notesLoading, setNotesLoading] = useState(false)
  const [mediaLoading, setMediaLoading] = useState(false)
  const [commentsLoading, setCommentsLoading] = useState(false)
  const [applicationsLoading, setApplicationsLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [uploadProgress, setUploadProgress] = useState<{ current: number; total: number } | null>(null)
  const notesTurn = useRef(new RequestTurn())
  const mediaTurn = useRef(new RequestTurn())
  const commentsTurn = useRef(new RequestTurn())
  const applicationsTurn = useRef(new RequestTurn())
  const labelsRef = useRef(labels)
  labelsRef.current = labels

  const loadNotes = useCallback(async () => {
    if (!needsNotes || !demandRef.current.needsNotes) {
      setNotesLoading(false)
      return
    }
    const signal = notesTurn.current.begin()
    setNotesLoading(true)
    try {
      const next = await loadNoteDocs(signal)
      if (!signal.aborted) setDocs(next)
    } catch (err) {
      if (signal.aborted) return
      setError(userFacingError(err, labelsRef.current.loadFailed))
      setDocs([])
    } finally {
      if (!signal.aborted) setNotesLoading(false)
    }
  }, [needsNotes, setError])

  const loadMedia = useCallback(async () => {
    if (!needsMedia || !demandRef.current.needsMedia) {
      setMediaLoading(false)
      return
    }
    const signal = mediaTurn.current.begin()
    mediaFetching.current = true
    setMediaLoading(true)
    setMediaCursor(null)
    try {
      const next = await mediaApi.listMedia({ filter: activeMediaFilter }, signal)
      if (!signal.aborted) {
        setMedia(next.items)
        setMediaTotal(next.total ?? 0)
        setMediaCursor(next.next_cursor)
        if (next.origins) {
          setMediaOrigins(next.origins)
        } else if (
          activeMediaFilter.origin === 'all' &&
          activeMediaFilter.format === 'all' &&
          !activeMediaFilter.query.trim()
        ) {
          setMediaOrigins(
            Array.from(new Set(next.items.map(mediaApi.mediaAssetOrigin))),
          )
        }
        if (next.formats) {
          setMediaFormats(next.formats)
        } else if (
          activeMediaFilter.format === 'all' &&
          !activeMediaFilter.query.trim()
        ) {
          setMediaFormats(
            Array.from(new Set(next.items.map(workbenchMediaFormatKey))),
          )
        }
      }
    } catch (err) {
      if (signal.aborted) return
      setError(userFacingError(err, labelsRef.current.mediaLoadFailed))
      setMedia([])
    } finally {
      if (!signal.aborted) {
        mediaFetching.current = false
        setMediaLoading(false)
      }
    }
  }, [needsMedia, activeMediaFilter, setError])

  const loadMoreMedia = useCallback(async () => {
    if (!mediaCursor || mediaFetching.current || !demandRef.current.needsMedia) return
    const signal = mediaTurn.current.begin()
    mediaFetching.current = true
    setMediaLoading(true)
    try {
      const next = await mediaApi.listMedia({ filter: activeMediaFilter, cursor: mediaCursor }, signal)
      if (!signal.aborted) {
        setMedia(previous => [...previous, ...next.items])
        setMediaCursor(next.next_cursor)
      }
    } catch (err) {
      if (!signal.aborted) setError(userFacingError(err, labelsRef.current.mediaLoadFailed))
    } finally {
      if (!signal.aborted) {
        mediaFetching.current = false
        setMediaLoading(false)
      }
    }
  }, [activeMediaFilter, mediaCursor, setError])

  useEffect(() => {
    void loadNotes()
    return () => notesTurn.current.cancel()
  }, [docsEpoch, loadNotes])

  useEffect(() => {
    void loadMedia()
    return () => mediaTurn.current.cancel()
  }, [loadMedia, mediaEpoch])

  const loadComments = useCallback(async () => {
    if (!needsComments || !demandRef.current.needsComments) {
      setCommentsLoading(false)
      return
    }
    const signal = commentsTurn.current.begin()
    setCommentsLoading(true)
    try {
      const next = await phantasiApi.listAdminComments({ signal })
      if (!signal.aborted) setComments(next.comments ?? [])
    } catch (err) {
      if (signal.aborted) return
      setError(userFacingError(err, labelsRef.current.loadFailed))
      setComments([])
    } finally {
      if (!signal.aborted) setCommentsLoading(false)
    }
  }, [needsComments, setError])

  useEffect(() => {
    void loadComments()
    return () => commentsTurn.current.cancel()
  }, [loadComments])

  const loadApplications = useCallback(async () => {
    if (!needsApplications || !demandRef.current.needsApplications) {
      setApplicationsLoading(false)
      return
    }
    const signal = applicationsTurn.current.begin()
    setApplicationsLoading(true)
    try {
      const next = await phantasiApi.listSourceApplications({ signal })
      if (!signal.aborted) setApplications(next.applications ?? [])
    } catch (err) {
      if (signal.aborted) return
      setError(userFacingError(err, labelsRef.current.loadFailed))
      setApplications([])
    } finally {
      if (!signal.aborted) setApplicationsLoading(false)
    }
  }, [needsApplications, setError])

  useEffect(() => {
    void loadApplications()
    return () => applicationsTurn.current.cancel()
  }, [loadApplications])

  const removeNotes = useCallback(
    async (docs: PhantasiNoteDoc[]): Promise<boolean> => {
      if (docs.length === 0) return true
      setBusy(true)
      try {
        const results = await Promise.allSettled(
          docs.map(async (doc) => {
            if (doc.item_id != null) await phantasiApi.deleteNote(doc.item_id)
            else await phantasiApi.deleteNoteDoc(doc.id)
            return doc.id
          }),
        )
        const dropped = new Set(
          results.flatMap((result) =>
            result.status === 'fulfilled' ? [result.value] : [],
          ),
        )
        if (dropped.size > 0) {
          setDocs((prev) => prev.filter((row) => !dropped.has(row.id)))
        }
        const failed = docs.filter((_, index) => results[index]?.status === 'rejected')
        if (failed.length > 0) {
          const first = results.find((result) => result.status === 'rejected')
          const detail = userFacingError(
            first && first.status === 'rejected' ? first.reason : null,
            labelsRef.current.noteDeleteFailed,
          )
          const names = failed
            .map((doc) => doc.title.trim() || `#${doc.id}`)
            .join('、')
          setError(`${detail}: ${names}`)
          return false
        }
        return true
      } finally {
        setBusy(false)
      }
    },
    [setError],
  )

  const removeNote = useCallback(
    async (doc: PhantasiNoteDoc) => removeNotes([doc]),
    [removeNotes],
  )

  const unschedule = useCallback(
    async (id: number, revision: number) => {
      setBusy(true)
      try {
        const next = await phantasiApi.unscheduleNoteDoc(id, { revision })
        setDocs((prev) => prev.map((row) => (row.id === id ? next : row)))
      } catch (err) {
        setError(userFacingError(err, labelsRef.current.unscheduleFailed))
      } finally {
        setBusy(false)
      }
    },
    [setError],
  )

  // 多个文件一个接一个传，全部传完只刷新一次列表。不能传的类型直接跳过并提示。
  const upload = useCallback(
    async (files: readonly File[]) => {
      const { accepted, rejected } = splitWorkbenchUploads(files)
      const skipped = rejected.length > 0
        ? `${labelsRef.current.mediaUploadSkipped}: ${rejected.map(file => file.name).join('、')}`
        : ''
      if (skipped) setError(skipped)
      if (accepted.length === 0) return
      setBusy(true)
      setUploadProgress({ current: 1, total: accepted.length })
      try {
        const { done, failed } = await uploadInOrder(
          accepted,
          file => mediaApi.uploadMedia(file),
          index => setUploadProgress({ current: index + 1, total: accepted.length }),
        )
        if (done > 0) setMediaEpoch(value => value + 1)
        if (failed.length > 0) {
          const detail = userFacingError(failed[0]?.error, labelsRef.current.mediaUploadFailed)
          const names = failed.map(row => row.item.name).join('、')
          // 同一个提示位会顶掉前面的，跳过的文件一并带上。
          setError([`${detail}: ${names}`, skipped].filter(Boolean).join(' '))
        }
      } finally {
        setUploadProgress(null)
        setBusy(false)
      }
    },
    [setError],
  )

  const removeMedia = useCallback(
    async (id: number) => {
      setBusy(true)
      try {
        await mediaApi.deleteMedia(id)
        setMediaEpoch(value => value + 1)
      } catch (err) {
        setError(userFacingError(err, labelsRef.current.mediaDeleteFailed))
      } finally {
        setBusy(false)
      }
    },
    [setError],
  )

  const removeComments = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return
      setBusy(true)
      try {
        const results = await Promise.allSettled(
          ids.map(async (id) => {
            await phantasiApi.deleteComment(id)
            return id
          }),
        )
        const dropped = new Set(
          results.flatMap((result) =>
            result.status === 'fulfilled' ? [result.value] : [],
          ),
        )
        if (dropped.size > 0) {
          setComments((prev) => prev.filter((row) => !dropped.has(row.id)))
        }
        const failedIds = ids.filter((_, index) => results[index]?.status === 'rejected')
        if (failedIds.length > 0) {
          const failed = results.find((result) => result.status === 'rejected')
          const detail = userFacingError(
            failed && failed.status === 'rejected' ? failed.reason : null,
            labelsRef.current.commentDeleteFailed,
          )
          setError(`${detail}: #${failedIds.join('、#')}`)
        }
      } finally {
        setBusy(false)
      }
    },
    [setError],
  )

  const approveApplication = useCallback(
    async (id: number) => {
      setBusy(true)
      try {
        const next = await phantasiApi.approveSourceApplication(id)
        setApplications((prev) =>
          prev.map((row) => (row.id === id ? next.application : row)),
        )
        return true
      } catch (err) {
        setError(userFacingError(err, labelsRef.current.reviewApproveFailed))
        return false
      } finally {
        setBusy(false)
      }
    },
    [setError],
  )

  const rejectApplication = useCallback(
    async (id: number) => {
      setBusy(true)
      try {
        const next = await phantasiApi.rejectSourceApplication(id)
        setApplications((prev) =>
          prev.map((row) => (row.id === id ? next.application : row)),
        )
      } catch (err) {
        setError(userFacingError(err, labelsRef.current.reviewRejectFailed))
      } finally {
        setBusy(false)
      }
    },
    [setError],
  )

  const removeApplications = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return
      setBusy(true)
      try {
        const results = await Promise.allSettled(
          ids.map(async (id) => {
            await phantasiApi.deleteSourceApplication(id)
            return id
          }),
        )
        const dropped = new Set(
          results.flatMap((result) =>
            result.status === 'fulfilled' ? [result.value] : [],
          ),
        )
        if (dropped.size > 0) {
          setApplications((prev) => prev.filter((row) => !dropped.has(row.id)))
        }
        const failedIds = ids.filter((_, index) => results[index]?.status === 'rejected')
        if (failedIds.length > 0) {
          const failed = results.find((result) => result.status === 'rejected')
          const detail = userFacingError(
            failed && failed.status === 'rejected' ? failed.reason : null,
            labelsRef.current.reviewDeleteFailed,
          )
          setError(`${detail}: #${failedIds.join('、#')}`)
        }
      } finally {
        setBusy(false)
      }
    },
    [setError],
  )

  return {
    needsCategories,
    docs,
    media,
    mediaTotal,
    mediaOrigins,
    mediaFormats,
    mediaFilter: effectiveMediaFilter,
    setMediaFilter,
    mediaHasMore: mediaCursor !== null,
    loadMoreMedia,
    comments,
    applications,
    notesLoading,
    mediaLoading,
    commentsLoading,
    applicationsLoading,
    busy,
    uploadProgress,
    removeNote,
    removeNotes,
    removeComments,
    approveApplication,
    rejectApplication,
    removeApplications,
    unschedule,
    upload,
    removeMedia,
    reloadNotes: loadNotes,
    reloadMedia: loadMedia,
    acceptMedia: () => setMediaEpoch(value => value + 1),
    reloadComments: loadComments,
    reloadApplications: loadApplications,
  }
}
