import type { ReactNode } from 'react'
import type { PhantasiItemPreview, PhantasiNoteDoc, PhantasiSource } from '../../../types/phantasi'
import type { PhantasiBoard } from '../logic/board'

import type { FeedStory } from '../logic/feedStories'

import type { HomeBoardNote } from '../logic/homeBoard'
import type { PeekStoryPreview } from '../ui/peekLane'
import { useMemo } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import {
  notesBoardIsEmpty,
  visibleCloudNoteDocs,
} from '../notes/noteBoard'
import { PhantasiVacant } from '../ui/Empty'
import PhantasiFeeds from './PhantasiFeeds'
import PhantasiFriends from './PhantasiFriends'
import PhantasiNotes from './PhantasiNotes'
import '../ui/phantasi.css'

interface PhantasiBoardViewProps {
  board: PhantasiBoard
  sources: PhantasiSource[]
  focusSourceId?: number | null
  isEditMode?: boolean
  selectedIds?: Set<number>
  onToggleSelect?: (id: number) => void
  onSourceClick: (source: PhantasiSource) => void
  onOpenItem: (item: PhantasiItemPreview, source: PhantasiSource) => void
  onPeekItem?: (item: PeekStoryPreview) => void
  onPeekEnd?: () => void
  onToggleStar?: (item: PhantasiItemPreview) => void
  onEditSource?: (source: PhantasiSource) => void
  onOpenDoc?: (id: number) => void
  toolbar?: ReactNode
  notesHasMore?: boolean
  onLoadMoreNotes?: () => void
  notesLoading?: boolean
  notesFailed?: boolean
  onRetryNotes?: () => void
  notes?: HomeBoardNote[]
  docs?: PhantasiNoteDoc[]
  vacant?: ReactNode
  stories?: FeedStory[]
  onJumpSource?: (sourceId: number) => void
  onHoldStories?: () => void
  onReleaseStories?: () => void
  railEpoch?: number | string
  onReadySource?: (id: number | null) => void
  onRailFocus?: (sourceId: number | null) => void
  sourceTags?: ReactNode
  noteCategory?: string | null
  topicCards?: string[]
}

export default function PhantasiBoardView({
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
  onOpenDoc,
  toolbar,
  notes = [],
  notesHasMore = false,
  onLoadMoreNotes,
  notesLoading = false,
  notesFailed = false,
  onRetryNotes,
  docs = [],
  vacant,
  stories,
  onJumpSource,
  onHoldStories,
  onReleaseStories,
  railEpoch = 0,
  onReadySource,
  onRailFocus,
  sourceTags,
  noteCategory = null,
  topicCards = [],
}: PhantasiBoardViewProps) {
  const { t } = useI18n()

  const cloudDocs = useMemo(() => board === 'notes' ? visibleCloudNoteDocs(docs) : [], [board, docs])
  const empty =
    board === 'notes'
      ? notesBoardIsEmpty(sources.length, notes.length, docs)
      : sources.length === 0

  if (empty && board !== 'feeds' && !(board === 'notes' && (notesLoading || notesFailed))) {
    if (vacant) return vacant
    return (
      <PhantasiVacant
        layout={board === 'sites' ? 'friends' : 'articles'}
        title={
          board === 'notes'
            ? t.phantasi.emptyNoNotes
            : board === 'sites'
              ? t.phantasi.emptyNoSites
              : t.phantasi.emptyNoSources
        }
        articleTitle={
          board === 'sites' ? t.phantasi.friendArticles : undefined
        }
      />
    )
  }

  if (board === 'feeds') {
    return (
      <PhantasiFeeds
        sources={sources}
        focusSourceId={focusSourceId}
        isEditMode={isEditMode}
        selectedIds={selectedIds}
        onToggleSelect={onToggleSelect}
        onOpenItem={onOpenItem}
        onPeekItem={onPeekItem}
        onPeekEnd={onPeekEnd}
        onToggleStar={onToggleStar}
        onEditSource={onEditSource}
        toolbar={toolbar}
        vacant={vacant}
        stories={stories}
        onJumpSource={onJumpSource}
        onHoldStories={onHoldStories}
        onReleaseStories={onReleaseStories}
        onRailFocus={onRailFocus}
        railEpoch={railEpoch}
        onReadySource={onReadySource}
        sourceTags={sourceTags}
        topicCards={topicCards}
      />
    )
  }

  if (board === 'sites') {
    return (
      <PhantasiFriends
        sources={sources}
        stories={stories ?? []}
        isEditMode={isEditMode}
        selectedIds={selectedIds}
        onToggleSelect={onToggleSelect}
        onOpenItem={onOpenItem}
        onPeekItem={onPeekItem}
        onPeekEnd={onPeekEnd}
        onToggleStar={onToggleStar}
        onEditSource={onEditSource}
      />
    )
  }

  return (
    <PhantasiNotes
      sources={sources}
      notes={notes}
      loading={notesLoading}
      hasMore={notesHasMore}
      onLoadMore={onLoadMoreNotes}
      failed={notesFailed}
      onRetry={onRetryNotes}
      docs={cloudDocs}
      category={noteCategory}
      isEditMode={isEditMode}
      selectedIds={selectedIds}
      onToggleSelect={onToggleSelect}
      onSourceClick={onSourceClick}
      onOpenItem={onOpenItem}
      onPeekItem={onPeekItem}
      onPeekEnd={onPeekEnd}
      onToggleStar={onToggleStar}
      onOpenDoc={onOpenDoc}
    />
  )
}
