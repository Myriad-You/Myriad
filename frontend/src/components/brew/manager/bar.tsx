import type { ChangeEvent, ReactNode, RefObject } from 'react'
import type { BrewSource } from '../../../types/brew'
import type {
  ControlMode,
  ImportProgress,
  SortMode,
  StarredModeConfig,
  TopicFeedModeConfig,
} from './modes/types'
import {
  LuArrowUpDown as ArrowUpDown,
  LuCheck as Check,
  LuCheckCircle as CheckCircle,
  LuCheckSquare as CheckSquare,
  LuChevronDown as ChevronDown,
  LuChevronLeft as ChevronLeft,
  LuDownload as Download,
  LuEdit3 as Edit3,
  LuKeyboard as Keyboard,
  LuMinusSquare as MinusSquare,
  LuPlus as Plus,
  LuRefreshCw as RefreshCw,
  LuSearch as Search,
  LuSquare as Square,
  LuStar as Star,
  LuTrash2 as Trash2,
  LuUpload as Upload,
  LuX as X,
} from '@lib/icons'
import { useEffect, useRef, useState } from 'react'

import { useI18n } from '../../../contexts/I18nContext'
import { BREW_SHORTCUTS } from '../../../hooks/useBrewKeyboard'
import { SettingGuideBody } from '../../settings/guides/SettingGuideBody'
import { SettingTitleGuideEntry } from '../../settings/SettingTitleGuideEntry'
import { Spinner } from '../../Spinner'

import { refreshableSourceCount } from '../logic/board'
import {
  BrewBarMenu,
  BrewBarMenuItem,
  BrewBarMeta,
  BrewBarSearchField,
  BrewBarTitle,
  BrewBarWrap,
  BrewLabel,
  BrewMark,
  BrewTag,
} from '../ui/Bar'
import { useManagementDisplay } from '../ui/BrewManagement'
import { BrewChip } from '../ui/Chip'
import { cx } from '../ui/cx'
import { buildBrewSortOptions } from './modes/sortOptions'

function BrewGuideTag({
  title,
  guide,
  children,
  onClick,
}: {
  title: string
  guide: ReactNode
  children: ReactNode
  onClick?: () => void
}) {
  return (
    <SettingTitleGuideEntry
      title={title}
      guide={guide}
      requireShowDetails={false}
      renderTrigger={(api) => (
        <BrewTag
          pressed={api.open}
          onClick={() => {
            api.toggle()
            onClick?.()
          }}
        >
          {children}
        </BrewTag>
      )}
    />
  )
}

export function BrewBarDefault({
  sortMode,
  onSortModeChange,
  onModeChange,
  onOpenStarred,
  onWriteNote,
  isAdmin,
  isAuthenticated,
  hasAddSource,
  canEdit = true,
  tagFrom = 0,
}: {
  sortMode: SortMode
  onSortModeChange?: (mode: SortMode) => void
  onModeChange: (mode: ControlMode) => void
  onOpenStarred?: () => void
  onWriteNote?: () => void
  isAdmin?: boolean
  isAuthenticated?: boolean
  hasAddSource?: boolean
  canEdit?: boolean
  tagFrom?: number
}) {
  const { t } = useI18n()
  const brew = t.brew
  const [open, setOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)
  const options = buildBrewSortOptions()
  const current = options.find((option) => option.value === sortMode) ?? options[0]
  const displayControl = useManagementDisplay()
  const guideLabels = {
    what: t.config.guideSectionWhat,
    chain: t.config.guideSectionChain,
    frontend: t.config.guideSectionFrontend,
    notes: t.config.guideSectionNotes,
  }
  const shortcutsNotes = BREW_SHORTCUTS.map(
    (item) => `${item.key}  ${brew[item.descriptionKey]}`,
  ).join('\n')
  let tagAt = tagFrom

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent | TouchEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', close)
    document.addEventListener('touchstart', close)
    return () => {
      document.removeEventListener('mousedown', close)
      document.removeEventListener('touchstart', close)
    }
  }, [open])

  return (
    <>
      {displayControl ? (
        <BrewChip key="d:spread" id="d:spread" index={tagAt++}>
          {displayControl}
        </BrewChip>
      ) : null}
      <BrewChip key="d:sort" id="d:sort" index={tagAt++}>
        <BrewBarWrap wrapRef={menuRef}>
          <BrewTag
            pressed={open}
            title={brew.sortMethod}
            onClick={() => setOpen((next) => !next)}
          >
            <BrewMark>
              <ArrowUpDown />
            </BrewMark>
            <BrewLabel>{brew[current.labelKey] || brew.sortMethod}</BrewLabel>
            <ChevronDown
              aria-hidden
              className={cx('brew-bar__chev', open && 'is-open')}
            />
          </BrewTag>
          {open ? (
            <BrewBarMenu>
              {options.map((option) => {
                const on = sortMode === option.value
                return (
                  <BrewBarMenuItem
                    key={option.value}
                    on={on}
                    onClick={() => {
                      onSortModeChange?.(option.value)
                      setOpen(false)
                    }}
                  >
                    <BrewMark>{option.icon}</BrewMark>
                    <span className="brew-bar__label">
                      {brew[option.labelKey]}
                    </span>
                    {on ? (
                      <BrewMark>
                        <Check />
                      </BrewMark>
                    ) : null}
                  </BrewBarMenuItem>
                )
              })}
            </BrewBarMenu>
          ) : null}
        </BrewBarWrap>
      </BrewChip>
      {isAuthenticated && onOpenStarred ? (
        <BrewChip key="d:starred" id="d:starred" index={tagAt++}>
          <BrewTag title={brew.starred} onClick={onOpenStarred}>
            <BrewMark>
              <Star />
            </BrewMark>
            <BrewLabel>{brew.starred}</BrewLabel>
          </BrewTag>
        </BrewChip>
      ) : null}
      {isAdmin ? (
        <BrewChip key="d:edit" id="d:edit" index={tagAt++} conceal={!canEdit}>
          <BrewTag
            title={brew.editMode}
            disabled={!canEdit}
            onClick={() => onModeChange('edit')}
          >
            <BrewMark>
              <Edit3 />
            </BrewMark>
            <BrewLabel>{brew.edit}</BrewLabel>
          </BrewTag>
        </BrewChip>
      ) : null}
      <BrewChip key="d:keys" id="d:keys" index={tagAt++}>
        <BrewGuideTag
          title={brew.keyboardShortcuts}
          guide={
            <SettingGuideBody
              entry={{
                what: brew.shortcutsTip,
                frontend: brew.shortcutsGuideWhere,
                notes: shortcutsNotes,
              }}
              labels={guideLabels}
            />
          }
        >
          <BrewMark>
            <Keyboard />
          </BrewMark>
          <BrewLabel>{brew.shortcuts}</BrewLabel>
        </BrewGuideTag>
      </BrewChip>
      {onWriteNote ? (
        <BrewChip key="d:note" id="d:note" index={tagAt++}>
          <BrewTag title={brew.noteWrite} onClick={onWriteNote}>
            <BrewMark>
              <Edit3 />
            </BrewMark>
            <BrewLabel>{brew.noteWrite}</BrewLabel>
          </BrewTag>
        </BrewChip>
      ) : null}
      {isAdmin && hasAddSource ? (
        <BrewChip key="d:add" id="d:add" index={tagAt++}>
          <BrewGuideTag
            title={brew.addSubscription}
            guide={
              <SettingGuideBody
                entry={{
                  what: brew.addSubscriptionTip,
                  frontend: brew.addGuideWhere,
                  notes: brew.addGuideNotes,
                }}
                labels={guideLabels}
              />
            }
            onClick={() => onModeChange('add')}
          >
            <BrewMark>
              <Plus />
            </BrewMark>
            <BrewLabel>{brew.add}</BrewLabel>
          </BrewGuideTag>
        </BrewChip>
      ) : null}
    </>
  )
}

export function BrewBarSearch({
  searchQuery,
  setSearchQuery,
  filteredCount,
  onClose,
  tagFrom = 0,
}: {
  searchQuery: string
  setSearchQuery?: (query: string) => void
  filteredCount: number
  onClose: () => void
  tagFrom?: number
}) {
  const { t, format } = useI18n()
  const brew = t.brew
  let tagAt = tagFrom
  return (
    <>
      <BrewChip key="s:field" id="s:field" index={tagAt++}>
        <BrewMark>
          <Search />
        </BrewMark>
        <BrewBarSearchField
          value={searchQuery}
          onChange={setSearchQuery}
          placeholder={brew.searchSources}
        />
      </BrewChip>
      <BrewChip key="s:meta" id="s:meta" index={tagAt++}>
        <BrewBarMeta>
          {format(brew.resultsCount, { count: filteredCount })}
        </BrewBarMeta>
      </BrewChip>
      <BrewChip key="s:close" id="s:close" index={tagAt++}>
        <BrewTag title={brew.closeSearch} onClick={onClose}>
          <BrewMark>
            <X />
          </BrewMark>
          <BrewLabel>{brew.close}</BrewLabel>
        </BrewTag>
      </BrewChip>
    </>
  )
}

export function BrewBarEdit({
  selectedIds,
  totalCount,
  refreshableCount,
  isDeleting,
  isRefreshing,
  isAuthenticated,
  onSelectAll,
  onBatchDelete,
  onBatchRefresh,
  onMarkAllSourcesRead,
  onClose,
  onEditSelected,
  onBrewExport,
  onBrewImportFile,
  importExportLoading,
  importProgress,
  importExportSuccess,
  importExportError,
  brewExportInputRef,
  sourcesCount,
  tagFrom = 0,
}: {
  selectedIds: Set<number>
  totalCount: number
  refreshableCount: number
  isDeleting: boolean
  isRefreshing: boolean
  isAuthenticated: boolean
  onSelectAll?: () => void
  onBatchDelete?: () => void
  onBatchRefresh?: () => void
  onMarkAllSourcesRead?: () => void
  onClose: () => void
  onEditSelected?: () => void
  onBrewExport?: () => void
  onBrewImportFile?: (e: ChangeEvent<HTMLInputElement>) => void
  importExportLoading?: boolean
  importProgress?: ImportProgress | null
  importExportSuccess?: string | null
  importExportError?: string | null
  brewExportInputRef?: RefObject<HTMLInputElement | null>
  sourcesCount?: number
  tagFrom?: number
}) {
  const { t } = useI18n()
  const brew = t.brew
  const allOn = selectedIds.size === totalCount && totalCount > 0
  let tagAt = tagFrom
  return (
    <>
      <BrewChip key="e:all" id="e:all" index={tagAt++}>
        <BrewTag
          title={allOn ? brew.deselectAll : brew.selectAll}
          onClick={onSelectAll}
        >
          <BrewMark>
            {allOn ? (
              <CheckSquare />
            ) : selectedIds.size > 0 ? (
              <MinusSquare />
            ) : (
              <Square />
            )}
          </BrewMark>
          <BrewLabel>{allOn ? brew.deselectAll : brew.selectAll}</BrewLabel>
        </BrewTag>
      </BrewChip>
      <BrewChip key="e:count" id="e:count" index={tagAt++}>
        <BrewBarMeta>
          {selectedIds.size}/{totalCount}
        </BrewBarMeta>
      </BrewChip>
      {onEditSelected ? (
        <BrewChip key="e:one" id="e:one" index={tagAt++}>
          <BrewTag title={brew.editSource} onClick={onEditSelected}>
            <BrewMark>
              <Edit3 />
            </BrewMark>
            <BrewLabel>{brew.editThisSource}</BrewLabel>
          </BrewTag>
        </BrewChip>
      ) : null}
      <BrewChip key="e:del" id="e:del" index={tagAt++}>
        <BrewTag
          danger
          disabled={isDeleting || selectedIds.size === 0}
          title={brew.deleteSelected}
          onClick={onBatchDelete}
        >
          <BrewMark>
            <Trash2 />
          </BrewMark>
          <BrewLabel>{brew.deleteSelected}</BrewLabel>
        </BrewTag>
      </BrewChip>
      {refreshableCount > 0 ? (
        <BrewChip key="e:refresh" id="e:refresh" index={tagAt++}>
          <BrewTag
            disabled={isRefreshing}
            title={brew.refreshAllSources}
            onClick={onBatchRefresh}
          >
            <BrewMark>
              {isRefreshing ? <Spinner size="sm" color="current" /> : <RefreshCw />}
            </BrewMark>
            <BrewLabel>{brew.refreshAllSources}</BrewLabel>
          </BrewTag>
        </BrewChip>
      ) : null}
      {isAuthenticated && onMarkAllSourcesRead ? (
        <BrewChip key="e:read" id="e:read" index={tagAt++}>
          <BrewTag title={brew.markAllAsRead} onClick={onMarkAllSourcesRead}>
            <BrewMark>
              <CheckCircle />
            </BrewMark>
            <BrewLabel>{brew.markAllAsRead}</BrewLabel>
          </BrewTag>
        </BrewChip>
      ) : null}
      {onBrewExport ? (
        <BrewChip key="e:export" id="e:export" index={tagAt++}>
          <BrewTag
            disabled={importExportLoading || !sourcesCount}
            title={brew.exportBrewpack}
            onClick={onBrewExport}
          >
            <BrewMark>
              <Download />
            </BrewMark>
            <BrewLabel>{brew.exportBrewpack}</BrewLabel>
          </BrewTag>
        </BrewChip>
      ) : null}
      {onBrewImportFile ? (
        <BrewChip key="e:import" id="e:import" index={tagAt++}>
          <BrewTag htmlFor="brew-bar-import" title={brew.importBrewpack}>
            <BrewMark>
              <Upload />
            </BrewMark>
            <BrewLabel>{brew.importBrewpack}</BrewLabel>
            <input
              id="brew-bar-import"
              ref={brewExportInputRef}
              type="file"
              accept=".brewpack,.zip"
              onChange={onBrewImportFile}
              className="brew-bar__file"
              disabled={importExportLoading}
            />
          </BrewTag>
        </BrewChip>
      ) : null}
      {importProgress ? (
        <BrewChip key="e:progress" id="e:progress" index={tagAt++}>
          <BrewBarMeta>
            {importProgress.step}
            {importProgress.total > 0
              ? ` ${importProgress.current}/${importProgress.total}`
              : ''}
          </BrewBarMeta>
        </BrewChip>
      ) : null}
      {!importProgress && (importExportSuccess || importExportError) ? (
        <BrewChip key="e:result" id="e:result" index={tagAt++}>
          <BrewBarMeta>{importExportSuccess || importExportError}</BrewBarMeta>
        </BrewChip>
      ) : null}
      <BrewChip key="e:exit" id="e:exit" index={tagAt++}>
        <BrewTag title={brew.exitEdit} onClick={onClose}>
          <BrewMark>
            <X />
          </BrewMark>
          <BrewLabel>{brew.exitEdit}</BrewLabel>
        </BrewTag>
      </BrewChip>
    </>
  )
}

export function BrewBarTopicFeed({
  topicFeedMode,
  tagFrom = 0,
}: {
  topicFeedMode: TopicFeedModeConfig
  tagFrom?: number
}) {
  const { t, format } = useI18n()
  const brew = t.brew
  let tagAt = tagFrom
  return (
    <>
      <BrewChip key="t:back" id="t:back" index={tagAt++}>
        <BrewTag title={brew.backToAllSources} onClick={topicFeedMode.onBack}>
          <BrewMark>
            <ChevronLeft />
          </BrewMark>
          <BrewLabel>{brew.back}</BrewLabel>
        </BrewTag>
      </BrewChip>
      <BrewChip key="t:title" id="t:title" index={tagAt++} grow>
        <BrewBarTitle>
          <BrewLabel>{topicFeedMode.topicLabel}</BrewLabel>
          <BrewBarMeta>
            {format(brew.totalArticles, { count: topicFeedMode.total })}
          </BrewBarMeta>
        </BrewBarTitle>
      </BrewChip>
    </>
  )
}

export function BrewBarStarred({
  starredMode,
  tagFrom = 0,
}: {
  starredMode: StarredModeConfig
  tagFrom?: number
}) {
  const { t, format } = useI18n()
  const brew = t.brew
  let tagAt = tagFrom
  return (
    <>
      <BrewChip key="st:back" id="st:back" index={tagAt++}>
        <BrewTag title={brew.backToAllSources} onClick={starredMode.onBack}>
          <BrewMark>
            <ChevronLeft />
          </BrewMark>
          <BrewLabel>{brew.back}</BrewLabel>
        </BrewTag>
      </BrewChip>
      <BrewChip key="st:title" id="st:title" index={tagAt++} grow>
        <BrewBarTitle>
          <BrewMark>
            <Star />
          </BrewMark>
          <BrewLabel>{brew.starredArticles}</BrewLabel>
          <BrewBarMeta>
            {format(brew.starredCount, { count: starredMode.total })}
          </BrewBarMeta>
        </BrewBarTitle>
      </BrewChip>
      {starredMode.total > 0 ? (
        <BrewChip key="st:edit" id="st:edit" index={tagAt++}>
          <BrewTag title={brew.editMode} onClick={starredMode.onEnterEditMode}>
            <BrewMark>
              <Edit3 />
            </BrewMark>
            <BrewLabel>{brew.edit}</BrewLabel>
          </BrewTag>
        </BrewChip>
      ) : null}
    </>
  )
}

export function BrewBarStarredEdit({
  starredMode,
  tagFrom = 0,
}: {
  starredMode: StarredModeConfig
  tagFrom?: number
}) {
  const { t, format } = useI18n()
  const brew = t.brew
  const allOn =
    starredMode.selectedIds.size === starredMode.total && starredMode.total > 0
  let tagAt = tagFrom
  return (
    <>
      <BrewChip key="se:exit" id="se:exit" index={tagAt++}>
        <BrewTag title={brew.exitEdit} onClick={starredMode.onExitEditMode}>
          <BrewMark>
            <X />
          </BrewMark>
          <BrewLabel>{brew.exitEdit}</BrewLabel>
        </BrewTag>
      </BrewChip>
      <BrewChip key="se:all" id="se:all" index={tagAt++}>
        <BrewTag title={brew.selectAllToggle} onClick={starredMode.onSelectAll}>
          <BrewMark>
            {allOn ? (
              <CheckSquare />
            ) : starredMode.selectedIds.size > 0 ? (
              <MinusSquare />
            ) : (
              <Square />
            )}
          </BrewMark>
          <BrewLabel>
            {starredMode.selectedIds.size > 0
              ? format(brew.selectedCount, {
                  count: starredMode.selectedIds.size,
                })
              : brew.selectArticles}
          </BrewLabel>
        </BrewTag>
      </BrewChip>
      <BrewChip key="se:unstar" id="se:unstar" index={tagAt++}>
        <BrewTag
          danger
          disabled={
            starredMode.selectedIds.size === 0 || starredMode.isProcessing
          }
          title={brew.unstar}
          onClick={starredMode.onBatchUnstar}
        >
          <BrewMark>
            {starredMode.isProcessing ? (
              <Spinner size="sm" color="current" />
            ) : (
              <Star />
            )}
          </BrewMark>
          <BrewLabel>{brew.unstar}</BrewLabel>
        </BrewTag>
      </BrewChip>
    </>
  )
}

export function BrewBarPanelClose({
  title,
  onClose,
  tagFrom = 0,
}: {
  title: string
  onClose: () => void
  tagFrom?: number
}) {
  return (
    <BrewChip key="p:close" id="p:close" index={tagFrom}>
      <BrewTag pressed title={title} onClick={onClose}>
        <BrewMark>
          <X />
        </BrewMark>
        <BrewLabel>{title}</BrewLabel>
      </BrewTag>
    </BrewChip>
  )
}

export function BrewBarTags({
  mode,
  embedded,
  searchQuery,
  setSearchQuery,
  filteredSources,
  selectedIds,
  selectedSource,
  sortMode,
  onSortModeChange,
  onModeChange,
  onClose,
  topicFeedMode,
  starredMode,
  isDeleting,
  isRefreshing,
  isAuthenticated,
  isAdmin,
  hasAddSource,
  canEdit,
  onSelectAll,
  onBatchDelete,
  onBatchRefresh,
  onMarkAllSourcesRead,
  onOpenStarred,
  onWriteNote,
  pack,
  sourcesCount,
}: {
  mode: ControlMode
  embedded: boolean
  searchQuery: string
  setSearchQuery?: (query: string) => void
  filteredSources: BrewSource[]
  selectedIds: Set<number>
  selectedSource?: BrewSource
  sortMode: SortMode
  onSortModeChange?: (mode: SortMode) => void
  onModeChange: (mode: ControlMode) => void
  onClose: () => void
  topicFeedMode?: TopicFeedModeConfig
  starredMode?: StarredModeConfig
  isDeleting: boolean
  isRefreshing: boolean
  isAuthenticated: boolean
  isAdmin: boolean
  hasAddSource: boolean
  canEdit: boolean
  onSelectAll?: () => void
  onBatchDelete?: () => void
  onBatchRefresh?: () => void
  onMarkAllSourcesRead?: () => void
  onOpenStarred?: () => void
  onWriteNote?: () => void
  pack: {
    exportPack: () => void
    importFile: (event: ChangeEvent<HTMLInputElement>) => void
    loading: boolean
    progress: ImportProgress | null
    success: string | null
    error: string | null
    inputRef: RefObject<HTMLInputElement | null>
  }
  sourcesCount: number
}) {
  const { t } = useI18n()
  const tagFrom = 0

  if (mode === 'search') {
    return (
      <BrewBarSearch
        key="search"
        searchQuery={searchQuery}
        setSearchQuery={setSearchQuery}
        filteredCount={filteredSources.length}
        onClose={onClose}
        tagFrom={tagFrom}
      />
    )
  }
  if (mode === 'edit') {
    return (
      <BrewBarEdit
        key="edit"
        selectedIds={selectedIds}
        totalCount={filteredSources.length}
        refreshableCount={refreshableSourceCount(filteredSources)}
        isDeleting={isDeleting}
        isRefreshing={isRefreshing}
        isAuthenticated={isAuthenticated}
        onSelectAll={onSelectAll}
        onBatchDelete={onBatchDelete}
        onBatchRefresh={onBatchRefresh}
        onMarkAllSourcesRead={onMarkAllSourcesRead}
        onClose={onClose}
        onEditSelected={
          embedded && canEdit && selectedSource
            ? () => onModeChange('source-edit')
            : undefined
        }
        onBrewExport={pack.exportPack}
        onBrewImportFile={pack.importFile}
        importExportLoading={pack.loading}
        importProgress={pack.progress}
        importExportSuccess={pack.success}
        importExportError={pack.error}
        brewExportInputRef={pack.inputRef}
        sourcesCount={sourcesCount}
        tagFrom={tagFrom}
      />
    )
  }
  if (mode === 'topic-feed' && topicFeedMode) {
    return (
      <BrewBarTopicFeed
        key="topic-feed"
        topicFeedMode={topicFeedMode}
        tagFrom={tagFrom}
      />
    )
  }
  if (mode === 'starred' && starredMode) {
    return (
      <BrewBarStarred
        key="starred"
        starredMode={starredMode}
        tagFrom={tagFrom}
      />
    )
  }
  if (mode === 'starred-edit' && starredMode) {
    return (
      <BrewBarStarredEdit
        key="starred-edit"
        starredMode={starredMode}
        tagFrom={tagFrom}
      />
    )
  }
  if (mode === 'keyboard') {
    return (
      <BrewBarPanelClose
        key="keyboard"
        title={t.brew.shortcuts}
        onClose={onClose}
        tagFrom={tagFrom}
      />
    )
  }
  if (mode === 'add') {
    return (
      <BrewBarPanelClose
        key="add"
        title={t.brew.add}
        onClose={onClose}
        tagFrom={tagFrom}
      />
    )
  }
  if (mode === 'source-edit' && selectedSource) {
    return (
      <BrewBarPanelClose
        key="source-edit"
        title={t.brew.editSource}
        onClose={onClose}
        tagFrom={tagFrom}
      />
    )
  }
  return (
    <BrewBarDefault
      key="default"
      sortMode={sortMode}
      onSortModeChange={onSortModeChange}
      onModeChange={onModeChange}
      onOpenStarred={
        isAuthenticated && onOpenStarred ? onOpenStarred : undefined
      }
      onWriteNote={onWriteNote}
      isAdmin={isAdmin}
      isAuthenticated={isAuthenticated}
      hasAddSource={hasAddSource}
      canEdit={canEdit}
      tagFrom={tagFrom}
    />
  )
}
