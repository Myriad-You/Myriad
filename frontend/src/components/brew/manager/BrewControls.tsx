import type { AddSourceInput, BrewSource, UpdateSourceRequest } from '../../../types/brew'
import type {
  BrewControlsHandle,
  SortMode,
  StarredModeConfig,
  TopicFeedModeConfig,
} from './modes'

import {
  forwardRef,
  useCallback,
  useImperativeHandle,
  useRef,
} from 'react'

import { useI18n } from '../../../contexts/I18nContext'
import { userFacingError } from '../../../utils/userFacingError'
import { BrewBar } from '../ui/Bar'
import { BrewManagement } from '../ui/BrewManagement'
import { BrewSearch } from '../ui/BrewSearch'
import { BrewBarTags } from './bar'
import { FormTurn } from './formTurn'
import { AddMode, EditSourceMode, KeyboardMode } from './modes'
import { toAddSourceInput } from './modes/addSource'
import RSSHubConfigComponent from './RSSHubConfig'
import { useBarWave } from './useBarWave'
import { useBrewpack } from './useBrewpack'

interface BrewControlsProps {
  sources: BrewSource[]
  filteredSources: BrewSource[]
  categories: string[]
  searchQuery?: string
  setSearchQuery?: (query: string) => void
  selectedIds?: Set<number>
  onSelectAll?: () => void
  onBatchDelete?: () => void
  onBatchRefresh?: () => void
  onMarkAllSourcesRead?: () => void
  onEnterEditMode?: () => void
  onExitEditMode?: () => void
  isEditMode?: boolean
  isDeleting?: boolean
  isRefreshing?: boolean
  onAddSource?: (input: AddSourceInput) => Promise<void>
  onUpdateSource?: (id: number, data: UpdateSourceRequest) => Promise<void>
  onDiscover?: (
    url: string,
    signal?: AbortSignal,
  ) => Promise<{
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
  onSourcesChange?: () => void
  sortMode?: SortMode
  onSortModeChange?: (mode: SortMode) => void
  starredMode?: StarredModeConfig
  topicFeedMode?: TopicFeedModeConfig
  isAdmin?: boolean
  isAuthenticated?: boolean
  onOpenStarred?: () => void
  onWriteNote?: () => void
  embedded?: boolean
  canEdit?: boolean
  onWaveDisplayed?: (wave: string) => void
}

export type { BrewControlsHandle }

const EMPTY_SELECTED_IDS = new Set<number>()

const BrewControls = forwardRef<BrewControlsHandle, BrewControlsProps>(
  (
    {
      sources,
      filteredSources,
      categories,
      searchQuery = '',
      setSearchQuery,
      selectedIds = EMPTY_SELECTED_IDS,
      onSelectAll,
      onBatchDelete,
      onBatchRefresh,
      onMarkAllSourcesRead,
      onEnterEditMode,
      onExitEditMode,
      isEditMode = false,
      isDeleting = false,
      isRefreshing = false,
      onAddSource,
      onUpdateSource,
      onDiscover,
      onGenerateStyleTags,
      onImportOpml,
      onSourcesChange,
      sortMode = 'smart',
      onSortModeChange,
      topicFeedMode,
      starredMode,
      isAdmin = false,
      isAuthenticated = false,
      onOpenStarred,
      onWriteNote,
      embedded = false,
      canEdit = true,
      onWaveDisplayed,
    },
    ref,
  ) => {
    const { t } = useI18n()
    const selectedSource =
      selectedIds.size === 1
        ? filteredSources.find((source) => selectedIds.has(source.id))
        : undefined
    const {
      mode,
      setMode,
      changeMode,
      close,
    } = useBarWave({
      starredEdit: !!starredMode?.isEditMode,
      hasStarred: !!starredMode,
      hasTopic: !!topicFeedMode,
      isEditMode,
      hasSelectedSource: !!selectedSource,
      onEnterEditMode,
      onExitEditMode,
      setSearchQuery,
    })
    const formTurn = useRef(new FormTurn())
    const dismissForm = useCallback(() => {
      formTurn.current.abandon()
      close()
    }, [close])
    const switchMode = useCallback(
      (next: Parameters<typeof changeMode>[0]) => {
        if (next !== 'add' && next !== 'source-edit') formTurn.current.abandon()
        changeMode(next)
      },
      [changeMode],
    )

    useImperativeHandle(ref, () => ({ changeMode: switchMode }), [switchMode])

    const handleWaveDisplayed = useCallback(
      (next: string) => {
        onWaveDisplayed?.(next)
      },
      [onWaveDisplayed, searchQuery, setSearchQuery],
    )

    const handleSourceSave = useCallback(
      async (id: number, data: UpdateSourceRequest) => {
        if (!onUpdateSource) return
        const turn = formTurn.current.begin()
        await onUpdateSource(id, data)
        if (!formTurn.current.isCurrent(turn)) return
        setMode('edit')
      },
      [onUpdateSource, setMode],
    )

    const handleAddSubmit = useCallback(
      async (data: Parameters<typeof toAddSourceInput>[0]) => {
        if (!onAddSource) return { success: false as const }
        try {
          await onAddSource(toAddSourceInput(data))
          return { success: true as const }
        } catch (err) {
          return {
            success: false as const,
            error: userFacingError(err, t.brew.errorAddFailed),
          }
        }
      },
      [onAddSource, t.brew.errorAddFailed],
    )

    const pack = useBrewpack(sources, onSourcesChange)
    const allAddCategories = Iterator.from(
      new Set([t.brew.friendLinks, t.brew.me]).union(new Set(categories)),
    ).toArray()

    const panel =
      mode === 'add' ? (
        <AddMode
          allCategories={allAddCategories}
          sourcesCount={sources.length}
          onSubmit={onAddSource ? handleAddSubmit : undefined}
          onDiscover={onDiscover}
          onImportOpml={onImportOpml}
          onExportOpml={pack.exportOpml}
          RSSHubConfigComponent={RSSHubConfigComponent}
        />
      ) : mode === 'source-edit' && selectedSource ? (
        <EditSourceMode
          key={selectedSource.id}
          source={selectedSource}
          categories={categories}
          onSave={handleSourceSave}
          onGenerateStyleTags={onGenerateStyleTags}
        />
      ) : mode === 'keyboard' ? (
        <KeyboardMode />
      ) : null

    const content = (
      <>
      <BrewSearch value={searchQuery} onChange={setSearchQuery} />
      <BrewBar
        page={!embedded}
        panel={panel}
        wave={mode}
        onDisplayed={handleWaveDisplayed}
      >
        <BrewBarTags
          mode={mode}
          embedded={embedded}
          searchQuery={searchQuery}
          setSearchQuery={setSearchQuery}
          filteredSources={filteredSources}
          selectedIds={selectedIds}
          selectedSource={selectedSource}
          sortMode={sortMode}
          onSortModeChange={onSortModeChange}
          onModeChange={switchMode}
          onClose={dismissForm}
          topicFeedMode={topicFeedMode}
          starredMode={starredMode}
          isDeleting={isDeleting}
          isRefreshing={isRefreshing}
          isAuthenticated={isAuthenticated}
          isAdmin={isAdmin}
          hasAddSource={!!onAddSource}
          canEdit={canEdit}
          onSelectAll={onSelectAll}
          onBatchDelete={onBatchDelete}
          onBatchRefresh={onBatchRefresh}
          onMarkAllSourcesRead={onMarkAllSourcesRead}
          onOpenStarred={onOpenStarred}
          onWriteNote={onWriteNote}
          pack={pack}
          sourcesCount={sources.length}
        />
      </BrewBar>
      </>
    )
    return embedded ? content : (
      <BrewManagement embedded={false} active={mode !== 'default'}>{content}</BrewManagement>
    )
  },
)

export default BrewControls
export type { SortMode }
