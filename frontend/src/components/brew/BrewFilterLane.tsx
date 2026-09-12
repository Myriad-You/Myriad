/** 订阅轨不走这里。 */

import type { BrewItem, BrewSource } from '../../types/brew'
import type {
  StarredModeConfig,
  TopicFeedModeConfig,
} from './manager/modes'

import { useI18n } from '../../contexts/I18nContext'
import BrewControls from './manager/BrewControls'
import BrewListView from './skin/BrewList'

const EMPTY_CATEGORIES: string[] = []

interface BrewFilterLaneProps {
  sources: BrewSource[]
  isAdmin: boolean
  isAuthenticated: boolean
  topicFeedMode?: TopicFeedModeConfig
  starredMode?: StarredModeConfig
  items: BrewItem[]
  selectedItem: BrewItem | null
  loading: boolean
  hasMore: boolean
  total: number
  onItemSelect: (item: BrewItem) => void
  onLoadMore: () => void
  onToggleStar: (item: BrewItem) => void
  onItemSelectToggle?: (id: number) => void
}

export default function BrewFilterLane({
  sources,
  isAdmin,
  isAuthenticated,
  topicFeedMode,
  starredMode,
  items,
  selectedItem,
  loading,
  hasMore,
  total,
  onItemSelect,
  onLoadMore,
  onToggleStar,
  onItemSelectToggle,
}: BrewFilterLaneProps) {
  const { t } = useI18n()
  return (
    <div className="brew-page">
      <BrewControls
        sources={sources}
        filteredSources={sources}
        categories={EMPTY_CATEGORIES}
        isAdmin={isAdmin}
        isAuthenticated={isAuthenticated}
        topicFeedMode={topicFeedMode}
        starredMode={starredMode}
      />
      <BrewListView
        items={items}
        selectedItem={selectedItem}
        loading={loading}
        hasMore={hasMore}
        total={total}
        onItemSelect={onItemSelect}
        onLoadMore={onLoadMore}
        onToggleStar={onToggleStar}
        emptyText={starredMode ? t.brew.starredEmpty : undefined}
        emptyAction={
          starredMode
            ? { label: t.brew.back, onClick: starredMode.onBack }
            : undefined
        }
        editMode={starredMode?.isEditMode}
        selectedIds={starredMode?.selectedIds}
        onItemSelectToggle={onItemSelectToggle}
      />
    </div>
  )
}
