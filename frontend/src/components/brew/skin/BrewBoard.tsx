import type { ReactNode } from 'react'
import type { BrewItemPreview, BrewSource } from '../../../types/brew'
import type { BrewBoard } from '../logic/board'

import type { FeedStory } from '../logic/feedStories'

import type { HomeBoardNote } from '../logic/homeBoard'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../../contexts/I18nContext'
import { brewOwnItemPath, getIconUrl, getImageUrl } from '../constants'
import { isSiteSource } from '../logic/board'
import {
  BrewEmpty,
  BrewEmptyAction,
  BrewEmptyRow,
} from '../ui/Empty'
import { BrewPick } from '../ui/Pick'
import {
  SalonCard,
  SalonEdit,
  SalonGrid,
  SalonHit,
  SalonNote,
  SiteMark,
} from '../ui/SiteCard'
import BrewFeeds from './BrewFeeds'
import { brewRelativeTime, useBrewTimes } from './time'
import '../ui/brew.css'

export interface BrewBoardViewProps {
  board: BrewBoard
  sources: BrewSource[]
  focusSourceId?: number | null
  isEditMode?: boolean
  selectedIds?: Set<number>
  onToggleSelect?: (id: number) => void
  onSourceClick: (source: BrewSource) => void
  onOpenItem?: (item: BrewItemPreview, source: BrewSource) => void
  onPeekItem?: (item: BrewItemPreview) => void
  onPeekEnd?: () => void
  onToggleStar?: (item: BrewItemPreview) => void
  onEditSource?: (source: BrewSource) => void
  onWriteNote?: () => void
  onSitesOpenChange?: (open: boolean) => void
  toolbar?: ReactNode
  notes?: HomeBoardNote[]
  vacant?: ReactNode
  stories?: FeedStory[]
  onReadySource?: (id: number | null) => void
}

function openLink(source: BrewSource) {
  const href = source.site_url || source.url
  if (!href) return
  window.open(href, '_blank', 'noopener,noreferrer')
}

export default function BrewBoardView({
  board,
  sources,
  focusSourceId,
  isEditMode = false,
  selectedIds,
  onToggleSelect,
  onSourceClick,
  onOpenItem,
  onPeekItem,
  onPeekEnd,
  onToggleStar,
  onEditSource,
  onWriteNote,
  onSitesOpenChange,
  toolbar,
  notes = [],
  vacant,
  stories,
  onReadySource,
}: BrewBoardViewProps) {
  const { t, locale } = useI18n()
  const navigate = useNavigate()
  const times = useBrewTimes()

  const activateSource = (source: BrewSource) => {
    if (isEditMode) {
      onToggleSelect?.(source.id)
      return
    }
    if (isSiteSource(source)) {
      openLink(source)
      return
    }
    onSourceClick(source)
  }

  const empty =
    sources.length === 0 && (board !== 'notes' || notes.length === 0)

  if (empty && board !== 'feeds') {
    return (
      <BrewEmpty>
        <BrewEmptyRow>
          <span>
            {board === 'notes'
              ? t.brew.emptyNoNotes
              : board === 'sites'
                ? t.brew.emptyNoSites
                : t.brew.emptyNoSources}
          </span>
          {board === 'notes' && onWriteNote ? (
            <BrewEmptyAction onClick={onWriteNote}>
              {t.brew.noteWrite}
            </BrewEmptyAction>
          ) : null}
        </BrewEmptyRow>
      </BrewEmpty>
    )
  }

  if (board === 'feeds') {
    return (
      <BrewFeeds
        sources={sources}
        focusSourceId={focusSourceId}
        isEditMode={isEditMode}
        selectedIds={selectedIds}
        onToggleSelect={onToggleSelect}
        onSourceClick={onSourceClick}
        onOpenItem={onOpenItem}
        onPeekItem={onPeekItem}
        onPeekEnd={onPeekEnd}
        onToggleStar={onToggleStar}
        onEditSource={onEditSource}
        onSitesOpenChange={onSitesOpenChange}
        toolbar={toolbar}
        vacant={vacant}
        stories={stories}
        onReadySource={onReadySource}
      />
    )
  }

  return (
    <SalonGrid>
      {board === 'notes'
        ? notes.map((note, index) => (
            <SalonNote
              key={`note:${note.id}`}
              cardKey={`note:${note.id}`}
              arrive={index < 8 ? index : undefined}
              cover={getImageUrl(note.image)}
              kicker={
                brewRelativeTime(note.published_at, times, locale) ||
                t.brew.boardNotes
              }
              title={note.title}
              summary={note.summary}
              onClick={() => {
                const source = sources.find((s) => s.id === note.source_id)
                const preview: BrewItemPreview = {
                  id: note.id,
                  title: note.title,
                  summary: note.summary,
                  image: note.image,
                  published_at: note.published_at,
                  is_read: true,
                }
                if (source && onOpenItem) onOpenItem(preview, source)
                else navigate(brewOwnItemPath(note.id))
              }}
            />
          ))
        : null}
      {sources.map((source, index) => {
        const at = (board === 'notes' ? notes.length : 0) + index
        return (
          <SalonCard
            key={source.id}
            cardKey={`site:${source.id}`}
            arrive={at < 8 ? at : undefined}
            editing={isEditMode}
            picked={selectedIds?.has(source.id)}
            onClick={() => activateSource(source)}
          >
            {isEditMode ? (
              <BrewPick on={selectedIds?.has(source.id) ?? false} />
            ) : null}
            {isEditMode && onEditSource ? (
              <SalonEdit
                label={t.brew.editSource}
                onClick={() => onEditSource(source)}
              />
            ) : null}
            <SalonHit
              pressed={isEditMode ? selectedIds?.has(source.id) : undefined}
              title={source.name}
              mark={
                <SiteMark name={source.name} icon={getIconUrl(source.icon)} />
              }
              summary={
                source.description ||
                source.recent_items?.[0]?.title ||
                undefined
              }
              onClick={() => activateSource(source)}
            />
          </SalonCard>
        )
      })}
    </SalonGrid>
  )
}
