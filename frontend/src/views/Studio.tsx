import type { WorkbenchPane } from '../components/phantasi/logic/board'
import type { BlogBuildMark } from '../components/studio/studioIsland'
import type { StudioSection } from '../components/studio/studioRoutes'
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router-dom'
import AnimatedView from '../components/AnimatedView'
import { workbenchPendingReviews } from '../components/phantasi/logic/workbenchHome'
import { useNoteTransfer } from '../components/phantasi/manager/useNoteTransfer'
import { usePhantasiCategories } from '../components/phantasi/usePhantasiCategories'
import { usePhantasiWorkbench } from '../components/phantasi/usePhantasiWorkbench'
import {
  blogReloadDecision,
  closeStudioIsland,
  findStudioIsland,
  withoutStudioParam,
} from '../components/studio/studioIsland'
import { openPublishedItem, openStudioNote } from '../components/studio/studioNoteNav'
import {
  parseStudioPath,
  STUDIO_ROOT,
  studioMediaOrigin,
  studioNotePath,
  studioSectionPath,
} from '../components/studio/studioRoutes'
import { StudioShell } from '../components/studio/StudioShell'
import { ToastContainer } from '../components/ToastContainer'
import { useI18n } from '../contexts/I18nContext'
import { getBlogSettings } from '../services/blogApi'
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

async function buildMark(): Promise<BlogBuildMark | null> {
  try {
    const { build } = await getBlogSettings()
    return { status: build.status, lastBuiltAt: build.last_built_at ?? null }
  } catch {
    return null
  }
}

const RELOAD_POLL_MS = 1500
const RELOAD_POLL_LIMIT = 40

/**
 * 和外层创作台岛配合：Esc 和点遮罩收岛；收岛时博客若已重新生成，刷新外层页面，
 * 让站长看到刚改的内容。还在生成就等它生成完。
 */
function useStudioIsland() {
  useEffect(() => {
    const host = findStudioIsland()
    if (!host) return
    let opened: BlogBuildMark | null = null
    let disposed = false
    let timer = 0
    const remember = () => {
      void buildMark().then((mark) => {
        if (!disposed) opened = mark
      })
    }
    const settle = (tries: number) => {
      void buildMark().then((now) => {
        if (disposed || host.island.open || !now) return
        const decision = blogReloadDecision(opened, now)
        if (decision === 'reload') { host.top.location.reload()
}
        else if (decision === 'wait' && tries < RELOAD_POLL_LIMIT) {
          timer = window.setTimeout(settle, RELOAD_POLL_MS, tries + 1)
        }
      })
    }
    const onToggle = () => {
      window.clearTimeout(timer)
      if (host.island.open) {
        remember()
        return
      }
      const href = host.top.location.href
      const clean = withoutStudioParam(href)
      if (clean !== href) host.top.history.replaceState(host.top.history.state, '', clean)
      settle(0)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !event.defaultPrevented) closeStudioIsland(host)
    }
    const onSheet = (event: MouseEvent) => {
      if (event.target === host.sheet) closeStudioIsland(host)
    }
    remember()
    host.island.addEventListener('toggle', onToggle)
    host.sheet?.addEventListener('click', onSheet)
    window.addEventListener('keydown', onKey)
    host.top.addEventListener('keydown', onKey)
    return () => {
      disposed = true
      window.clearTimeout(timer)
      host.island.removeEventListener('toggle', onToggle)
      host.sheet?.removeEventListener('click', onSheet)
      window.removeEventListener('keydown', onKey)
      host.top.removeEventListener('keydown', onKey)
    }
  }, [])
}

/** 创作台岛里的嵌入页 `/embed/studio/*`。 */
export default function Studio() {
  const location = useLocation()
  useStudioIsland()
  const parsed = parseStudioPath(location.pathname)
  return (
    <div className="embed-root studio-embed">
      {!parsed ? (
        <Navigate to={STUDIO_ROOT} replace />
      ) : parsed.kind === 'editor' ? (
        <StudioEditor docId={parsed.docId} />
      ) : (
        <StudioDesk section={parsed.section} />
      )}
      <ToastContainer />
    </div>
  )
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
        onClose={() => navigate(studioSectionPath('notes'))}
        onSaved={() => {}}
        onDeleted={() => navigate(studioSectionPath('notes'))}
        onCloudId={(id) => {
          if (pin !== 'new') return
          ignoreUrl.current = true
          navigate(studioNotePath(id), { replace: true })
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
    <AnimatedView className="studio-page min-h-full px-4 sm:px-6 py-6">
      <StudioShell
        section={section}
        docs={workbench.docs}
        media={workbench.media}
        mediaTotal={workbench.mediaTotal}
        mediaOrigins={workbench.mediaOrigins}
        mediaFormats={workbench.mediaFormats}
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
