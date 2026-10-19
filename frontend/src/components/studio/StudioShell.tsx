import type { MediaAsset, MediaFilter, MediaOrigin } from '../../services/mediaApi'
import type {
  CommentItem,
  PhantasiNoteDoc,
  PhantasiSourceApplication,
} from '../../types/phantasi'
import type { NoteTransferKind } from '../phantasi/logic/board'
import type { WorkbenchCategoryRow } from '../phantasi/logic/categories'
import type { WorkbenchNoteOpen } from '../phantasi/logic/workbench'
import type { StudioTool } from './overlay/studioTools'
import { LuChevronLeft } from '@lib/icons'
import { useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { PhantasiCategoryAdmin } from '../phantasi/manager/PhantasiCategoryAdmin'
import { WorkbenchPage, WorkbenchSearch } from '../phantasi/skin/PhantasiWorkbenchChrome'
import { WorkbenchCommentsPane } from '../phantasi/skin/PhantasiWorkbenchComments'
import { WorkbenchIoPane } from '../phantasi/skin/PhantasiWorkbenchIo'
import { WorkbenchMediaPane } from '../phantasi/skin/PhantasiWorkbenchMedia'
import { WorkbenchNotesPane } from '../phantasi/skin/PhantasiWorkbenchNotes'
import { WorkbenchReviewsPane } from '../phantasi/skin/PhantasiWorkbenchReviews'
import { StudioBlogPane } from './StudioBlogPane'
import { StudioOverview } from './StudioOverview'
import '../ConfigForm.css'
import '../phantasi/ui/css/workbench.css'
import './StudioShell.css'

/** 媒体窗口里按来源筛；原来侧栏上的细分挪到这里。 */
const MEDIA_ORIGINS: Array<'all' | MediaOrigin> = ['all', 'upload', 'generated', 'kept', 'channel', 'import']

export interface StudioToolPaneProps {
  tool: Exclude<StudioTool, 'editor'>
  /** 窗口参数：媒体来源、博客设置停在哪一页。 */
  arg: string | null
  /** 这一页是哪篇文章。 */
  itemId: number | null
  docs: PhantasiNoteDoc[]
  media: MediaAsset[]
  mediaTotal: number
  mediaFormats?: readonly string[]
  mediaFilter: MediaFilter
  onMediaFilter: (filter: MediaFilter) => void
  onMediaOrigin: (origin: 'all' | MediaOrigin) => void
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
  onOpenTool: (tool: StudioTool) => void
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
}

/**
 * 创作台覆盖层里一个工具窗口的内容。窗口自己有标题栏，这里不再放侧栏和返回。
 */
export function StudioToolPane(props: StudioToolPaneProps) {
  const { t } = useI18n()
  const { tool } = props
  const [categoryQuery, setCategoryQuery] = useState('')
  const [openedCategory, setOpenedCategory] = useState<string | null>(null)
  const [onlyThisPost, setOnlyThisPost] = useState(props.itemId != null)

  const originLabel = (origin: 'all' | MediaOrigin): string => {
    switch (origin) {
      case 'all':
        return t.phantasi.workbenchMediaAll
      case 'upload':
        return t.phantasi.workbenchMediaUploadKind
      case 'generated':
        return t.phantasi.workbenchMediaGeneratedKind
      case 'kept':
        return t.phantasi.workbenchMediaKeptKind
      case 'channel':
        return t.phantasi.workbenchMediaChannelKind
      case 'import':
        return t.phantasi.workbenchMediaImportKind
    }
  }

  return (
    <div className="phantasi-workbench studio-tool" data-studio-surface="studio" data-studio-tool={tool}>
      <div className="phantasi-workbench__main">
        {tool === 'overview' ? (
          <StudioOverview
            itemId={props.itemId}
            docs={props.docs}
            comments={props.comments}
            mediaTotal={props.mediaTotal}
            pendingReviews={props.pendingReviews}
            busy={props.busy}
            onWrite={props.onWrite}
            onOpenNote={props.onOpenNote}
            onOpenTool={props.onOpenTool}
          />
        ) : null}
        <WorkbenchNotesPane
          active={tool === 'notes'}
          back={null}
          docs={props.docs}
          notesLoading={props.notesLoading}
          busy={props.busy}
          noteCategories={props.noteCategories}
          onWrite={props.onWrite}
          onOpenNote={props.onOpenNote}
          onDeleteNotes={props.onDeleteNotes}
          onUnschedule={props.onUnschedule}
          onAssignNotes={props.onAssignNotes}
          onOpenCategories={() => props.onOpenTool('categories')}
        />
        <WorkbenchCommentsPane
          active={tool === 'comments'}
          back={null}
          filters={
            props.itemId != null ? (
              <label className="studio-tool__scope">
                <input
                  type="checkbox"
                  checked={onlyThisPost}
                  onChange={(event) => setOnlyThisPost(event.target.checked)}
                />
                <span>{t.studio.overlayOnlyThisPost}</span>
              </label>
            ) : null
          }
          comments={
            onlyThisPost && props.itemId != null
              ? props.comments.filter((comment) => comment.item_id === props.itemId)
              : props.comments
          }
          commentsLoading={props.commentsLoading}
          busy={props.busy}
          onDeleteComments={props.onDeleteComments}
          onOpenCommentItem={props.onOpenCommentItem}
        />
        <WorkbenchReviewsPane
          active={tool === 'reviews'}
          back={null}
          applications={props.applications}
          applicationsLoading={props.applicationsLoading}
          busy={props.busy}
          onApprove={(id) => {
            void props.onApproveApplication(id)
          }}
          onReject={props.onRejectApplication}
          onDelete={props.onDeleteApplications}
        />
        <WorkbenchMediaPane
          active={tool === 'media'}
          title={originLabel(props.mediaFilter.origin)}
          back={null}
          filters={
            <div className="studio-tool__chips" role="group" aria-label={t.phantasi.workbenchMedia}>
        {MEDIA_ORIGINS.map((origin) => (
          <button
            key={origin}
            type="button"
            className="studio-tool__chip"
            aria-pressed={props.mediaFilter.origin === origin}
            onClick={() => {
              props.onMediaOrigin(origin)
              props.onMediaFilter({ ...props.mediaFilter, origin, format: 'all' })
            }}
          >
            {originLabel(origin)}
          </button>
        ))}
      </div>
          }
          media={props.media}
          mediaFormats={props.mediaFormats}
          mediaLoading={props.mediaLoading}
          mediaFilter={props.mediaFilter}
          onMediaFilter={props.onMediaFilter}
          mediaHasMore={props.mediaHasMore}
          onLoadMoreMedia={props.onLoadMoreMedia}
          busy={props.busy}
          onUpload={props.onUpload}
          onDeleteMedia={props.onDeleteMedia}
          onMediaSaved={props.onMediaSaved}
        />
        {tool === 'categories' ? (
          <WorkbenchPage
            title={openedCategory || t.studio.navCategories}
            back={
              openedCategory ? (
                <button
                  type="button"
                  className="section-header-back phantasi-workbench__parent-back"
                  onClick={() => setOpenedCategory(null)}
                  aria-label={t.common.back}
                >
                  <LuChevronLeft size={18} aria-hidden />
                  <span>{t.common.back}</span>
                </button>
              ) : null
            }
            search={
              openedCategory ? undefined : (
                <WorkbenchSearch
                  value={categoryQuery}
                  onChange={setCategoryQuery}
                  placeholder={t.phantasi.workbenchSearchCategories}
                />
              )
            }
          >
            <PhantasiCategoryAdmin
              query={categoryQuery}
              rows={props.categoryRows}
              notes={props.docs}
              openName={openedCategory}
              loading={props.categoriesLoading}
              busy={props.categoriesBusy}
              onOpen={setOpenedCategory}
              onOpenNote={props.onOpenNote}
              onCreate={props.onCreateCategory}
              onRename={props.onRenameCategory}
              onDelete={props.onDeleteCategory}
            />
          </WorkbenchPage>
        ) : null}
        {tool === 'transfer' ? (
          <WorkbenchIoPane
            pane="notesIo"
            back={null}
            docs={props.docs}
            notesBusy={props.notesBusy}
            notesKind={props.notesKind}
            notesProgress={props.notesProgress}
            onExportNotes={props.onExportNotes}
            onImportNotes={props.onImportNotes}
          />
        ) : null}
        {tool === 'settings' ? <StudioBlogPane back={null} arg={props.arg} /> : null}
      </div>
    </div>
  )
}
