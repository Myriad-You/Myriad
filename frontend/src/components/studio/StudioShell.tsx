import type { ReactNode } from 'react'
import type { MediaAsset, MediaFilter, MediaOrigin } from '../../services/mediaApi'
import type {
  CommentItem,
  PhantasiNoteDoc,
  PhantasiSourceApplication,
} from '../../types/phantasi'
import type { NoteTransferKind } from '../phantasi/logic/board'
import type { WorkbenchCategoryRow } from '../phantasi/logic/categories'
import type { WorkbenchNoteOpen } from '../phantasi/logic/workbench'
import type { StudioSection } from './studioRoutes'
import {
  LuChevronLeft,
  LuDownload,
  LuHeart,
  LuMessageCircle,
  LuNotebookPen,
  LuSparkles,
  LuTag,
  LuUpload,
} from '@lib/icons'
import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { mediaAssetOrigin } from '../../services/mediaApi'
import { PhantasiCategoryAdmin } from '../phantasi/manager/PhantasiCategoryAdmin'
import { PageAction, WorkbenchPage } from '../phantasi/skin/PhantasiWorkbenchChrome'
import { WorkbenchCommentsPane } from '../phantasi/skin/PhantasiWorkbenchComments'
import { WorkbenchIoPane } from '../phantasi/skin/PhantasiWorkbenchIo'
import { WorkbenchMediaPane } from '../phantasi/skin/PhantasiWorkbenchMedia'
import { WorkbenchNotesPane } from '../phantasi/skin/PhantasiWorkbenchNotes'
import { WorkbenchReviewsPane } from '../phantasi/skin/PhantasiWorkbenchReviews'
import { PhantasiWorkbenchIcon } from '../phantasi/ui/PhantasiWorkbenchIcon'
import { InputItem } from '../settings'
import { StudioBlogPane } from './StudioBlogPane'
import {
  studioMediaOrigin,
  studioSectionPath,
} from './studioRoutes'
import '../ConfigForm.css'
import '../phantasi/ui/css/workbench.css'
import './StudioShell.css'

type RailPack = 'blog' | 'media'

/** 概览不归组；其余按博客、媒体两块分。媒体按来源细分，把 AI 生成与 Agent 收藏分开。 */
const RAIL: Array<{
  section: StudioSection
  label:
    | 'navHome'
    | 'workbenchNotes'
    | 'workbenchComments'
    | 'workbenchReviews'
    | 'workbenchMediaAll'
    | 'workbenchMediaUploadKind'
    | 'workbenchMediaGeneratedKind'
    | 'workbenchMediaKeptKind'
    | 'workbenchMediaChannelKind'
    | 'workbenchMediaImportKind'
    | 'navCategories'
    | 'workbenchNavTransfer'
    | 'blogSettings'
  icon: ReactNode
  pack: RailPack | null
}> = [
  { section: 'home', label: 'navHome', icon: <PhantasiWorkbenchIcon kind="overview" />, pack: null },
  { section: 'notes', label: 'workbenchNotes', icon: <PhantasiWorkbenchIcon kind="notes" />, pack: 'blog' },
  { section: 'comments', label: 'workbenchComments', icon: <PhantasiWorkbenchIcon kind="comments" />, pack: 'blog' },
  { section: 'categories', label: 'navCategories', icon: <LuTag size={18} />, pack: 'blog' },
  { section: 'reviews', label: 'workbenchReviews', icon: <PhantasiWorkbenchIcon kind="reviews" />, pack: 'blog' },
  { section: 'import', label: 'workbenchNavTransfer', icon: <PhantasiWorkbenchIcon kind="notes-transfer" />, pack: 'blog' },
  { section: 'blog', label: 'blogSettings', icon: <PhantasiWorkbenchIcon kind="studio" />, pack: 'blog' },
  { section: 'media', label: 'workbenchMediaAll', icon: <PhantasiWorkbenchIcon kind="media" />, pack: 'media' },
  { section: 'mediaUploads', label: 'workbenchMediaUploadKind', icon: <LuUpload size={18} />, pack: 'media' },
  { section: 'mediaGenerated', label: 'workbenchMediaGeneratedKind', icon: <LuSparkles size={18} />, pack: 'media' },
  { section: 'mediaKept', label: 'workbenchMediaKeptKind', icon: <LuHeart size={18} />, pack: 'media' },
  { section: 'mediaChannel', label: 'workbenchMediaChannelKind', icon: <LuMessageCircle size={18} />, pack: 'media' },
  { section: 'mediaImported', label: 'workbenchMediaImportKind', icon: <LuDownload size={18} />, pack: 'media' },
]

const RAIL_GROUPS: Array<RailPack | null> = [null, 'blog', 'media']

function railLabel(key: (typeof RAIL)[number]['label'], t: ReturnType<typeof useI18n>['t']): string {
  switch (key) {
    case 'navHome':
      return t.studio.navHome
    case 'navCategories':
      return t.studio.navCategories
    case 'blogSettings':
      return t.studio.blogSettings
    case 'workbenchNotes':
      return t.phantasi.workbenchNotes
    case 'workbenchComments':
      return t.phantasi.workbenchComments
    case 'workbenchReviews':
      return t.phantasi.workbenchReviews
    case 'workbenchMediaAll':
      return t.phantasi.workbenchMediaAll
    case 'workbenchMediaUploadKind':
      return t.phantasi.workbenchMediaUploadKind
    case 'workbenchMediaGeneratedKind':
      return t.phantasi.workbenchMediaGeneratedKind
    case 'workbenchMediaKeptKind':
      return t.phantasi.workbenchMediaKeptKind
    case 'workbenchMediaChannelKind':
      return t.phantasi.workbenchMediaChannelKind
    case 'workbenchMediaImportKind':
      return t.phantasi.workbenchMediaImportKind
    case 'workbenchNavTransfer':
      return t.phantasi.workbenchNavTransfer
  }
}

export function StudioShell({
  section,
  docs,
  media,
  mediaTotal,
  mediaOrigins,
  mediaFormats,
  mediaFilter,
  onMediaFilter,
  mediaHasMore,
  onLoadMoreMedia,
  comments,
  applications,
  notesLoading,
  mediaLoading,
  commentsLoading,
  applicationsLoading,
  busy,
  noteCategories,
  categoryRows,
  categoriesLoading,
  categoriesBusy,
  onCreateCategory,
  onRenameCategory,
  onDeleteCategory,
  notesBusy,
  notesKind,
  notesProgress,
  onWrite,
  onOpenNote,
  onDeleteNotes,
  onUnschedule,
  onAssignNotes,
  onDeleteComments,
  onOpenCommentItem,
  onApproveApplication,
  onRejectApplication,
  onDeleteApplications,
  onUpload,
  onDeleteMedia,
  onMediaSaved,
  onExportNotes,
  onImportNotes,
  pendingReviews,
}: {
  section: StudioSection
  docs: PhantasiNoteDoc[]
  media: MediaAsset[]
  mediaTotal: number
  mediaOrigins?: readonly MediaOrigin[]
  mediaFormats?: readonly string[]
  mediaFilter: MediaFilter
  onMediaFilter: (filter: MediaFilter) => void
  mediaHasMore: boolean
  onLoadMoreMedia: () => void
  comments: CommentItem[]
  applications: PhantasiSourceApplication[]
  notesLoading: boolean
  mediaLoading: boolean
  commentsLoading: boolean
  applicationsLoading: boolean
  busy: boolean
  noteCategories: readonly string[]
  categoryRows: WorkbenchCategoryRow[]
  categoriesLoading: boolean
  categoriesBusy: boolean
  onCreateCategory: (name: string) => Promise<boolean>
  onRenameCategory: (from: string, to: string) => Promise<boolean>
  onDeleteCategory: (name: string) => Promise<boolean>
  notesBusy: boolean
  notesKind: NoteTransferKind | null
  notesProgress: string | null
  onWrite: () => void
  onOpenNote: (open: WorkbenchNoteOpen) => void
  onDeleteNotes: (docs: PhantasiNoteDoc[]) => void | Promise<boolean>
  onUnschedule: (id: number, revision: number) => void
  onAssignNotes: (docs: PhantasiNoteDoc[], category: string) => void
  onDeleteComments: (ids: number[]) => void
  onOpenCommentItem: (itemId: number) => void
  onApproveApplication: (id: number) => Promise<boolean>
  onRejectApplication: (id: number) => void
  onDeleteApplications: (ids: number[]) => void
  onUpload: (file: File) => void
  onDeleteMedia: (id: number) => void
  onMediaSaved: (item: MediaAsset) => void
  onExportNotes: (kind: NoteTransferKind) => void
  onImportNotes: (kind: NoteTransferKind, file: File) => void
  pendingReviews: number
}) {
  const { t } = useI18n()
  const navigate = useNavigate()
  const [mobilePane, setMobilePane] = useState<'nav' | 'section'>('section')
  const [categoryQuery, setCategoryQuery] = useState('')
  const [openedCategory, setOpenedCategory] = useState<string | null>(null)
  const presentMediaOrigins = mediaOrigins
    ? new Set(mediaOrigins)
    : mediaFilter.origin === 'all' &&
        mediaFilter.format === 'all' &&
        !mediaFilter.query.trim() &&
        media.length > 0
      ? new Set(media.map(mediaAssetOrigin))
      : null
  const go = (next: StudioSection) => {
    const nextOrigin = studioMediaOrigin(next)
    if (nextOrigin && next !== section) {
      onMediaFilter({ ...mediaFilter, origin: nextOrigin, format: 'all' })
    }
    navigate(studioSectionPath(next))
    setMobilePane('section')
    setOpenedCategory(null)
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
  const backToNotes = (
    <button
      type="button"
      className="section-header-back phantasi-workbench__parent-back"
      onClick={() => go('notes')}
      aria-label={t.phantasi.workbenchNotes}
    >
      <LuChevronLeft size={18} aria-hidden />
      <span>{t.common.back}</span>
    </button>
  )

  return (
    <div className="phantasi-workbench" data-studio-surface="studio" data-mobile-pane={mobilePane}>
      <aside className="config-sidebar phantasi-workbench__rail">
        <div className="config-sidebar-header">
          <span className="nav-icon" aria-hidden>
            <PhantasiWorkbenchIcon kind="studio" />
          </span>
          <div className="config-sidebar-heading">
            <h3 className="nav-title">{t.studio.title}</h3>
            <p className="nav-subtitle">{t.studio.subtitle}</p>
          </div>
        </div>
        <nav className="config-sidebar-scroll" aria-label={t.studio.title}>
          {RAIL_GROUPS.map((pack) => (
            <div key={pack ?? 'top'} className="config-nav-group">
              {pack ? (
                <div className="config-nav-group-title">
                  {pack === 'blog' ? t.studio.blog : t.studio.navGroupMedia}
                </div>
              ) : null}
              {RAIL.filter((item) => {
                if (item.pack !== pack) return false
                const origin = studioMediaOrigin(item.section)
                if (!origin || origin === 'all' || item.section === section) return true
                return presentMediaOrigins ? presentMediaOrigins.has(origin) : true
              }).map((item) => {
                const current = section === item.section
                return (
                  <button
                    key={item.section}
                    type="button"
                    className={`config-nav-item${current ? ' is-active' : ''}`}
                    aria-current={current ? 'page' : undefined}
                    onClick={() => go(item.section)}
                  >
                    <span className="config-nav-item-icon" aria-hidden>{item.icon}</span>
                    <span className="config-nav-item-label">{railLabel(item.label, t)}</span>
                  </button>
                )
              })}
            </div>
          ))}
        </nav>
      </aside>
      <div className="phantasi-workbench__main">
        {section === 'home' ? (
          <WorkbenchPage
            title={t.studio.navHome}
            icon={<PhantasiWorkbenchIcon kind="overview" />}
            back={back}
            action={
              <PageAction
                label={t.phantasi.noteWrite}
                description={t.phantasi.workbenchWriteHint}
                icon={<LuNotebookPen />}
                disabled={busy}
                onPick={onWrite}
              />
            }
          >
            <div className="phantasi-workbench__home">
              <section className="phantasi-workbench__kpis" aria-label={t.studio.navHome}>
                <Kpi value={docs.length} label={t.phantasi.workbenchNotes} onPick={() => go('notes')} />
                <Kpi value={comments.length} label={t.phantasi.workbenchComments} onPick={() => go('comments')} />
                <Kpi
                  value={pendingReviews}
                  label={t.phantasi.workbenchReviews}
                  tone={pendingReviews > 0 ? 'warn' : undefined}
                  onPick={() => go('reviews')}
                />
                <Kpi value={mediaTotal} label={t.phantasi.workbenchMedia} onPick={() => go('media')} />
              </section>
            </div>
          </WorkbenchPage>
        ) : null}
        <WorkbenchNotesPane
          active={section === 'notes'}
          back={back}
          docs={docs}
          notesLoading={notesLoading}
          busy={busy}
          noteCategories={noteCategories}
          onWrite={onWrite}
          onOpenNote={onOpenNote}
          onDeleteNotes={onDeleteNotes}
          onUnschedule={onUnschedule}
          onAssignNotes={onAssignNotes}
          onOpenCategories={() => go('categories')}
        />
        <WorkbenchCommentsPane
          active={section === 'comments'}
          back={back}
          comments={comments}
          commentsLoading={commentsLoading}
          busy={busy}
          onDeleteComments={onDeleteComments}
          onOpenCommentItem={onOpenCommentItem}
        />
        <WorkbenchReviewsPane
          active={section === 'reviews'}
          back={back}
          applications={applications}
          applicationsLoading={applicationsLoading}
          busy={busy}
          onApprove={(id) => { void onApproveApplication(id) }}
          onReject={onRejectApplication}
          onDelete={onDeleteApplications}
        />
        <WorkbenchMediaPane
          active={studioMediaOrigin(section) !== null}
          title={
            section === 'mediaUploads'
              ? t.phantasi.workbenchMediaUploadKind
              : section === 'mediaGenerated'
                ? t.phantasi.workbenchMediaGeneratedKind
                : section === 'mediaKept'
                  ? t.phantasi.workbenchMediaKeptKind
                  : section === 'mediaChannel'
                    ? t.phantasi.workbenchMediaChannelKind
                    : section === 'mediaImported'
                      ? t.phantasi.workbenchMediaImportKind
                      : t.phantasi.workbenchMedia
          }
          back={back}
          media={media}
          mediaFormats={mediaFormats}
          mediaLoading={mediaLoading}
          mediaFilter={mediaFilter}
          onMediaFilter={onMediaFilter}
          mediaHasMore={mediaHasMore}
          onLoadMoreMedia={onLoadMoreMedia}
          busy={busy}
          onUpload={onUpload}
          onDeleteMedia={onDeleteMedia}
          onMediaSaved={onMediaSaved}
        />
        {section === 'categories' ? (
          <WorkbenchPage
            title={openedCategory || t.studio.navCategories}
            icon={<LuTag />}
            back={openedCategory ? (
              <button
                type="button"
                className="section-header-back phantasi-workbench__parent-back"
                onClick={() => setOpenedCategory(null)}
                aria-label={t.common.back}
              >
                <LuChevronLeft size={18} aria-hidden />
                <span>{t.common.back}</span>
              </button>
            ) : backToNotes}
            search={openedCategory ? undefined : (
              <InputItem
                itemKey="studio-category-search"
                label={t.phantasi.workbenchSearchCategories}
                value={categoryQuery}
                onChange={setCategoryQuery}
                placeholder={t.phantasi.workbenchSearchCategories}
                inputType="search"
                size="sm"
                layout="vertical"
                autoComplete="off"
                className="phantasi-workbench__title-search"
              />
            )}
          >
            <PhantasiCategoryAdmin
              query={categoryQuery}
              rows={categoryRows}
              notes={docs}
              openName={openedCategory}
              loading={categoriesLoading}
              busy={categoriesBusy}
              onOpen={setOpenedCategory}
              onOpenNote={onOpenNote}
              onCreate={onCreateCategory}
              onRename={onRenameCategory}
              onDelete={onDeleteCategory}
            />
          </WorkbenchPage>
        ) : null}
        {section === 'import' ? (
          <WorkbenchIoPane
            pane="notesIo"
            back={back}
            docs={docs}
            notesBusy={notesBusy}
            notesKind={notesKind}
            notesProgress={notesProgress}
            onExportNotes={onExportNotes}
            onImportNotes={onImportNotes}
          />
        ) : null}
        {section === 'blog' ? <StudioBlogPane back={back} /> : null}
      </div>
    </div>
  )
}

function Kpi({
  value,
  label,
  tone,
  onPick,
}: {
  value: number
  label: string
  tone?: 'warn'
  onPick: () => void
}) {
  return (
    <button
      type="button"
      className={`phantasi-workbench__kpi${tone ? ` is-${tone}` : ''}`}
      onClick={onPick}
    >
      <span className="phantasi-workbench__kpi-value">{value}</span>
      <span className="phantasi-workbench__kpi-label">{label}</span>
    </button>
  )
}
