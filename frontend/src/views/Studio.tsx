import type { WorkbenchPane } from '../components/phantasi/logic/board'
import type { StudioSection } from '../components/studio/studioRoutes'
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router-dom'
import AnimatedView from '../components/AnimatedView'
import { workbenchPendingReviews } from '../components/phantasi/logic/workbenchHome'
import { useNoteTransfer } from '../components/phantasi/manager/useNoteTransfer'
import { usePhantasiCategories } from '../components/phantasi/usePhantasiCategories'
import { usePhantasiWorkbench } from '../components/phantasi/usePhantasiWorkbench'
import { openPublishedItem, openStudioNote } from '../components/studio/studioNoteNav'
import { parseStudioPath, studioMediaOrigin } from '../components/studio/studioRoutes'
import { StudioShell } from '../components/studio/StudioShell'
import { useI18n } from '../contexts/I18nContext'
import { showToast } from '../utils/toastManager'

const NoteEditor = lazy(() => import('../components/phantasi/notes/NoteEditor'))

function deskPane(section: StudioSection): WorkbenchPane {
  switch (section) {
    case 'home':
      return 'home'
    case 'notes':
      return 'notes'
    case 'comments':
      return 'comments'
    case 'media':
    case 'mediaUploads':
    case 'mediaGenerated':
    case 'mediaKept':
    case 'mediaChannel':
    case 'mediaImported':
      return 'media'
    case 'reviews':
      return 'reviews'
    case 'categories':
      return 'noteCategories'
    case 'import':
      return 'notesIo'
    case 'blog':
      return 'blog'
  }
}

export default function Studio() {
  const location = useLocation()
  const parsed = parseStudioPath(location.pathname)
  if (!parsed) return <Navigate to="/studio" replace />
  if (parsed.kind === 'editor') return <StudioEditor docId={parsed.docId} />
  return <StudioDesk section={parsed.section} />
}

function StudioEditor({ docId }: { docId: number | 'new' }) {
  const navigate = useNavigate()
  const [pin, setPin] = useState(docId)
  const ignoreUrl = useRef(false)
  useEffect(() => {
    if (ignoreUrl.current) {
      ignoreUrl.current = false
      return
    }
    setPin(docId)
  }, [docId])
  return (
    <Suspense fallback={null}>
      <NoteEditor
        key={pin === 'new' ? 'new' : String(pin)}
        docId={pin === 'new' ? undefined : pin}
        onClose={() => navigate('/studio/notes')}
        onSaved={() => {}}
        onDeleted={() => navigate('/studio/notes')}
        onCloudId={(id) => {
          if (pin !== 'new') return
          ignoreUrl.current = true
          navigate(`/studio/notes/${id}`, { replace: true })
        }}
      />
    </Suspense>
  )
}

function StudioDesk({ section }: { section: StudioSection }) {
  const { t } = useI18n()
  const navigate = useNavigate()
  const [docsEpoch, setDocsEpoch] = useState(0)
  const setError = useCallback((message: string) => {
    if (message) showToast({ message, type: 'error', replaceKey: 'studio' })
  }, [])
  const workbench = usePhantasiWorkbench(
    deskPane(section),
    docsEpoch,
    {
      loadFailed: t.phantasi.workbenchLoadFailed,
      noteDeleteFailed: t.errors.operationFailed,
      unscheduleFailed: t.phantasi.workbenchUnscheduleFailed,
      mediaLoadFailed: t.errors.mediaLoadFailed,
      mediaUploadFailed: t.errors.mediaUploadFailed,
      mediaDeleteFailed: t.errors.mediaDeleteFailed,
      commentDeleteFailed: t.phantasi.workbenchCommentDeleteFailed,
      reviewApproveFailed: t.phantasi.workbenchReviewApproveFailed,
      reviewRejectFailed: t.phantasi.workbenchReviewRejectFailed,
      reviewDeleteFailed: t.phantasi.workbenchReviewDeleteFailed,
    },
    setError,
    studioMediaOrigin(section),
  )
  const categories = usePhantasiCategories(
    section === 'notes' || section === 'categories' || section === 'home',
    workbench.docs,
    {
      loadFailed: t.phantasi.workbenchCategoryLoadFailed,
      createFailed: t.phantasi.workbenchCategoryCreateFailed,
      renameFailed: t.phantasi.workbenchCategoryRenameFailed,
      deleteFailed: t.phantasi.workbenchCategoryDeleteFailed,
      assignFailed: t.phantasi.workbenchAssignCategoryFailed,
      categoryFull: t.phantasi.workbenchCategoryFull,
      untitled: t.phantasi.workbenchNoteUntitled,
    },
    setError,
    () => {
      setDocsEpoch((value) => value + 1)
    },
  )
  const notesIo = useNoteTransfer(workbench.docs, () => {
    setDocsEpoch((value) => value + 1)
  })
  const failed = t.studio.openFailed
  return (
    <AnimatedView className="studio-page min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
      <StudioShell
        section={section}
        docs={workbench.docs}
        media={workbench.media}
        mediaTotal={workbench.mediaTotal}
        mediaFilter={workbench.mediaFilter}
        onMediaFilter={workbench.setMediaFilter}
        mediaHasMore={workbench.mediaHasMore}
        onLoadMoreMedia={workbench.loadMoreMedia}
        comments={workbench.comments}
        applications={workbench.applications}
        notesLoading={workbench.notesLoading}
        mediaLoading={workbench.mediaLoading}
        commentsLoading={workbench.commentsLoading}
        applicationsLoading={workbench.applicationsLoading}
        busy={workbench.busy}
        noteCategories={categories.noteRows.map((row) => row.name)}
        categoryRows={categories.noteRows}
        categoriesLoading={categories.loading}
        categoriesBusy={categories.busy}
        onCreateCategory={categories.create}
        onRenameCategory={(from, to) => categories.rename(from, to)}
        onDeleteCategory={(name) => categories.remove(name)}
        notesBusy={notesIo.loading}
        notesKind={notesIo.activeKind}
        notesProgress={
          notesIo.progress
            ? `${notesIo.progress.step}${
                notesIo.progress.total > 0
                  ? ` ${notesIo.progress.current}/${notesIo.progress.total}`
                  : ''
              }`
            : null
        }
        onWrite={() => {
          void openStudioNote(navigate, { kind: 'new' })
        }}
        onOpenNote={(open) => {
          void openStudioNote(navigate, open, setError, failed)
        }}
        onDeleteNotes={async (docs) => workbench.removeNotes(docs)}
        onUnschedule={workbench.unschedule}
        onAssignNotes={(docs, name) => {
          void categories.assign(docs.map((doc) => doc.id), name)
        }}
        onDeleteComments={(ids) => {
          void workbench.removeComments(ids)
        }}
        onOpenCommentItem={(id) => {
          void openPublishedItem(id, setError, failed)
        }}
        onApproveApplication={workbench.approveApplication}
        onRejectApplication={(id) => {
          void workbench.rejectApplication(id)
        }}
        onDeleteApplications={(ids) => {
          void workbench.removeApplications(ids)
        }}
        onUpload={workbench.upload}
        onDeleteMedia={workbench.removeMedia}
        onMediaSaved={() => {
          workbench.acceptMedia()
        }}
        onExportNotes={(kind) => {
          void notesIo.exportKind(kind)
        }}
        onImportNotes={(kind, file) => {
          void notesIo.importKind(kind, file)
        }}
        pendingReviews={workbenchPendingReviews(workbench.applications).length}
      />
    </AnimatedView>
  )
}
