import type { ReactNode } from 'react'
import type { MediaAsset } from '../../../services/mediaApi'
import type {
  CommentItem,
  PhantasiNoteDoc,
  PhantasiSource,
  PhantasiSourceApplication,
} from '../../../types/phantasi'
import type { WorkbenchPane } from '../logic/board'
import type { WorkbenchHomeRecent } from '../logic/workbenchHome'
import { getImageUrl } from '../constants'
import { workbenchNoteCover, workbenchNoteListExcerpt, workbenchNoteOpen } from '../logic/workbench'
import {
  workbenchHomeMediaFace,

  workbenchHomeScheduleKind,
} from '../logic/workbenchHome'
import { noteScheduleLabel } from '../notes/noteBoard'
import { displayImageUrl } from '../notes/noteImageUrl'
import { WorkbenchHomeOptions } from './PhantasiWorkbenchHomeOptions'

export function Thumb({
  src,
  video = false,
  cover = false,
}: {
  src?: string
  video?: boolean
  cover?: boolean
}) {
  const className = cover ? 'phantasi-workbench__cover' : 'phantasi-workbench__thumb'
  if (!src) return <span className={`${className} is-empty`} />
  if (video) {
    return (
      <video
        className={className}
        src={src}
        muted
        playsInline
        preload="metadata"
      />
    )
  }
  return <img className={className} src={src} alt="" />
}

function HomeBlock({
  title,
  children,
}: {
  title: string
  children: ReactNode
}) {
  return (
    <section className="phantasi-workbench__home-block">
      <h3>{title}</h3>
      <div className="phantasi-workbench__home-list">{children}</div>
    </section>
  )
}

function homeFaceSrc(raw: string | null | undefined): string | undefined {
  if (!raw?.trim()) return undefined
  return getImageUrl(raw) || displayImageUrl(raw)
}

function HomeRow({
  title,
  meta,
  excerpt,
  src,
  video = false,
  quiet = false,
  onPick,
}: {
  title: string
  meta?: string
  excerpt?: string
  src?: string
  video?: boolean
  quiet?: boolean
  onPick: () => void
}) {
  const face = Boolean(src) && !quiet
  return (
    <button
      type="button"
      className={`phantasi-workbench__home-row${quiet ? ' is-quiet' : ''}${face ? ' has-face' : ''}`}
      onClick={onPick}
    >
      {face ? <Thumb cover src={src} video={video} /> : null}
      <span className="phantasi-workbench__home-row-copy">
        <span className="phantasi-workbench__home-row-title">{title}</span>
        {face && excerpt ? (
          <span className="phantasi-workbench__home-row-excerpt">{excerpt}</span>
        ) : null}
        {meta ? (
          <span className="phantasi-workbench__home-row-meta">{meta}</span>
        ) : null}
      </span>
    </button>
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
  tone?: 'warn' | 'danger'
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

export function WorkbenchHome({
  empty,
  drafts,
  upcoming,
  recent,
  quiet,
  docs,
  media,
  mediaTotal,
  sources,
  comments,
  pendingReviews = [],
  feedCount,
  locale,
  copy,
  onOpenNote,
  onOpen,
}: {
  empty: boolean
  drafts: readonly PhantasiNoteDoc[]
  upcoming: readonly PhantasiNoteDoc[]
  recent: readonly WorkbenchHomeRecent[]
  quiet: { notes: readonly PhantasiNoteDoc[]; sources: readonly PhantasiSource[] }
  docs: readonly PhantasiNoteDoc[]
  media: readonly MediaAsset[]
  mediaTotal: number
  sources: readonly PhantasiSource[]
  comments: readonly CommentItem[]
  pendingReviews?: readonly PhantasiSourceApplication[]
  feedCount: number
  locale: string
  copy: {
    workbenchHomeEmpty: string
    workbenchHomeContinue: string
    workbenchHomeUpcoming: string
    workbenchHomeRecent: string
    workbenchHomeScheduleMissing: string
    workbenchHomeScheduleOverdue: string
    workbenchHomeRecentNote: string
    workbenchHomeRecentSource: string
    workbenchHomeRecentMedia: string
    workbenchHomeNoteFailed: string
    workbenchHomeSourceFailed: string
    workbenchNoteUntitled: string
    workbenchNotes: string
    workbenchComments: string
    workbenchReviews: string
    workbenchHomeReviews: string
    workbenchMedia: string
    workbenchSources: string
    workbenchOverview: string
  }
  onOpenNote: (open: ReturnType<typeof workbenchNoteOpen>) => void
  onOpen: (pane: WorkbenchPane) => void
}) {
  const hasLists =
    pendingReviews.length > 0 ||
    drafts.length > 0 ||
    upcoming.length > 0 ||
    recent.length > 0 ||
    quiet.notes.length > 0 ||
    quiet.sources.length > 0

  return (
    <div className="phantasi-workbench__home">
      <section className="phantasi-workbench__kpis" aria-label={copy.workbenchOverview}>
        <Kpi value={docs.length} label={copy.workbenchNotes} onPick={() => onOpen('notes')} />
        <Kpi value={comments.length} label={copy.workbenchComments} onPick={() => onOpen('comments')} />
        <Kpi
          value={pendingReviews.length}
          label={copy.workbenchReviews}
          tone={pendingReviews.length > 0 ? 'warn' : undefined}
          onPick={() => onOpen('reviews')}
        />
        <Kpi value={mediaTotal} label={copy.workbenchMedia} onPick={() => onOpen('media')} />
        <Kpi value={feedCount} label={copy.workbenchSources} onPick={() => onOpen('sources')} />
      </section>
      <div className="phantasi-workbench__home-body">
        <div className="phantasi-workbench__home-data">
          {hasLists ? (
            <>
              {pendingReviews.length > 0 ? (
                <HomeBlock title={copy.workbenchHomeReviews}>
                  {pendingReviews.map((row) => (
                    <HomeRow
                      key={`review-${row.id}`}
                      title={row.site_name}
                      meta={row.site_url}
                      onPick={() => onOpen('reviews')}
                    />
                  ))}
                </HomeBlock>
              ) : null}
              {drafts.length > 0 ? (
                <HomeBlock title={copy.workbenchHomeContinue}>
                  {drafts.map((doc) => {
                    const src = homeFaceSrc(workbenchNoteCover(doc))
                    return (
                      <HomeRow
                        key={`draft-${doc.id}`}
                        title={doc.title.trim() || copy.workbenchNoteUntitled}
                        excerpt={src ? workbenchNoteListExcerpt(doc) || undefined : undefined}
                        src={src}
                        meta={noteScheduleLabel(doc.updated_at, locale) || undefined}
                        onPick={() => onOpenNote(workbenchNoteOpen(doc))}
                      />
                    )
                  })}
                </HomeBlock>
              ) : null}
              {upcoming.length > 0 ? (
                <HomeBlock title={copy.workbenchHomeUpcoming}>
                  {upcoming.map((doc) => {
                    const kind = workbenchHomeScheduleKind(doc.scheduled_at)
                    const when = noteScheduleLabel(doc.scheduled_at, locale)
                    const meta =
                      kind === 'missing'
                        ? copy.workbenchHomeScheduleMissing
                        : kind === 'overdue' && when
                          ? `${when} · ${copy.workbenchHomeScheduleOverdue}`
                          : kind === 'overdue'
                            ? copy.workbenchHomeScheduleOverdue
                            : when || undefined
                    const src = homeFaceSrc(workbenchNoteCover(doc))
                    return (
                      <HomeRow
                        key={`soon-${doc.id}`}
                        title={doc.title.trim() || copy.workbenchNoteUntitled}
                        excerpt={src ? workbenchNoteListExcerpt(doc) || undefined : undefined}
                        src={src}
                        meta={meta}
                        onPick={() => onOpenNote(workbenchNoteOpen(doc))}
                      />
                    )
                  })}
                </HomeBlock>
              ) : null}
              {recent.length > 0 ? (
                <HomeBlock title={copy.workbenchHomeRecent}>
                  {recent.map((item) => {
                    if (item.kind === 'note') {
                      const doc = docs.find((row) => row.id === item.id)
                      if (!doc) return null
                      const when = noteScheduleLabel(doc.updated_at, locale)
                      const src = homeFaceSrc(workbenchNoteCover(doc))
                      return (
                        <HomeRow
                          key={`recent-note-${doc.id}`}
                          title={doc.title.trim() || copy.workbenchNoteUntitled}
                          excerpt={src ? workbenchNoteListExcerpt(doc) || undefined : undefined}
                          src={src}
                          meta={[copy.workbenchHomeRecentNote, when].filter(Boolean).join(' · ')}
                          onPick={() => onOpenNote(workbenchNoteOpen(doc))}
                        />
                      )
                    }
                    if (item.kind === 'source') {
                      const source = sources.find((row) => row.id === item.id)
                      if (!source) return null
                      const when = noteScheduleLabel(source.created_at, locale)
                      return (
                        <HomeRow
                          key={`recent-source-${source.id}`}
                          title={source.name}
                          meta={[copy.workbenchHomeRecentSource, when].filter(Boolean).join(' · ')}
                          onPick={() => onOpen('sources')}
                        />
                      )
                    }
                    const asset = media.find((row) => row.id === item.id)
                    if (!asset) return null
                    const when = noteScheduleLabel(asset.created_at, locale)
                    const face = workbenchHomeMediaFace(asset)
                    return (
                      <HomeRow
                        key={`recent-media-${asset.id}`}
                        title={asset.name}
                        src={homeFaceSrc(face?.src)}
                        video={face?.video}
                        meta={[copy.workbenchHomeRecentMedia, when].filter(Boolean).join(' · ')}
                        onPick={() => onOpen('media')}
                      />
                    )
                  })}
                </HomeBlock>
              ) : null}
              {quiet.notes.length > 0 || quiet.sources.length > 0 ? (
                <div className="phantasi-workbench__home-quiet">
                  {quiet.notes.map((doc) => (
                    <HomeRow
                      key={`fail-note-${doc.id}`}
                      quiet
                      title={doc.title.trim() || copy.workbenchNoteUntitled}
                      meta={copy.workbenchHomeNoteFailed}
                      onPick={() => onOpenNote(workbenchNoteOpen(doc))}
                    />
                  ))}
                  {quiet.sources.map((source) => (
                    <HomeRow
                      key={`fail-source-${source.id}`}
                      quiet
                      title={source.name}
                      meta={copy.workbenchHomeSourceFailed}
                      onPick={() => onOpen('sources')}
                    />
                  ))}
                </div>
              ) : null}
            </>
          ) : empty ? (
            <p className="phantasi-workbench__home-empty">{copy.workbenchHomeEmpty}</p>
          ) : null}
        </div>
        <WorkbenchHomeOptions />
      </div>
    </div>
  )
}
