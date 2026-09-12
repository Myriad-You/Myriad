import type { BrewBoard, BrewViewMode } from '../components/brew/logic/board'

import type { BoardScroll } from '../components/brew/logic/boardScroll'

import type { BrewItem } from '../types/brew'
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { useLocation, useMatch, useNavigate, useSearchParams } from 'react-router-dom'
import {
  cancelArticlePrefetch,
  prefetchArticleDetails,
} from '../components/brew/articlePrefetch'
import { brewBoardNavItems } from '../components/brew/boardNav'
import BrewFilterLane from '../components/brew/BrewFilterLane'
import BrewReader from '../components/brew/BrewReader'
import BrewSourceGrid from '../components/brew/BrewSourceGrid'
import { brewOwnItemPath } from '../components/brew/constants'
import {
  filterLaneItems,
  showsFilterLane,
} from '../components/brew/logic/board'
import {
  captureBoardScroll,
  restoreBoardScroll,
} from '../components/brew/logic/boardScroll'
import { shouldPopOpenedItem } from '../components/brew/logic/brewItemRoute'
import { topicDisplayName } from '../components/brew/logic/topics'
import NoteEditor from '../components/brew/notes/NoteEditor'
import { BrewViewLane } from '../components/brew/skin/BrewChip'
import { AnimatePresence, BrewPage } from '../components/brew/skin/BrewPage'
import { useBrewAgentOpen } from '../components/brew/useBrewAgentOpen'
import { useBrewBoardRoute } from '../components/brew/useBrewBoardRoute'
import { useBrewItemActions } from '../components/brew/useBrewItemActions'
import { useBrewItemRoute } from '../components/brew/useBrewItemRoute'
import { useBrewItems } from '../components/brew/useBrewItems'
import { useBrewNavExpand } from '../components/brew/useBrewNavExpand'
import { useBrewNotes } from '../components/brew/useBrewNotes'
import { useBrewSeo } from '../components/brew/useBrewSeo'
import { useBrewSources } from '../components/brew/useBrewSources'
import { useBrewStarred } from '../components/brew/useBrewStarred'
import { useBrewSurface } from '../components/brew/useBrewSurface'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useSecondaryNav } from '../contexts/NavigationContext'
import { useReadingListOptional } from '../contexts/ReadingListContext'
import { useBrewScheduler } from '../hooks/animation'
import { useBrewKeyboard } from '../hooks/useBrewKeyboard'
import { brewSubject } from '../utils/brewSubject'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../utils/moduleVisibility'

const EMPTY_ITEMS: BrewItem[] = []

export default function Brew() {
  const subject = useSyncExternalStore(
    brewSubject.subscribe,
    brewSubject.getSnapshot,
    brewSubject.getSnapshot,
  )
  const { hasChecked } = useAuth()
  if (!hasChecked || !subject.active) return <BrewPage lock={false} loading />
  return <BrewSubjectPage key={subject.generation} />
}

function BrewSubjectPage() {
  useBrewScheduler()
  const { t } = useI18n()
  const navigate = useNavigate()
  const location = useLocation()
  const itemIdParam = useMatch('/brew/item/:itemId')?.params.itemId
  const [searchParams, setSearchParams] = useSearchParams()
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.brew,
    { isAuthenticated: false, isAdmin: false },
  )
  const { isAuthenticated, isAdmin } = useAuth()
  const readingList = useReadingListOptional()
  const [error, setError] = useState<string | null>(null)

  const sources = useBrewSources(
    isAuthenticated,
    {
      loadFailed: t.brew.loadSourcesFailed,
      refreshFailed: t.errors.brewRefreshFailed,
    },
    setError,
  )

  const navItems = useMemo(() => brewBoardNavItems(t.brew), [t.brew])
  const { activeId, setActiveId, setExpanded } = useSecondaryNav({
    routePath: '/brew',
    items: navItems,
    defaultActiveId: 'feeds',
    expandHint: t.brew.expandMenu,
  })
  useBrewNavExpand(setExpanded)

  const route = useBrewBoardRoute(
    isAuthenticated,
    sources.sources,
    activeId,
    setActiveId,
    searchParams,
    setSearchParams,
  )

  const list = useBrewItems(
    t.brew.loadArticlesFailed,
    setError,
    route.viewMode,
    route.selectedTopic?.key,
  )
  const starred = useBrewStarred(
    list.items,
    list.setItems,
    list.setTotal,
    t.brew.starFailed,
    setError,
  )

  const item = useBrewItemRoute(
    itemIdParam,
    sources.sources,
    sources.sourcesLoaded,
    navigate,
    setError,
    t.brew.loadArticlesFailed,
  )

  useBrewAgentOpen({
    itemsRef: list.itemsRef,
    openArticle: item.openArticle,
    setItems: list.setItems,
    setTotal: list.setTotal,
    setError,
    webSearchLabel: t.brew.webSearch,
    loadFailed: t.brew.loadArticlesFailed,
  })

  const actions = useBrewItemActions({
    isAuthenticated,
    openArticle: item.openArticle,
    viewMode: route.viewMode,
    selectedItem: item.selectedItem,
    setItems: list.setItems,
    setTotal: list.setTotal,
    setError,
    itemsRef: list.itemsRef,
    unselectStarred: starred.unselect,
    navigate,
    readingList,
    labels: {
      starFailed: t.brew.starFailed,
      readingFailed: t.errors.readingStateFailed,
      loadFailed: t.brew.loadArticlesFailed,
      webSearch: t.brew.webSearch,
    },
  })

  const notes = useBrewNotes(
    item.selectedItem,
    item.setSelectedItem,
    list.setItems,
    route.viewMode,
    route.selectedTopic?.key,
    list.loadItems,
    sources.reloadBoard,
    sources.loadSources,
    sources.loadStats,
  )

  useBrewSeo(
    item.selectedItem,
    item.selectedItemSource,
    item.selectedItemOwnState,
    moduleOpenToAll,
    t.nav.brewReading || t.nav.brew,
    t.widgets.brewDesc,
  )

  const handleOpenStarred = useCallback(() => {
    item.closeArticle()
    route.openStarred()
  }, [item.closeArticle, route.openStarred])

  const handleBackFromTopic = useCallback(() => {
    item.closeArticle()
    route.backFromTopic()
  }, [item.closeArticle, route.backFromTopic])

  const handleStarredBack = useCallback(() => {
    route.backToFeeds()
    starred.exitEdit()
  }, [route.backToFeeds, starred.exitEdit])

  const handleCloseReader = useCallback(() => {
    const openedId = item.selectedItem?.id
    item.closeArticle()
    if (!itemIdParam) return
    if (shouldPopOpenedItem(location.state, openedId)) navigate(-1)
    else navigate('/brew', { replace: true })
  }, [
    itemIdParam,
    item.selectedItem?.id,
    item.closeArticle,
    location.state,
    navigate,
  ])

  const boardScroll = useRef<BoardScroll | null>(null)
  const selectedId = item.selectedItem?.id
  useEffect(() => {
    if (selectedId != null) {
      if (!boardScroll.current) boardScroll.current = captureBoardScroll()
      return
    }
    const pos = boardScroll.current
    boardScroll.current = null
    if (!pos) return
    const frame = requestAnimationFrame(() => restoreBoardScroll(pos))
    return () => cancelAnimationFrame(frame)
  }, [selectedId])

  const listItems = filterLaneItems(route.viewMode, list.items, EMPTY_ITEMS)

  const topicFeedMode = useMemo(() => {
    if (!route.selectedTopic) return undefined
    return {
      topicKey: route.selectedTopic.key,
      topicLabel: topicDisplayName(route.selectedTopic, t.brew),
      total: list.total,
      onBack: handleBackFromTopic,
    }
  }, [route.selectedTopic, list.total, handleBackFromTopic, t.brew])

  const starredMode = useMemo(
    () => ({
      total: sources.stats?.total_starred || 0,
      selectedIds: starred.selectedIds,
      isEditMode: starred.editMode,
      onBack: handleStarredBack,
      onEnterEditMode: starred.enterEdit,
      onExitEditMode: starred.exitEdit,
      onSelectAll: starred.selectAll,
      onBatchUnstar: starred.batchUnstar,
      isProcessing: starred.processing,
    }),
    [
      sources.stats?.total_starred,
      starred.selectedIds,
      starred.editMode,
      handleStarredBack,
      starred.enterEdit,
      starred.exitEdit,
      starred.selectAll,
      starred.batchUnstar,
      starred.processing,
    ],
  )

  const handleCardStar = useCallback(
    (preview: { id: number; is_starred?: boolean }) => {
      return actions.toggleStar({
        id: preview.id,
        is_starred: !!preview.is_starred,
      })
    },
    [actions.toggleStar],
  )

  const handleKeyboardSelect = useCallback(
    (next: BrewItem | null) => {
      if (next) void actions.select(next)
      else handleCloseReader()
    },
    [actions.select, handleCloseReader],
  )

  const handleReaderStar = useCallback(() => {
    if (item.selectedItem) actions.toggleStar(item.selectedItem)
  }, [item.selectedItem, actions.toggleStar])

  useBrewKeyboard({
    items: listItems,
    selectedItem: item.selectedItem,
    enabled: true,
    onSelectItem: handleKeyboardSelect,
    onToggleRead: actions.toggleRead,
    onToggleStar: actions.toggleStar,
    onRefresh: () => {},
    onAddSource: () => {},
    onMarkAllRead: actions.markAllRead,
    onCloseReader: handleCloseReader,
    onShowHelp: () => {},
  })

  const [surfaceView, setSurfaceView] = useState<BrewViewMode>(route.viewMode)
  const [surfaceBoard, setSurfaceBoard] = useState<BrewBoard>(route.board)
  const lockViewport = surfaceView === 'sources' && surfaceBoard === 'feeds'
  useBrewSurface(sources.booting, lockViewport)

  if (sources.booting) {
    return <BrewPage lock={false} loading />
  }

  return (
    <BrewPage lock={lockViewport}>
      {item.opening && (
        <div className="brew-skin brew-toast" role="status">
          {t.common.loading}
        </div>
      )}
      <BrewViewLane
        wave={route.viewMode}
        onDisplayed={(next) => setSurfaceView(next as BrewViewMode)}
      >
        {route.viewMode === 'sources' && (
          <BrewSourceGrid
            sources={sources.sources}
            board={route.board}
            focusSourceId={route.railFocusId}
            onBoardSurface={setSurfaceBoard}
            onSourceClick={actions.openLatest}
            onRefreshSource={sources.refreshSource}
            onSourcesChange={sources.reloadBoard}
            onAddSource={sources.addSource}
            onUpdateSource={sources.updateSource}
            onDiscoverSource={sources.discoverSource}
            onGenerateStyleTags={sources.generateStyleTags}
            onImportOpml={sources.importOpml}
            onRemoveSources={sources.removeSources}
            onOpenItem={actions.openPreview}
            onPeekItem={(item) => prefetchArticleDetails([item.id])}
            onPeekEnd={cancelArticlePrefetch}
            onToggleStar={handleCardStar}
            onMarkAllRead={isAuthenticated ? actions.markAllRead : undefined}
            onOpenStarred={isAuthenticated ? handleOpenStarred : undefined}
            onWriteNote={isAdmin ? notes.write : undefined}
            isAuthenticated={isAuthenticated}
            isAdmin={isAdmin}
          />
        )}

        {showsFilterLane(
          route.viewMode,
          !!route.selectedTopic,
          isAuthenticated,
        ) ? (
          <BrewFilterLane
            sources={sources.sources}
            isAdmin={isAdmin}
            isAuthenticated={isAuthenticated}
            topicFeedMode={
              route.viewMode === 'topic-feed' ? topicFeedMode : undefined
            }
            starredMode={route.viewMode === 'starred' ? starredMode : undefined}
            items={listItems}
            selectedItem={item.selectedItem}
            loading={list.itemsLoading}
            hasMore={list.hasMore}
            total={list.total}
            onItemSelect={actions.select}
            onLoadMore={list.loadMore}
            onToggleStar={actions.toggleStar}
            onItemSelectToggle={starred.toggle}
          />
        ) : null}
      </BrewViewLane>

      <AnimatePresence mode="wait">
        {item.selectedItem && (
          <BrewReader
            key="brew-reader"
            item={item.selectedItem}
            onClose={handleCloseReader}
            onToggleStar={handleReaderStar}
            isAuthenticated={isAuthenticated}
            isAdmin={isAdmin}
            sourceType={item.selectedItemSource?.source_type}
            onEditNote={
              isAdmin && item.selectedItemSource?.source_type === 'note'
                ? () => notes.edit(item.selectedItem!.id)
                : undefined
            }
            shareUrl={
              item.selectedItemIsOwn
                ? `${typeof window !== 'undefined' ? window.location.origin : ''}${brewOwnItemPath(item.selectedItem.id)}`
                : undefined
            }
            onNavigateToArticle={actions.navigateToArticle}
            readingQueue={item.queue}
          />
        )}
      </AnimatePresence>

      {notes.noteEditor !== null && (
        <NoteEditor
          noteId={notes.noteEditor === 'new' ? undefined : notes.noteEditor}
          onClose={notes.close}
          onSaved={notes.onSaved}
          onDeleted={notes.onDeleted}
        />
      )}

      {error ? (
        <div className="brew-skin brew-toast" role="status">
          <span>{error}</span>
          <button
            type="button"
            className="brew-sheet__ghost is-fit"
            onClick={() => setError(null)}
          >
            {t.brew.close}
          </button>
        </div>
      ) : null}
    </BrewPage>
  )
}
