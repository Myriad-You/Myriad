/** 工作台只在这一路挂数据。订阅墙不建这些 hook。 */

import type { PhantasiItem } from '../../types/phantasi'
import type { WorkbenchPane } from './logic/board'
import type { ArticleLoader, OpenArticleOptions } from './useArticleOpen'
import type { usePhantasiNotes } from './usePhantasiNotes'
import type { usePhantasiSources } from './usePhantasiSources'
import { useI18n } from '../../contexts/I18nContext'
import * as phantasiApi from '../../services/phantasiApi'
import { isSiteSource, refreshableSourceCount } from './logic/board'
import { PhantasiCategoryAdmin } from './manager/PhantasiCategoryAdmin'
import { PhantasiWorkbenchAdmin } from './manager/PhantasiWorkbenchAdmin'
import { useNoteTransfer } from './manager/useNoteTransfer'
import { usePipack } from './manager/usePipack'
import PhantasiWorkbench from './skin/PhantasiWorkbench'
import { usePhantasiCategories } from './usePhantasiCategories'
import { usePhantasiWorkbench } from './usePhantasiWorkbench'

export default function PhantasiWorkbenchLane({
  sources,
  notes,
  openArticle,
  pane,
  onPane,
  setError,
}: {
  sources: ReturnType<typeof usePhantasiSources>
  notes: Pick<
    ReturnType<typeof usePhantasiNotes>,
    'docsEpoch' | 'touchDocs' | 'write' | 'edit' | 'editDoc'
  >
  openArticle: (
    target: PhantasiItem | ArticleLoader,
    options?: OpenArticleOptions,
  ) => Promise<PhantasiItem | void>
  pane: WorkbenchPane
  onPane: (next: WorkbenchPane) => void
  setError: (message: string) => void
}) {
  const { t } = useI18n()
  const pack = usePipack(sources.sources, sources.reloadBoard)
  const workbench = usePhantasiWorkbench(
    pane,
    notes.docsEpoch,
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
  )
  const categories = usePhantasiCategories(
    workbench.needsCategories,
    workbench.docs,
    sources.sources,
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
    sources.updateSource,
    () => {
      notes.touchDocs()
      void workbench.reloadNotes()
    },
  )
  const notesIo = useNoteTransfer(workbench.docs, () => {
    notes.touchDocs()
    void workbench.reloadNotes()
    sources.reloadBoard()
  })

  return (
    <PhantasiWorkbench
      pane={pane}
      onPane={onPane}
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
      sourceCount={sources.sources.length}
      sources={sources.sources}
      packBusy={pack.loading}
      packProgress={
        pack.progress
          ? `${pack.progress.step}${
              pack.progress.total > 0
                ? ` ${pack.progress.current}/${pack.progress.total}`
                : ''
            }`
          : null
      }
      onWrite={notes.write}
      onOpenNote={(open) => {
        if (open.kind === 'item') notes.edit(open.id)
        else notes.editDoc(open.id)
      }}
      onDeleteNotes={async (docs) => {
        const ok = await workbench.removeNotes(docs)
        sources.reloadBoard()
        return ok
      }}
      onDeleteComments={(ids) => {
        void workbench.removeComments(ids)
      }}
      onOpenCommentItem={(id) => {
        void openArticle((signal) =>
          phantasiApi.getItem(id, undefined, { signal }),
        )
      }}
      onApproveApplication={async (id) => {
        const ok = await workbench.approveApplication(id)
        if (ok) sources.reloadBoard()
        return ok
      }}
      onRejectApplication={(id) => {
        void workbench.rejectApplication(id)
      }}
      onDeleteApplications={(ids) => {
        void workbench.removeApplications(ids)
      }}
      onUnschedule={workbench.unschedule}
      onUpload={workbench.upload}
      onMediaSaved={workbench.acceptMedia}
      onDeleteMedia={workbench.removeMedia}
      onExportPack={() => void pack.exportPack()}
      onImportPack={(file) => void pack.importFromFile(file)}
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
      onExportNotes={(kind) => void notesIo.exportKind(kind)}
      onImportNotes={(kind, file) => void notesIo.importKind(kind, file)}
      canRefreshSources={refreshableSourceCount(sources.sources) > 0}
      onRefreshSources={() =>
        sources.refreshSources(
          sources.sources
            .filter((source) => !isSiteSource(source))
            .map((source) => source.id),
        )
      }
      noteCategories={categories.noteRows.map((row) => row.name)}
      onAssignNotes={(docs, name) => {
        void categories.assign(
          'notes',
          docs.map((doc) => doc.id),
          name,
        )
      }}
      admin={
        pane === 'noteCategories' || pane === 'sourceCategories' ? (
          <PhantasiCategoryAdmin
            page={pane === 'noteCategories' ? 'notes' : 'sources'}
            rows={
              pane === 'noteCategories'
                ? categories.noteRows
                : categories.sourceRows
            }
            sources={sources.sources}
            notes={workbench.docs}
            loading={categories.loading}
            busy={categories.busy}
            onOpenNote={(open) => {
              if (open.kind === 'item') notes.edit(open.id)
              else notes.editDoc(open.id)
            }}
            onCreate={categories.create}
            onRename={(from, to) =>
              categories.rename(
                from,
                to,
                pane === 'noteCategories' ? 'notes' : 'sources',
              )
            }
            onDelete={(name) =>
              categories.remove(
                name,
                pane === 'noteCategories' ? 'notes' : 'sources',
              )
            }
          />
        ) : pane === 'sources' ||
          pane === 'add' ||
          pane === 'topics' ||
          pane === 'rsshub' ||
          pane === 'feedsIo' ? (
          <PhantasiWorkbenchAdmin
            pane={pane}
            extraCategories={categories.names}
            onAssignSources={(ids, name) => {
              void categories.assign('sources', ids, name)
            }}
            sources={sources.sources}
            onAddSource={sources.addSource}
            onDiscover={sources.discoverSource}
            onImportOpml={sources.importOpml}
            onExportOpml={pack.exportOpml}
            onUpdateSource={sources.updateSource}
            onRemoveSources={sources.removeSources}
            onRefreshSource={sources.refreshSource}
            onGenerateStyleTags={sources.generateStyleTags}
            onOpenItem={(id) => {
              void openArticle((signal) =>
                phantasiApi.getItem(id, undefined, { signal }),
              )
            }}
          />
        ) : null
      }
    />
  )
}
