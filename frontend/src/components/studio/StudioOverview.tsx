import type { CommentItem, PhantasiNoteDoc } from '../../types/phantasi'
import type { WorkbenchNoteOpen } from '../phantasi/logic/workbench'
import type { StudioTool } from './overlay/studioTools'
import { LuExternalLink, LuMessageCircle, LuNotebookPen } from '@lib/icons'
import { useI18n } from '../../contexts/I18nContext'
import { noteScheduleLabel } from '../phantasi/notes/noteBoard'

const RECENT_LIMIT = 5

function statusLabel(doc: PhantasiNoteDoc, t: ReturnType<typeof useI18n>['t']): string {
  switch (doc.status) {
    case 'published':
      return t.phantasi.noteStatusPublished
    case 'scheduled':
      return t.phantasi.noteStatusScheduled
    default:
      return t.phantasi.noteStatusDraft
  }
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
        <section className="studio-overview__post">
          <div className="studio-overview__eyebrow">{t.studio.overlayThisPost}</div>
          <h2 className="studio-overview__title">
            {doc?.title.trim() || t.phantasi.workbenchNoteUntitled}
          </h2>
          {doc ? (
            <div className="studio-overview__meta">
              <span>{statusLabel(doc, t)}</span>
              {doc.topic ? <span>{doc.topic}</span> : null}
              <span>{noteScheduleLabel(doc.updated_at, locale)}</span>
              {doc.slug ? <code>/blog/posts/{doc.slug}/</code> : null}
            </div>
          ) : null}
          <div className="studio-overview__actions">
            <button
              type="button"
              className="studio-overview__action is-primary"
              disabled={busy}
              onClick={() => onOpenNote(doc ? { kind: 'doc', id: doc.id } : { kind: 'item', id: itemId })}
            >
              <LuNotebookPen aria-hidden />
              {t.studio.overlayEditPost}
            </button>
            <button type="button" className="studio-overview__action" onClick={() => onOpenTool('comments')}>
              <LuMessageCircle aria-hidden />
              {format(t.studio.overlayPostComments, { count: postComments })}
            </button>
          </div>
        </section>
      ) : (
        <section className="studio-overview__post">
          <div className="studio-overview__eyebrow">{t.studio.overlayThisPage}</div>
          <p className="studio-overview__note">{t.studio.overlayNotAPost}</p>
          <div className="studio-overview__actions">
            <button
              type="button"
              className="studio-overview__action is-primary"
              disabled={busy}
              onClick={onWrite}
            >
              <LuNotebookPen aria-hidden />
              {t.phantasi.noteWrite}
            </button>
            <button type="button" className="studio-overview__action" onClick={onOpenPost}>
              <LuExternalLink aria-hidden />
              {t.studio.openBlog}
            </button>
          </div>
        </section>
      )}

      <section className="phantasi-workbench__kpis studio-overview__kpis" aria-label={t.studio.overlayGroupSiteWide}>
        <Kpi value={docs.length} label={t.phantasi.workbenchNotes} onPick={() => onOpenTool('notes')} />
        <Kpi value={comments.length} label={t.phantasi.workbenchComments} onPick={() => onOpenTool('comments')} />
        <Kpi
          value={pendingReviews}
          label={t.phantasi.workbenchReviews}
          tone={pendingReviews > 0 ? 'warn' : undefined}
          onPick={() => onOpenTool('reviews')}
        />
        <Kpi value={mediaTotal} label={t.phantasi.workbenchMedia} onPick={() => onOpenTool('media')} />
      </section>

      {recent.length > 0 ? (
        <section className="studio-overview__recent">
          <div className="studio-overview__eyebrow">{t.studio.overlayRecent}</div>
          <ul>
            {recent.map((row) => (
              <li key={row.id}>
                <button type="button" onClick={() => onOpenNote({ kind: 'doc', id: row.id })}>
                  <span className="studio-overview__recent-title">
                    {row.title.trim() || t.phantasi.workbenchNoteUntitled}
                  </span>
                  <span className="studio-overview__recent-meta">
                    {statusLabel(row, t)} · {noteScheduleLabel(row.updated_at, locale)}
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
      className={`phantasi-workbench__kpi${tone ? ` is-${tone}` : ''}`}
      onClick={onPick}
    >
      <span className="phantasi-workbench__kpi-value">{value}</span>
      <span className="phantasi-workbench__kpi-label">{label}</span>
    </button>
  )
}
