import type {
  PhantasiItemPreview,
  PhantasiNoteDoc,
  PhantasiSource,
  UpdateSourceRequest,
} from '../../types/phantasi'
import type { PhantasiBoard, SourceSortMode } from './logic/board'
import type { PeekStoryPreview } from './ui/peekLane'
import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'

import { useI18n } from '../../contexts/I18nContext'
import { useModuleVisibilityPreferences } from '../../utils/moduleVisibility'
import {
  filterItemsByQuery,
  filterSourcesByQuery,
  haystackMatchesQuery,
  refreshableSourceCount,
  sourcesForBoard,
} from './logic/board'
import { shuffleBySeed, storiesFromSources } from './logic/feedStories'
import { roleFromAuth } from './logic/score'
import { PhantasiSourceTitleTags } from './manager/PhantasiSourceTitleTags'
import { leftoverNoteSources, noteWallSearchSources, visibleCloudNoteDocs } from './notes/noteBoard'
import { loadNoteDocs } from './pageData'
import PhantasiBoardView from './skin/PhantasiBoard'
import { PhantasiViewLane } from './skin/PhantasiChip'
import {
  PhantasiNoteCategoryTitleTags,
  useNoteBoardCategory,
} from './skin/PhantasiNoteCategoryTitleTags'
import { PhantasiVacant } from './ui/Empty'
import { PhantasiPageStage } from './ui/PhantasiPageStage'
import { PhantasiRailTitle } from './ui/PhantasiRailTitle'
import { schedulePhantasiPeekResume } from './ui/StoryCard'
import { useArticleFlags } from './useArticleFlags'
import { useBoardEdit } from './useBoardEdit'
import {
  useBoardCatalog,
  useBoardNotes,
  useFeedStories,
} from './useBoardPage'

interface PhantasiSourceGridProps {
  sources: PhantasiSource[]
  board: PhantasiBoard
  focusSourceId?: number | null
  onRailFocus?: (sourceId: number | null) => void
  onSourceClick: (source: PhantasiSource) => void
  onRefreshSource: (sourceId: number) => void
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
  onRemoveSources?: (ids: number[]) => Promise<void>
  onOpenItem: (item: PhantasiItemPreview, source: PhantasiSource) => void
  onPeekItem?: (item: PeekStoryPreview) => void
  onPeekEnd?: () => void
  onToggleStar?: (item: PhantasiItemPreview) => void
  onWriteNote?: () => void
  onOpenDoc?: (id: number) => void
  onMarkAllRead?: () => void
  docsEpoch?: number
  isAuthenticated?: boolean
  isAdmin?: boolean
  searchQuery?: string
  onSearchHits?: (count: number) => void
}

export default function PhantasiSourceGrid({
  sources,
  board,
  focusSourceId,
  onRailFocus,
  onSourceClick,
  onRefreshSource,
  onUpdateSource,
  onDiscoverSource,
  onGenerateStyleTags,
  onRemoveSources,
  onOpenItem,
  onPeekItem,
  onPeekEnd,
  onToggleStar,
  onWriteNote,
  onOpenDoc,
  onMarkAllRead,
  docsEpoch = 0,
  isAuthenticated = false,
  isAdmin = false,
  searchQuery = '',
  onSearchHits,
}: PhantasiSourceGridProps) {
  const { t } = useI18n()
  const titleId = useId()
  const viewerRole = roleFromAuth(isAuthenticated, isAdmin)
  const { preferences } = useModuleVisibilityPreferences()
  const [sortOverride, setSortMode] = useState<SourceSortMode | null>(null)
  const sortMode = sortOverride ?? preferences.journalSourceSort
  const [scoreNow, setScoreNow] = useState(() => Date.now())
  const { categories, filtered, sorted } = useBoardCatalog(
    sources,
    board,
    searchQuery,
    sortMode,
    viewerRole,
    scoreNow,
  )
  const { notes, loading: notesLoading, failed: notesFailed, retry: retryNotes, hasMore: notesHasMore, loadMore: loadMoreNotes } = useBoardNotes(board, sources, docsEpoch)
  const [docs, setDocs] = useState<PhantasiNoteDoc[]>([])
  useEffect(() => {
    if (board !== 'notes' || !isAdmin) {
      setDocs([])
      return
    }
    const controller = new AbortController()
    void loadNoteDocs(controller.signal)
      .then((next) => {
        if (!controller.signal.aborted) setDocs(next)
      })
      .catch(() => {
        if (!controller.signal.aborted) setDocs([])
      })
    return () => controller.abort()
    // 草稿跟 docsEpoch，不跟笔记墙投影。收藏/已读会换 notes 数组，不能重拉文档。
  }, [board, isAdmin, docsEpoch])
  const cloudDocs = useMemo(
    () => (board === 'notes' ? visibleCloudNoteDocs(docs) : []),
    [board, docs],
  )
  const noteCats = useNoteBoardCategory(
    board === 'notes' ? notes : [],
    board === 'notes' ? cloudDocs : [],
    board === 'notes' ? sorted : [],
    notesLoading || notesFailed,
  )
  const {
    stories,
    onStar,
    jump,
    holdStories,
    releaseStories,
    railEpoch,
    topicCards,
  } = useFeedStories(board, sorted, onToggleStar)
  const flags = useArticleFlags()
  const flagsRevision = flags.getSnapshot()
  const friendSeed = useRef(Math.random())
  const friendPool = useMemo(
    () =>
      board === 'sites'
        ? shuffleBySeed(sourcesForBoard(sources, 'sites'), friendSeed.current)
        : [],
    [board, sources],
  )
  const friendStoriesAll = useMemo(
    () =>
      (board === 'sites'
        ? storiesFromSources(friendPool, friendSeed.current)
        : []
      ).map((story) => flags.project(story)),
    [board, flags, flagsRevision, friendPool],
  )
  const friendStories = useMemo(
    () =>
      board === 'sites' ? filterItemsByQuery(friendStoriesAll, searchQuery) : [],
    [board, friendStoriesAll, searchQuery],
  )
  const friendSources = useMemo(() => {
    if (board !== 'sites') return []
    if (!searchQuery.trim()) return friendPool
    const keep = new Set(friendStories.map((story) => story.source_id))
    const named = new Set(filterSourcesByQuery(friendPool, searchQuery).map((source) => source.id))
    return friendPool.filter((source) => named.has(source.id) || keep.has(source.id))
  }, [board, friendPool, friendStories, searchQuery])
  const edit = useBoardEdit(
    board === 'sites' ? friendSources : filtered,
    onRemoveSources,
    onRefreshSource,
    t.errors.phantasiSourceDeleteFailed,
    t.errors.phantasiRefreshFailed,
  )

  const handleSortModeChange = useCallback((mode: SourceSortMode) => {
    setSortMode(mode)
    setScoreNow(Date.now())
  }, [])
  const searchedNotes = useMemo(
    () => (board === 'notes' ? filterItemsByQuery(notes, searchQuery) : notes),
    [board, notes, searchQuery],
  )
  const searchedDocs = useMemo(
    () =>
      board === 'notes'
        ? cloudDocs.filter((doc) =>
            haystackMatchesQuery(searchQuery, doc.title, doc.excerpt),
          )
        : [],
    [board, cloudDocs, searchQuery],
  )
  const notesSources = useMemo(
    () =>
      board === 'notes'
        ? noteWallSearchSources(
            sorted,
            sources,
            sourcesForBoard(sources, 'notes'),
            searchedNotes,
            searchQuery,
          )
        : sorted,
    [board, searchQuery, searchedNotes, sorted, sources],
  )
  const noteHits = useMemo(() => {
    if (board !== 'notes') return 0
    return (
      searchedNotes.length +
      searchedDocs.length +
      leftoverNoteSources(notesSources, searchedNotes).length
    )
  }, [board, notesSources, searchedDocs.length, searchedNotes])
  useEffect(() => {
    if (!onSearchHits) return
    if (board === 'notes') onSearchHits(noteHits)
    else if (board === 'sites') onSearchHits(friendSources.length)
    else onSearchHits(sorted.length)
  }, [board, friendSources.length, noteHits, onSearchHits, sorted.length])

  const sourceTags = useMemo(
    () => (
      <PhantasiSourceTitleTags
        kind={board === 'feeds' ? 'feeds' : 'salon'}
        showApply={board === 'sites'}
        sortMode={sortMode}
        onSortModeChange={handleSortModeChange}
        isAdmin={isAdmin}
        isAuthenticated={isAuthenticated}
        canEdit
        editing={edit.isEditMode}
        selectedIds={edit.selectedIds}
        sources={board === 'sites' ? friendSources : sorted}
        categories={categories}
        refreshableCount={refreshableSourceCount(
          board === 'sites' ? friendSources : sorted,
        )}
        isDeleting={edit.isDeleting}
        isRefreshing={edit.isRefreshing}
        onEnterEdit={undefined}
        onExitEdit={edit.handleExitEditMode}
        onSelectAll={edit.handleSelectAll}
        onBatchDelete={edit.handleBatchDelete}
        onBatchRefresh={edit.handleBatchRefresh}
        onMarkAllRead={onMarkAllRead}
        onWriteNote={board === 'notes' ? onWriteNote : undefined}
        onUpdateSource={onUpdateSource}
        onDiscover={onDiscoverSource}
        onGenerateStyleTags={onGenerateStyleTags}
      />
    ),
    [
      board,
      sortMode,
      handleSortModeChange,
      isAdmin,
      isAuthenticated,
      edit.isEditMode,
      edit.selectedIds,
      friendSources,
      sorted,
      categories,
      edit.isDeleting,
      edit.isRefreshing,
      edit.handleEnterEditMode,
      edit.handleExitEditMode,
      edit.handleSelectAll,
      edit.handleBatchDelete,
      edit.handleBatchRefresh,
      onMarkAllRead,
      onWriteNote,
      onUpdateSource,
      onDiscoverSource,
      onGenerateStyleTags,
    ],
  )

  const searchMiss =
    !!searchQuery.trim() &&
    (board === 'notes'
      ? noteHits === 0 && !notesLoading && !notesFailed
      : board === 'sites'
        ? friendSources.length === 0 && friendStories.length === 0
        : filtered.length === 0)
  const miss = useMemo(
    () => (
      <PhantasiVacant
        layout={board === 'notes' ? 'articles' : 'friends'}
        title={t.phantasi.noMatchingSources}
        hint={t.phantasi.tryOtherKeywords}
        articleTitle={
          board === 'feeds'
            ? t.phantasi.latestArticles
            : board === 'sites'
              ? t.phantasi.friendArticles
              : undefined
        }
      />
    ),
    [
      board,
      t.phantasi.friendArticles,
      t.phantasi.latestArticles,
      t.phantasi.noMatchingSources,
      t.phantasi.tryOtherKeywords,
    ],
  )

  const boardView = (
    <PhantasiBoardView
      board={board}
      sources={
        board === 'sites' ? friendSources : board === 'notes' ? notesSources : sorted
      }
      focusSourceId={board === 'feeds' ? focusSourceId : undefined}
      isEditMode={edit.boardEdit}
      selectedIds={edit.selectedIds}
      onToggleSelect={edit.handleToggleSelect}
      onSourceClick={onSourceClick}
      onOpenItem={onOpenItem}
      onPeekItem={onPeekItem}
      onPeekEnd={onPeekEnd}
      onToggleStar={board === 'feeds' ? onStar : onToggleStar}
      onEditSource={undefined}
      vacant={searchMiss ? miss : null}
      stories={
        board === 'feeds' ? stories : board === 'sites' ? friendStories : undefined
      }
      onJumpSource={board === 'feeds' ? jump : undefined}
      onRailFocus={board === 'feeds' ? onRailFocus : undefined}
      onHoldStories={board === 'feeds' ? holdStories : undefined}
      onReleaseStories={board === 'feeds' ? releaseStories : undefined}
      railEpoch={board === 'feeds' ? railEpoch : 0}
      sourceTags={board === 'feeds' ? sourceTags : undefined}
      topicCards={board === 'feeds' ? topicCards : undefined}
      notes={searchedNotes}
      notesLoading={notesLoading}
      notesFailed={notesFailed}
      onRetryNotes={retryNotes}
      notesHasMore={notesHasMore}
      onLoadMoreNotes={loadMoreNotes}
      docs={board === 'notes' ? searchedDocs : docs}
      noteCategory={board === 'notes' ? noteCats.filter : null}
      onOpenDoc={onOpenDoc}
    />
  )

  return (
    <PhantasiViewLane
      wave={board}
      className="relative min-h-0 flex-1 overflow-visible"
      onDisplayed={() => {
        if (onPeekItem) schedulePhantasiPeekResume(onPeekItem)
      }}
    >
      {board === 'feeds' ? (
        <div className="flex min-h-0 flex-1 flex-col">{boardView}</div>
      ) : (
        <PhantasiPageStage
          title={
            <PhantasiRailTitle
              id={titleId}
              tags={
                board === 'notes' ? (
                  <PhantasiNoteCategoryTitleTags
                    categories={noteCats.categories}
                    hasUnfiled={noteCats.hasUnfiled}
                    value={noteCats.filter}
                    onChange={noteCats.setFilter}
                  />
                ) : undefined
              }
              action={sourceTags}
              pinned={edit.isEditMode}
            >
              {board === 'notes' ? t.phantasi.boardNotes : t.phantasi.boardSites}
            </PhantasiRailTitle>
          }
        >
          {boardView}
        </PhantasiPageStage>
      )}
    </PhantasiViewLane>
  )
}
