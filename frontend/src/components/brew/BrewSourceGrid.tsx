import type {
  AddSourceInput,
  BrewItemPreview,
  BrewSource,
  UpdateSourceRequest,
} from '../../types/brew'
import type { BrewBoard, SourceSortMode } from './logic/board'

import { LuSearch as Search } from '@lib/icons'
import { useCallback, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { roleFromAuth } from './logic/score'
import BrewControls from './manager/BrewControls'
import BrewBoardView from './skin/BrewBoard'
import { BrewViewLane } from './skin/BrewChip'
import { BrewEmpty, BrewEmptyHint } from './ui/Empty'
import { useBoardEdit } from './useBoardEdit'
import {
  useBoardCatalog,
  useBoardNotes,
  useFeedStories,
} from './useBoardPage'

interface BrewSourceGridProps {
  sources: BrewSource[]
  board: BrewBoard
  focusSourceId?: number | null
  onSourceClick: (source: BrewSource) => void
  onRefreshSource: (sourceId: number) => void
  onSourcesChange?: () => void
  onAddSource?: (input: AddSourceInput) => Promise<void>
  onUpdateSource?: (id: number, data: UpdateSourceRequest) => Promise<void>
  onDiscoverSource?: (url: string) => Promise<{
    url: string
    title: string
    feed_type: string
    autocompleted: boolean
  } | null>
  onGenerateStyleTags?: (
    sourceId: number,
    signal?: AbortSignal,
  ) => Promise<{ success: boolean; tags?: string[] }>
  onImportOpml?: (
    content: string,
    signal?: AbortSignal,
  ) => Promise<{ imported: number; skipped: number }>
  onRemoveSources?: (ids: number[]) => Promise<void>
  onOpenItem?: (item: BrewItemPreview, source: BrewSource) => void
  onPeekItem?: (item: BrewItemPreview) => void
  onPeekEnd?: () => void
  onToggleStar?: (item: BrewItemPreview) => void
  onOpenStarred?: () => void
  onWriteNote?: () => void
  onMarkAllRead?: () => void
  isAuthenticated?: boolean
  isAdmin?: boolean
  onBoardSurface?: (board: BrewBoard) => void
}

export default function BrewSourceGrid({
  sources,
  board,
  focusSourceId,
  onSourceClick,
  onRefreshSource,
  onSourcesChange,
  onAddSource,
  onUpdateSource,
  onDiscoverSource,
  onGenerateStyleTags,
  onImportOpml,
  onRemoveSources,
  onOpenItem,
  onPeekItem,
  onPeekEnd,
  onToggleStar,
  onOpenStarred,
  onWriteNote,
  onMarkAllRead,
  isAuthenticated = false,
  isAdmin = false,
  onBoardSurface,
}: BrewSourceGridProps) {
  const { t } = useI18n()
  const viewerRole = roleFromAuth(isAuthenticated, isAdmin)
  const [searchQuery, setSearchQuery] = useState('')
  const [sortMode, setSortMode] = useState<SourceSortMode>('smart')
  const [scoreNow, setScoreNow] = useState(() => Date.now())
  const [readySourceId, setReadySourceId] = useState<number | null>(null)

  const { categories, filtered, sorted } = useBoardCatalog(
    sources,
    board,
    searchQuery,
    sortMode,
    viewerRole,
    scoreNow,
  )
  const notes = useBoardNotes(board, sources)
  const readySource = useMemo(
    () => sources.find((source) => source.id === readySourceId) ?? null,
    [sources, readySourceId],
  )
  const { stories, onStar } = useFeedStories(board, readySource, onToggleStar)
  const edit = useBoardEdit(
    board,
    filtered,
    onRemoveSources,
    onRefreshSource,
    t.errors.brewSourceDeleteFailed,
    t.errors.brewRefreshFailed,
  )

  const handleSortModeChange = useCallback((mode: SourceSortMode) => {
    setSortMode(mode)
    setScoreNow(Date.now())
  }, [])
  const handleReadySource = useCallback((id: number | null) => {
    setReadySourceId(id)
  }, [])

  const bar = (
    <BrewControls
      ref={edit.barRef}
      sources={sources}
      filteredSources={sorted}
      categories={categories}
      searchQuery={searchQuery}
      setSearchQuery={setSearchQuery}
      selectedIds={edit.selectedIds}
      onSelectAll={edit.handleSelectAll}
      onBatchDelete={edit.handleBatchDelete}
      onBatchRefresh={edit.handleBatchRefresh}
      onMarkAllSourcesRead={onMarkAllRead}
      onEnterEditMode={edit.handleEnterEditMode}
      onExitEditMode={edit.handleExitEditMode}
      onWaveDisplayed={edit.handleWaveDisplayed}
      isEditMode={edit.isEditMode}
      isDeleting={edit.isDeleting}
      isRefreshing={edit.isRefreshing}
      onAddSource={onAddSource}
      onUpdateSource={onUpdateSource}
      onDiscover={onDiscoverSource}
      onGenerateStyleTags={onGenerateStyleTags}
      onImportOpml={onImportOpml}
      onSourcesChange={onSourcesChange}
      sortMode={sortMode}
      onSortModeChange={handleSortModeChange}
      isAdmin={isAdmin}
      isAuthenticated={isAuthenticated}
      onOpenStarred={onOpenStarred}
      onWriteNote={board === 'notes' ? onWriteNote : undefined}
      embedded={board === 'feeds'}
      canEdit={board !== 'feeds' || edit.sitesOpen}
    />
  )

  const searchMiss = filtered.length === 0 && !!searchQuery.trim()
  const miss = useMemo(
    () => (
      <BrewEmpty cardKey="search-empty">
        <Search className="brew-empty__mark" aria-hidden />
        <p>{t.brew.noMatchingSources}</p>
        <BrewEmptyHint>{t.brew.tryOtherKeywords}</BrewEmptyHint>
      </BrewEmpty>
    ),
    [t.brew.noMatchingSources, t.brew.tryOtherKeywords],
  )

  const boardView = (
    <BrewBoardView
      board={board}
      sources={sorted}
      focusSourceId={board === 'feeds' ? focusSourceId : undefined}
      isEditMode={edit.boardEdit}
      selectedIds={edit.selectedIds}
      onToggleSelect={edit.handleToggleSelect}
      onSourceClick={onSourceClick}
      onOpenItem={onOpenItem}
      onPeekItem={onPeekItem}
      onPeekEnd={onPeekEnd}
      onToggleStar={board === 'feeds' ? onStar : onToggleStar}
      onEditSource={
        board === 'feeds' && isAdmin ? edit.handleOpenSourceEdit : undefined
      }
      onWriteNote={onWriteNote}
      onSitesOpenChange={board === 'feeds' ? edit.setSitesOpen : undefined}
      toolbar={board === 'feeds' ? bar : undefined}
      vacant={board === 'feeds' && searchMiss ? miss : null}
      stories={board === 'feeds' ? stories : undefined}
      onReadySource={board === 'feeds' ? handleReadySource : undefined}
      notes={notes}
    />
  )

  return (
    <BrewViewLane
      wave={board}
      onDisplayed={(next) => onBoardSurface?.(next as BrewBoard)}
      className="relative min-h-0 flex-1 overflow-visible"
    >
      {board !== 'feeds' ? bar : null}
      {board === 'feeds' ? (
        <div className="flex min-h-0 flex-1 flex-col">{boardView}</div>
      ) : searchMiss ? (
        miss
      ) : (
        boardView
      )}
    </BrewViewLane>
  )
}
