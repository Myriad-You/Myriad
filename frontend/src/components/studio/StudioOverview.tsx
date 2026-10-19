import type { CommentItem, PhantasiNoteDoc } from '../../types/phantasi'
import type { WorkbenchNoteOpen } from '../phantasi/logic/workbench'
import type { StudioTool } from './overlay/studioTools'
import { LuExternalLink, LuMessageCircle, LuNotebookPen } from '@lib/icons'
import { useI18n } from '../../contexts/I18nContext'
import { getImageUrl } from '../phantasi/constants'
import { workbenchNoteCover } from '../phantasi/logic/workbench'
import { noteScheduleLabel } from '../phantasi/notes/noteBoard'
import { PageAction } from '../phantasi/skin/PhantasiWorkbenchChrome'
import { WorkbenchThumb } from '../phantasi/skin/WorkbenchThumb'

const RECENT_LIMIT = 5

type StatusTone = 'success' | 'warn' | 'muted'

function noteStatus(doc: PhantasiNoteDoc, t: ReturnType<typeof useI18n>['t']): { label: string; tone: StatusTone } {
  switch (doc.status) {
    case 'published':
      return { label: t.phantasi.noteStatusPublished, tone: 'success' }
    case 'scheduled':
      return { label: t.phantasi.noteStatusScheduled, tone: 'warn' }
    default:
      return { label: t.phantasi.noteStatusDraft, tone: 'muted' }
  }
}

function StatusBadge({ doc }: { doc: PhantasiNoteDoc }) {
  const { t } = useI18n()
  const status = noteStatus(doc, t)
  return <span className={`managed-list-badge managed-list-badge--${status.tone}`}>{status.label}</span>
}

/**
 * 「这一页」：覆盖层打开时先看到的窗口。在文章页上讲这篇，在别的页上讲全站，
 * 下面是全站的几个数和最近改过的笔记。
 */
export function StudioOverview({
  itemId,
  docs,
  comments,
  mediaTotal,
  pendingReviews,
  busy,
  onWrite,
  onOpenNote,
  onOpenTool,
  onOpenPost,
}: {
  itemId: number | null
  docs: PhantasiNoteDoc[]
  comments: CommentItem[]
  mediaTotal: number
  pendingReviews: number
  busy: boolean
  onWrite: () => void
  onOpenNote: (open: WorkbenchNoteOpen) => void
  onOpenTool: (tool: StudioTool) => void
  onOpenPost: () => void
}) {
  const { t, locale, format } = useI18n()
  const doc = itemId != null ? docs.find((row) => row.item_id === itemId) : undefined
  const postComments = itemId != null ? comments.filter((row) => row.item_id === itemId).length : 0
  const recent = [...docs].sort((a, b) => b.updated_at - a.updated_at).slice(0, RECENT_LIMIT)

  return (
    <div className="studio-overview">
      {itemId != null ? (
        <section className="studio-overview__post" aria-label={t.studio.overlayThisPost}>
          <div className="studio-overview__eyebrow">{t.studio.overlayThisPost}</div>
          <div className="studio-overview__head">
            <WorkbenchThumb cover src={doc ? getImageUrl(workbenchNoteCover(doc)) || undefined : undefined} />
            <div className="studio-overview__head-text">
              <h2 className="studio-overview__title">
                {doc?.title.trim() || t.phantasi.workbenchNoteUntitled}
              </h2>
              {doc ? (
                <div className="studio-overview__meta">
                  <StatusBadge doc={doc} />
                  {doc.topic ? <span>{doc.topic}</span> : null}
                  <span>{noteScheduleLabel(doc.updated_at, locale)}</span>
                </div>
              ) : null}
            </div>
          </div>
          <div className="studio-overview__actions">
            <PageAction
              primary
              label={t.studio.overlayEditPost}
              icon={<LuNotebookPen />}
              disabled={busy}
              onPick={() => onOpenNote(doc ? { kind: 'doc', id: doc.id } : { kind: 'item', id: itemId })}
            />
            <PageAction
              label={format(t.studio.overlayPostComments, { count: postComments })}
              icon={<LuMessageCircle />}
              onPick={() => onOpenTool('comments')}
            />
          </div>
        </section>
      ) : (
        <section className="studio-overview__post" aria-label={t.studio.overlayThisPage}>
          <div className="studio-overview__eyebrow">{t.studio.overlayThisPage}</div>
          <p className="studio-overview__note">{t.studio.overlayNotAPost}</p>
          <div className="studio-overview__actions">
            <PageAction primary label={t.phantasi.noteWrite} icon={<LuNotebookPen />} disabled={busy} onPick={onWrite} />
            <PageAction label={t.studio.openBlog} icon={<LuExternalLink />} onPick={onOpenPost} />
          </div>
        </section>
      )}

      <section className="studio-overview__section" aria-label={t.studio.overlayGroupSiteWide}>
        <div className="studio-overview__eyebrow">{t.studio.overlayGroupSiteWide}</div>
        <div className="studio-overview__stats">
          <Kpi value={docs.length} label={t.phantasi.workbenchNotes} onPick={() => onOpenTool('notes')} />
          <Kpi value={comments.length} label={t.phantasi.workbenchComments} onPick={() => onOpenTool('comments')} />
          <Kpi
            value={pendingReviews}
            label={t.phantasi.workbenchReviews}
            tone={pendingReviews > 0 ? 'warn' : undefined}
            onPick={() => onOpenTool('reviews')}
          />
          <Kpi value={mediaTotal} label={t.phantasi.workbenchMedia} onPick={() => onOpenTool('media')} />
        </div>
      </section>

      {recent.length > 0 ? (
        <section className="studio-overview__section" aria-label={t.studio.overlayRecent}>
          <div className="studio-overview__eyebrow">{t.studio.overlayRecent}</div>
          <ul className="studio-overview__recent">
            {recent.map((row) => (
              <li key={row.id}>
                <button type="button" onClick={() => onOpenNote({ kind: 'doc', id: row.id })}>
                  <span className="studio-overview__recent-title">
                    {row.title.trim() || t.phantasi.workbenchNoteUntitled}
                  </span>
                  <StatusBadge doc={row} />
                  <span className="studio-overview__recent-meta">
                    {noteScheduleLabel(row.updated_at, locale)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
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
      className={`studio-overview__stat${tone ? ` is-${tone}` : ''}`}
      onClick={onPick}
    >
      <span className="studio-overview__stat-value">{value}</span>
      <span className="studio-overview__stat-label">{label}</span>
    </button>
  )
}
