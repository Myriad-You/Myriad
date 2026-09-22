/** 管理员工作台皮。不进口 phantasiApi / manager。侧栏一页一项，分类标题用 SettingSection。 */

import type { ReactNode } from 'react'
import type { MediaAsset, MediaFilter } from '../../../services/mediaApi'
import type {
  CommentItem,
  PhantasiNoteDoc,
  PhantasiSource,
  PhantasiSourceApplication,
} from '../../../types/phantasi'
import type { NoteTransferKind, WorkbenchPane } from '../logic/board'
import type { workbenchNoteOpen } from '../logic/workbench'
import {
  LuChevronLeft,
  LuChevronRight,
  LuNotebookPen,
  LuPlus,
} from '@lib/icons'
import { useMemo, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { usePhantasiGuides } from '../guides/usePhantasiGuides'
import {
  workbenchFeedSourceCount,
  workbenchHomeDrafts,
  workbenchHomeIsEmpty,
  workbenchHomeQuietFails,
  workbenchHomeRecent,
  workbenchHomeUpcoming,
  workbenchPendingReviews,
} from '../logic/workbenchHome'
import { PhantasiWorkbenchIcon } from '../ui/PhantasiWorkbenchIcon'
import { PageAction, WorkbenchPage } from './PhantasiWorkbenchChrome'
import { WorkbenchCommentsPane } from './PhantasiWorkbenchComments'
import { WorkbenchFeedsPanes } from './PhantasiWorkbenchFeeds'
import { WorkbenchHome } from './PhantasiWorkbenchHome'
import { WorkbenchIoPane } from './PhantasiWorkbenchIo'
import { WorkbenchMediaPane } from './PhantasiWorkbenchMedia'
import { WorkbenchNotesPane } from './PhantasiWorkbenchNotes'
import { WorkbenchReviewsPane } from './PhantasiWorkbenchReviews'
import '../../ConfigForm.css'
import '../ui/css/workbench.css'

const RAIL: Array<{
  pane: WorkbenchPane
  label:
    | 'workbenchOverview'
    | 'workbenchNotes'
    | 'workbenchComments'
    | 'workbenchReviews'
    | 'workbenchMedia'
    | 'workbenchSources'
    | 'workbenchRsshub'
    | 'workbenchNavTransfer'
  icon: ReactNode
  pack: 'content' | 'feeds'
}> = [
  {
    pane: 'home',
    label: 'workbenchOverview',
    icon: <PhantasiWorkbenchIcon kind="overview" />,
    pack: 'content',
  },
  {
    pane: 'notes',
    label: 'workbenchNotes',
    icon: <PhantasiWorkbenchIcon kind="notes" />,
    pack: 'content',
  },
  {
    pane: 'comments',
    label: 'workbenchComments',
    icon: <PhantasiWorkbenchIcon kind="comments" />,
    pack: 'content',
  },
  {
    pane: 'media',
    label: 'workbenchMedia',
    icon: <PhantasiWorkbenchIcon kind="media" />,
    pack: 'content',
  },
  {
    pane: 'notesIo',
    label: 'workbenchNavTransfer',
    icon: <PhantasiWorkbenchIcon kind="notes-transfer" />,
    pack: 'content',
  },
  {
    pane: 'sources',
    label: 'workbenchSources',
    icon: <PhantasiWorkbenchIcon kind="sources" />,
    pack: 'feeds',
  },
  {
    pane: 'reviews',
    label: 'workbenchReviews',
    icon: <PhantasiWorkbenchIcon kind="reviews" />,
    pack: 'feeds',
  },
  {
    pane: 'rsshub',
    label: 'workbenchRsshub',
    icon: <PhantasiWorkbenchIcon kind="rsshub" />,
    pack: 'feeds',
  },
  {
    pane: 'feedsIo',
    label: 'workbenchNavTransfer',
    icon: <PhantasiWorkbenchIcon kind="feeds-transfer" />,
    pack: 'feeds',
  },
]

const RAIL_PACKS: Array<{
  id: 'content' | 'feeds'
  title: 'workbenchNavContent' | 'workbenchSources'
}> = [
  { id: 'content', title: 'workbenchNavContent' },
  { id: 'feeds', title: 'workbenchSources' },
]

function NavBtn({
  label,
  icon,
  current,
  onPick,
}: {
  label: string
  icon: ReactNode
  current: boolean
  onPick: () => void
}) {
  return (
    <button
      type="button"
      className={`config-nav-item${current ? ' is-active' : ''}`}
      aria-current={current ? 'page' : undefined}
      onClick={onPick}
    >
      <span className="config-nav-item-icon" aria-hidden>
        {icon}
      </span>
      <span className="config-nav-item-label">{label}</span>
      <span className="config-nav-item-chevron" aria-hidden>
        <LuChevronRight size={16} />
      </span>
    </button>
  )
}

export default function PhantasiWorkbench({
  pane,
  onPane,
  docs,
  media,
  mediaTotal,
  mediaFilter,
  onMediaFilter,
  mediaHasMore,
  onLoadMoreMedia,
  comments = [],
  applications = [],
  notesLoading,
  mediaLoading,
  commentsLoading = false,
  applicationsLoading = false,
  busy,
  sourceCount,
  sources = [],
  packBusy,
  packProgress,
  onWrite,
  onOpenNote,
  onDeleteNotes,
  onDeleteComments,
  onOpenCommentItem,
  onApproveApplication,
  onRejectApplication,
  onDeleteApplications,
  onUnschedule,
  onUpload,
  onDeleteMedia,
  onMediaSaved,
  onExportPack,
  onImportPack,
  notesBusy,
  notesKind,
  notesProgress,
  onExportNotes,
  onImportNotes,
  canRefreshSources,
  onRefreshSources,
  noteCategories = [],
  onAssignNotes,
  admin,
}: {
  pane: WorkbenchPane
  onPane: (pane: WorkbenchPane) => void
  docs: PhantasiNoteDoc[]
  media: MediaAsset[]
  mediaTotal: number
  mediaFilter: MediaFilter
  onMediaFilter: (filter: MediaFilter) => void
  mediaHasMore: boolean
  onLoadMoreMedia: () => void
  comments?: CommentItem[]
  applications?: PhantasiSourceApplication[]
  notesLoading: boolean
  mediaLoading: boolean
  commentsLoading?: boolean
  applicationsLoading?: boolean
  busy: boolean
  sourceCount: number
  sources?: readonly PhantasiSource[]
  packBusy: boolean
  packProgress: string | null
  onWrite: () => void
  onOpenNote: (open: ReturnType<typeof workbenchNoteOpen>) => void
  onDeleteNotes: (docs: PhantasiNoteDoc[]) => void | Promise<boolean>
  onDeleteComments?: (ids: number[]) => void
  onOpenCommentItem?: (itemId: number) => void
  onApproveApplication?: (id: number) => void | Promise<boolean>
  onRejectApplication?: (id: number) => void
  onDeleteApplications?: (ids: number[]) => void
  onUnschedule: (id: number, revision: number) => void
  onUpload: (file: File) => void
  onDeleteMedia: (id: number) => void
  onMediaSaved?: (item: MediaAsset) => void
  onExportPack: () => void
  onImportPack: (file: File) => void
  notesBusy: boolean
  notesKind: NoteTransferKind | null
  notesProgress: string | null
  onExportNotes: (kind: NoteTransferKind) => void
  onImportNotes: (kind: NoteTransferKind, file: File) => void
  canRefreshSources: boolean
  onRefreshSources: () => void | Promise<unknown>
  noteCategories?: readonly string[]
  onAssignNotes?: (docs: PhantasiNoteDoc[], category: string) => void
  admin?: ReactNode
}) {
  const { t, locale } = useI18n()
  const phantasi = t.phantasi
  const { catalog: g, bindGuide } = usePhantasiGuides()
  const [mobilePane, setMobilePane] = useState<'nav' | 'section'>('section')
  const feedCount = workbenchFeedSourceCount(sources)
  const pendingReviews = useMemo(
    () => workbenchPendingReviews(applications),
    [applications],
  )
  const homeEmpty = workbenchHomeIsEmpty(
    docs.length,
    feedCount,
    mediaTotal,
    pendingReviews.length,
  )
  const homeDrafts = useMemo(() => workbenchHomeDrafts(docs), [docs])
  const homeUpcoming = useMemo(() => workbenchHomeUpcoming(docs), [docs])
  const homeQuiet = useMemo(
    () => workbenchHomeQuietFails(docs, sources),
    [docs, sources],
  )
  const homeRecent = useMemo(
    () =>
      workbenchHomeRecent({
        notes: docs,
        skipNoteIds: new Set([
          ...homeDrafts.map((doc) => doc.id),
          ...homeUpcoming.map((doc) => doc.id),
          ...homeQuiet.notes.map((doc) => doc.id),
        ]),
        sources,
        skipSourceIds: new Set(homeQuiet.sources.map((source) => source.id)),
        media,
      }),
    [docs, homeDrafts, homeQuiet, homeUpcoming, media, sources],
  )

  const open = (next: WorkbenchPane) => {
    onPane(next)
    setMobilePane('section')
  }

  const back = (
    <button
      type="button"
      className="section-header-back"
      onClick={() => setMobilePane('nav')}
      aria-label={t.nav.backToNav}
    >
      <LuChevronLeft size={18} aria-hidden />
      <span>{t.common.back}</span>
    </button>
  )

  const backToSources = (
    <button
      type="button"
      className="section-header-back phantasi-workbench__parent-back"
      onClick={() => open('sources')}
      aria-label={phantasi.workbenchSources}
    >
      <LuChevronLeft size={18} aria-hidden />
      <span>{t.common.back}</span>
    </button>
  )

  const backToNotes = (
    <button
      type="button"
      className="section-header-back phantasi-workbench__parent-back"
      onClick={() => open('notes')}
      aria-label={phantasi.workbenchNotes}
    >
      <LuChevronLeft size={18} aria-hidden />
      <span>{t.common.back}</span>
    </button>
  )

  return (
    <div
      className="phantasi-workbench"
      data-phantasi-surface="workbench"
      data-mobile-pane={mobilePane}
    >
      <aside className="config-sidebar phantasi-workbench__rail">
        <div className="config-sidebar-header">
          <span className="nav-icon" aria-hidden>
            <PhantasiWorkbenchIcon kind="studio" />
          </span>
          <div className="config-sidebar-heading">
            <h3 className="nav-title">{phantasi.boardWorkbench}</h3>
            <p className="nav-subtitle">{phantasi.boardWorkbenchTitle}</p>
          </div>
        </div>
        <nav className="config-sidebar-scroll" aria-label={phantasi.boardWorkbench}>
          {RAIL_PACKS.map((pack) => (
            <div key={pack.id} className="config-nav-group">
              <div className="config-nav-group-title">{phantasi[pack.title]}</div>
              {RAIL.filter((item) => item.pack === pack.id).map((item) => (
                <NavBtn
                  key={`${pack.id}-${item.pane}`}
                  label={phantasi[item.label]}
                  icon={item.icon}
                  current={
                    pane === item.pane ||
                    (pane === 'add' && item.pane === 'sources') ||
                    (pane === 'topics' && item.pane === 'sources') ||
                    (pane === 'noteCategories' && item.pane === 'notes') ||
                    (pane === 'sourceCategories' && item.pane === 'sources')
                  }
                  onPick={() => open(item.pane)}
                />
              ))}
            </div>
          ))}
        </nav>
      </aside>

      <div className="phantasi-workbench__main">
        {pane === 'home' ? (
          <WorkbenchPage
            title={phantasi.workbenchOverview}
            icon={<PhantasiWorkbenchIcon kind="overview" />}
            back={back}
            {...bindGuide('workbench.overview', g.overview)}
            action={
              <>
                <PageAction
                  label={phantasi.noteWrite}
                  description={phantasi.workbenchWriteHint}
                  icon={<LuNotebookPen />}
                  disabled={busy}
                  onPick={onWrite}
                />
                <PageAction
                  label={phantasi.addSubscription}
                  description={phantasi.workbenchAddHint}
                  icon={<LuPlus />}
                  onPick={() => open('add')}
                />
              </>
            }
          >
            <WorkbenchHome
              empty={homeEmpty}
              drafts={homeDrafts}
              upcoming={homeUpcoming}
              recent={homeRecent}
              quiet={homeQuiet}
              docs={docs}
              media={media}
              mediaTotal={mediaTotal}
              sources={sources}
              comments={comments}
              pendingReviews={pendingReviews}
              feedCount={feedCount}
              locale={locale}
              copy={phantasi}
              onOpenNote={onOpenNote}
              onOpen={open}
            />
          </WorkbenchPage>
        ) : null}

        <WorkbenchNotesPane
          active={pane === 'notes'}
          back={back}
          docs={docs}
          notesLoading={notesLoading}
          busy={busy}
          noteCategories={noteCategories}
          {...bindGuide('workbench.notes', g.notes)}
          onWrite={onWrite}
          onOpenNote={onOpenNote}
          onDeleteNotes={onDeleteNotes}
          onUnschedule={onUnschedule}
          onAssignNotes={onAssignNotes}
          onOpenCategories={() => open('noteCategories')}
        />

        <WorkbenchCommentsPane
          active={pane === 'comments'}
          back={back}
          comments={comments}
          commentsLoading={commentsLoading}
          busy={busy}
          {...bindGuide('workbench.comments', g.comments)}
          onDeleteComments={onDeleteComments}
          onOpenCommentItem={onOpenCommentItem}
        />

        <WorkbenchReviewsPane
          active={pane === 'reviews'}
          back={back}
          applications={applications}
          applicationsLoading={applicationsLoading}
          busy={busy}
          {...bindGuide('workbench.reviews', g.reviews)}
          onApprove={onApproveApplication}
          onReject={onRejectApplication}
          onDelete={onDeleteApplications}
        />

        <WorkbenchMediaPane
          active={pane === 'media'}
          back={back}
          media={media}
          mediaLoading={mediaLoading}
          mediaFilter={mediaFilter}
          onMediaFilter={onMediaFilter}
          mediaHasMore={mediaHasMore}
          onLoadMoreMedia={onLoadMoreMedia}
          busy={busy}
          {...bindGuide('workbench.media', g.media)}
          onUpload={onUpload}
          onDeleteMedia={onDeleteMedia}
          onMediaSaved={onMediaSaved}
        />

        <WorkbenchFeedsPanes
          pane={pane}
          back={back}
          backToSources={backToSources}
          backToNotes={backToNotes}
          admin={admin}
          canRefreshSources={canRefreshSources}
          onRefreshSources={onRefreshSources}
          onPane={open}
        />

        <WorkbenchIoPane
          pane={pane}
          back={back}
          docs={docs}
          sourceCount={sourceCount}
          packBusy={packBusy}
          packProgress={packProgress}
          onExportPack={onExportPack}
          onImportPack={onImportPack}
          notesBusy={notesBusy}
          notesKind={notesKind}
          notesProgress={notesProgress}
          onExportNotes={onExportNotes}
          onImportNotes={onImportNotes}
          admin={admin}
        />
      </div>
    </div>
  )
}
