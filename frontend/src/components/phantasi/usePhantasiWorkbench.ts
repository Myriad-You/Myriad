import type { MediaAsset, MediaCursor, MediaFilter } from '../../services/mediaApi'
import type { CommentItem } from '../../services/phantasiApi'
import type { PhantasiNoteDoc, PhantasiSourceApplication } from '../../types/phantasi'
import type { WorkbenchPane } from './logic/board'
import { useCallback, useEffect, useRef, useState } from 'react'
import * as mediaApi from '../../services/mediaApi'
import * as phantasiApi from '../../services/phantasiApi'
import { userFacingError } from '../../utils/userFacingError'
import { RequestTurn } from './logic/requestTurn'
import { loadNoteDocs } from './pageData'

const ALL_MEDIA: MediaFilter = { kind: 'all', format: 'all', query: '' }

export function usePhantasiWorkbench(
  pane: WorkbenchPane,
  docsEpoch: number,
  labels: {
    loadFailed: string
    noteDeleteFailed: string
    unscheduleFailed: string
    mediaLoadFailed: string
    mediaUploadFailed: string
    mediaDeleteFailed: string
    commentDeleteFailed: string
    reviewApproveFailed: string
    reviewRejectFailed: string
    reviewDeleteFailed: string
  },
  setError: (message: string) => void,
) {
  // Category pickers include names used only by notes; category administration
  // also needs those notes to preserve classifications on the other page.
  const needsCategories =
    pane === 'notes' ||
    pane === 'noteCategories' ||
    pane === 'sourceCategories' ||
    pane === 'sources' ||
    pane === 'add'
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
  const [mediaFilter, setMediaFilter] = useState<MediaFilter>(ALL_MEDIA)
  const [mediaCursor, setMediaCursor] = useState<MediaCursor | null>(null)
  const [mediaEpoch, setMediaEpoch] = useState(0)
  const mediaFetching = useRef(false)
  const activeMediaFilter = pane === 'media' ? mediaFilter : ALL_MEDIA
  const [comments, setComments] = useState<CommentItem[]>([])
  const [applications, setApplications] = useState<PhantasiSourceApplication[]>(
    [],
  )
  const [notesLoading, setNotesLoading] = useState(false)
  const [mediaLoading, setMediaLoading] = useState(false)
  const [commentsLoading, setCommentsLoading] = useState(false)
  const [applicationsLoading, setApplicationsLoading] = useState(false)
  const [busy, setBusy] = useState(false)
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

  const upload = useCallback(
    async (file: File) => {
      setBusy(true)
      try {
        await mediaApi.uploadMedia(file)
        setMediaEpoch(value => value + 1)
      } catch (err) {
        setError(userFacingError(err, labelsRef.current.mediaUploadFailed))
      } finally {
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
    mediaFilter,
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
