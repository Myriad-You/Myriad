import type { WorkbenchPane } from '../components/phantasi/logic/board'
import type { StudioTool } from '../components/studio/overlay/studioTools'
import { lazy, Suspense, useCallback, useState } from 'react'
import { useLocation } from 'react-router-dom'
import { workbenchPendingReviews } from '../components/phantasi/logic/workbenchHome'
import { useNoteTransfer } from '../components/phantasi/manager/useNoteTransfer'
import { usePhantasiCategories } from '../components/phantasi/usePhantasiCategories'
import { usePhantasiWorkbench } from '../components/phantasi/usePhantasiWorkbench'
import { openBlogPage, studioOverlayApi } from '../components/studio/studioIsland'
import { openPublishedItem, openStudioNote, openStudioTool } from '../components/studio/studioNoteNav'
import { editorTarget, mediaOriginForArg, parseStudioToolPath } from '../components/studio/studioRoutes'
import { StudioToolPane } from '../components/studio/StudioShell'
import { ToastContainer } from '../components/ToastContainer'
import { useI18n } from '../contexts/I18nContext'
import { showToast } from '../utils/toastManager'
import '../components/ConfigForm.css'
import '../components/studio/StudioShell.css'

const NoteEditor = lazy(() => import('../components/phantasi/notes/NoteEditor'))

function deskPane(tool: Exclude<StudioTool, 'editor'>): WorkbenchPane {
  switch (tool) {
    case 'overview':
      return 'home'
    case 'notes':
      return 'notes'
    case 'comments':
      return 'comments'
    case 'media':
      return 'media'
    case 'reviews':
      return 'reviews'
    case 'categories':
      return 'noteCategories'
    case 'transfer':
      return 'notesIo'
    case 'settings':
      return 'blog'
  }
}

function positiveInt(value: string | null): number | null {
  const id = Number(value)
  return value && Number.isSafeInteger(id) && id > 0 ? id : null
}

/** 覆盖层里一个工具窗口的嵌入页 `/embed/studio/tool/{tool}`。 */
export default function Studio() {
  const location = useLocation()
  const tool = parseStudioToolPath(location.pathname)
  const query = new URLSearchParams(location.search)
  const arg = query.get('arg')
  const itemId = positiveInt(query.get('item'))
  return (
    <div className="studio-embed">
      {tool === 'editor' ? (
        <StudioEditor target={editorTarget(arg)} />
      ) : tool ? (
        <StudioToolPage key={tool} tool={tool} arg={arg} itemId={itemId} />
      ) : null}
      <ToastContainer />
    </div>
  )
}

function StudioEditor({ target }: { target: number | 'new' }) {
  const [pin] = useState(target)
  const close = () => {
    const api = studioOverlayApi()
    if (api) api.close('editor')
    else openBlogPage('/blog/')
  }
  return (
    <Suspense fallback={null}>
      <NoteEditor
        key={pin === 'new' ? 'new' : String(pin)}
        docId={pin === 'new' ? undefined : pin}
        onClose={close}
        onSaved={() => {}}
        onDeleted={close}
        onCloudId={(id) => {
          if (pin !== 'new') return
          // 新笔记拿到 id：记进布局，窗口不重新加载，编辑器也不重建。
          window.history.replaceState(window.history.state, '', `?arg=${id}`)
          studioOverlayApi()?.remember({ tool: 'editor', arg: String(id) })
        }}
      />
    </Suspense>
  )
}

function StudioToolPage({
  tool,
  arg,
  itemId,
}: {
  tool: Exclude<StudioTool, 'editor'>
  arg: string | null
  itemId: number | null
}) {
  const { t } = useI18n()
  const [docsEpoch, setDocsEpoch] = useState(0)
  const [mediaOrigin, setMediaOrigin] = useState(() => mediaOriginForArg(arg))
  const setError = useCallback((message: string) => {
    if (message) showToast({ message, type: 'error', replaceKey: 'studio' })
  }, [])
  const workbench = usePhantasiWorkbench(
    deskPane(tool),
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
    tool === 'media' ? mediaOrigin : null,
  )
  const categories = usePhantasiCategories(
    tool === 'notes' || tool === 'categories' || tool === 'overview',
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
    <div className="studio-tool-page">
      <StudioToolPane
        tool={tool}
        itemId={itemId}
        docs={workbench.docs}
        media={workbench.media}
        mediaTotal={workbench.mediaTotal}
        mediaFormats={workbench.mediaFormats}
        mediaFilter={{ ...workbench.mediaFilter, origin: mediaOrigin }}
        onMediaFilter={workbench.setMediaFilter}
        onMediaOrigin={setMediaOrigin}
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
          void openStudioNote({ kind: 'new' })
        }}
        onOpenNote={(open) => {
          void openStudioNote(open, setError, failed)
        }}
        onOpenTool={(next) => openStudioTool({ tool: next, arg: null })}
        onOpenPost={() => openBlogPage('/blog/')}
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
    </div>
  )
}
