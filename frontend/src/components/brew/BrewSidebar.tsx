/**
 * Brew 侧边栏组件
 * 显示订阅源列表、分类、筛选器
 *
 * 性能优化：
 * - useMemo 缓存分类计算
 * - useCallback 缓存回调函数
 * - memo 避免不必要的重渲染
 */

import type { BrewSource, BrewStats } from '../../types/brew'
import {
  LuChevronLeft as ChevronLeft,
  LuChevronRight as ChevronRight,
  LuFileText as FileText,
  LuFolder as Folder,
  LuGlobe as Globe,
  LuInbox as Inbox,
  LuKeyboard as Keyboard,
  LuMoreHorizontal as MoreHorizontal,
  LuPlus as Plus,
  LuRefreshCw as RefreshCw,
  LuRss as Rss,
  LuStar as Star,
  LuTrash2 as Trash2,
} from '@lib/icons'
import { memo, useCallback, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'

interface BrewSidebarProps {
  sources: BrewSource[]
  stats: BrewStats | null
  selectedSourceId: number | null
  selectedCategory: string | null
  filter: 'all' | 'unread' | 'starred'
  collapsed: boolean
  onSourceSelect: (sourceId: number | null) => void
  onCategorySelect: (category: string | null) => void
  onFilterChange: (filter: 'all' | 'unread' | 'starred') => void
  onAddSource: () => void
  onDeleteSource: (sourceId: number) => void
  onRefreshSource: (sourceId: number) => void
  onToggleCollapse: () => void
  onOpenOpml: () => void
  onShowKeyboardHelp: () => void
}

export default memo(({
  sources,
  stats,
  selectedSourceId,
  selectedCategory,
  filter,
  collapsed,
  onSourceSelect,
  onCategorySelect,
  onFilterChange,
  onAddSource,
  onDeleteSource,
  onRefreshSource,
  onToggleCollapse,
  onOpenOpml,
  onShowKeyboardHelp,
}: BrewSidebarProps) => {
  const { t } = useI18n()
  const [contextMenu, setContextMenu] = useState<{ sourceId: number, x: number, y: number } | null>(null)

  // 获取分类列表 - 支持多分类（逗号分隔）- useMemo 缓存
  const categories = useMemo(() => [...new Set(
    sources
      .filter(s => s.category)
      .flatMap(s => s.category!.split(',').map(c => c.trim()).filter(Boolean)),
  )], [sources])

  // 按分类分组订阅源 - 支持多分类（一个源可能出现在多个分类下）- useMemo 缓存
  const sourcesByCategory = useMemo(() => sources.reduce((acc, source) => {
    if (source.category) {
      const cats = source.category.split(',').map(c => c.trim()).filter(Boolean)
      cats.forEach((cat) => {
        if (!acc[cat])
          acc[cat] = []
        if (!acc[cat].find(s => s.id === source.id)) {
          acc[cat].push(source)
        }
      })
    }
    else {
      if (!acc[t.brew.uncategorized])
        acc[t.brew.uncategorized] = []
      acc[t.brew.uncategorized].push(source)
    }
    return acc
  }, {} as Record<string, BrewSource[]>), [sources])

  // 处理右键菜单 - useCallback 缓存
  const handleContextMenu = useCallback((e: React.MouseEvent, sourceId: number) => {
    e.preventDefault()
    setContextMenu({ sourceId, x: e.clientX, y: e.clientY })
  }, [])

  // 关闭右键菜单 - useCallback 缓存
  const closeContextMenu = useCallback(() => setContextMenu(null), [])

  if (collapsed) {
    return (
      <aside className="w-16 h-screen bg-gray-900/50 backdrop-blur-sm border-r border-white/10 flex flex-col items-center py-4 shrink-0">
        <button
          onClick={onToggleCollapse}
          className="p-2 rounded-lg hover:bg-white/10 transition-colors mb-4"
          aria-label={t.brew.expandSidebar}
        >
          <ChevronRight className="w-5 h-5 text-gray-400" />
        </button>

        <div className="flex flex-col gap-2">
          <button
            onClick={() => { onSourceSelect(null); onCategorySelect(null); onFilterChange('all') }}
            className={`p-3 rounded-lg transition-all duration-200 ease-out ${
              filter === 'all' && !selectedSourceId && !selectedCategory
                ? 'bg-blue-500/20 text-blue-400'
                : 'hover:bg-white/10 text-gray-400'
            }`}
            title={t.brew.all}
          >
            <Inbox className="w-5 h-5" />
          </button>

          <button
            onClick={() => onFilterChange('unread')}
            className={`p-3 rounded-lg transition-all duration-200 ease-out ${
              filter === 'unread' ? 'bg-blue-500/20 text-blue-400' : 'hover:bg-white/10 text-gray-400'
            }`}
            title={t.brew.unread}
          >
            <Rss className="w-5 h-5" />
          </button>

          <button
            onClick={() => onFilterChange('starred')}
            className={`p-3 rounded-lg transition-all duration-200 ease-out ${
              filter === 'starred' ? 'bg-yellow-500/20 text-yellow-400' : 'hover:bg-white/10 text-gray-400'
            }`}
            title={t.brew.starred}
          >
            <Star className="w-5 h-5" />
          </button>
        </div>

        <div className="mt-auto">
          <button
            onClick={onAddSource}
            className="p-3 rounded-lg hover:bg-white/10 text-gray-400 transition-colors"
            title={t.brew.addSubscription}
          >
            <Plus className="w-5 h-5" />
          </button>
        </div>
      </aside>
    )
  }

  return (
    <>
      <aside className="w-64 h-screen bg-gray-900/50 backdrop-blur-sm border-r border-white/10 flex flex-col shrink-0">
        {/* 头部 */}
        <div className="p-4 border-b border-white/10 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-white flex items-center gap-2">
            <Rss className="w-5 h-5 text-orange-400" />
            {t.brew.brewReader}
          </h2>
          <button
            onClick={onToggleCollapse}
            className="p-1.5 rounded-lg hover:bg-white/10 transition-colors"
            aria-label={t.brew.collapseSidebar}
          >
            <ChevronLeft className="w-4 h-4 text-gray-400" />
          </button>
        </div>

        {/* 统计 */}
        {stats && (
          <div className="px-4 py-3 border-b border-white/10 text-sm text-gray-400">
            <div className="flex items-center justify-between">
              <span>{t.brew.sourcesCount.replace('{count}', String(stats.total_sources))}</span>
              <span className="text-blue-400">{t.brew.unreadCount.replace('{count}', String(stats.total_unread))}</span>
            </div>
          </div>
        )}

        {/* 快捷筛选 */}
        <div className="p-2 border-b border-white/10">
          <button
            onClick={() => { onSourceSelect(null); onCategorySelect(null); onFilterChange('all') }}
            className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg transition-colors ${
              filter === 'all' && !selectedSourceId && !selectedCategory
                ? 'bg-blue-500/20 text-blue-400'
                : 'hover:bg-white/5 text-gray-300'
            }`}
          >
            <Inbox className="w-4 h-4" />
            <span>{t.brew.allArticles}</span>
            <span className="ml-auto text-xs opacity-60">{stats?.total_items || 0}</span>
          </button>

          <button
            onClick={() => { onSourceSelect(null); onCategorySelect(null); onFilterChange('unread') }}
            className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg transition-all duration-200 ease-out ${
              filter === 'unread' && !selectedSourceId && !selectedCategory
                ? 'bg-blue-500/20 text-blue-400'
                : 'hover:bg-white/5 text-gray-300'
            }`}
          >
            <Rss className="w-4 h-4" />
            <span>{t.brew.unread}</span>
            <span className="ml-auto text-xs opacity-60">{stats?.total_unread || 0}</span>
          </button>

          <button
            onClick={() => { onSourceSelect(null); onCategorySelect(null); onFilterChange('starred') }}
            className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg transition-all duration-200 ease-out ${
              filter === 'starred' && !selectedSourceId && !selectedCategory
                ? 'bg-yellow-500/20 text-yellow-400'
                : 'hover:bg-white/5 text-gray-300'
            }`}
          >
            <Star className="w-4 h-4" />
            <span>{t.brew.starred}</span>
            <span className="ml-auto text-xs opacity-60">{stats?.total_starred || 0}</span>
          </button>
        </div>

        {/* 订阅源列表 */}
        <div className="flex-1 overflow-y-auto p-2">
          {Object.entries(sourcesByCategory).map(([category, catSources]) => (
            <div key={category} className="mb-2">
              {/* 分类标题 */}
              <button
                onClick={() => onCategorySelect(category === t.brew.uncategorized ? null : category)}
                className={`w-full flex items-center gap-2 px-3 py-1.5 text-xs font-medium uppercase tracking-wider rounded-lg transition-all duration-200 ease-out ${
                  selectedCategory === category
                    ? 'bg-white/10 text-white'
                    : 'text-gray-500 hover:text-gray-300 hover:bg-white/5'
                }`}
              >
                <Folder className="w-3 h-3" />
                {category}
                <span className="ml-auto">{catSources.length}</span>
              </button>

              {/* 订阅源 */}
              <div className="mt-1 space-y-0.5">
                {catSources.map(source => (
                  <button
                    key={source.id}
                    onClick={() => onSourceSelect(source.id)}
                    onContextMenu={e => handleContextMenu(e, source.id)}
                    className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg transition-all duration-200 ease-out group ${
                      selectedSourceId === source.id
                        ? 'bg-blue-500/20 text-blue-400'
                        : 'hover:bg-white/5 text-gray-300'
                    }`}
                  >
                    {source.icon
                      ? (
                          <img
                            src={source.icon}
                            alt=""
                            className="w-4 h-4 rounded object-cover"
                            onError={(e) => {
                              (e.target as HTMLImageElement).style.display = 'none'
                            }}
                          />
                        )
                      : (
                          <Globe className="w-4 h-4 text-gray-500" />
                        )}
                    <span className="flex-1 truncate text-sm text-left">{source.name}</span>
                    {source.unread_count > 0 && (
                      <span className="px-1.5 py-0.5 text-xs bg-blue-500/30 text-blue-300 rounded">
                        {source.unread_count}
                      </span>
                    )}
                    <button
                      onClick={(e) => {
                        e.stopPropagation()
                        handleContextMenu(e, source.id)
                      }}
                      className="opacity-0 group-hover:opacity-100 p-1 hover:bg-white/10 rounded transition-all"
                      aria-label={t.brew.moreOptions}
                    >
                      <MoreHorizontal className="w-3 h-3" />
                    </button>
                  </button>
                ))}
              </div>
            </div>
          ))}

          {sources.length === 0 && (
            <div className="text-center py-8 text-gray-500">
              <Rss className="w-8 h-8 mx-auto mb-2 opacity-50" />
              <p className="text-sm">{t.brew.noSources}</p>
              <button
                onClick={onAddSource}
                className="mt-2 text-blue-400 text-sm hover:underline"
              >
                {t.brew.addFirstSubscription}
              </button>
            </div>
          )}
        </div>

        {/* 底部操作 */}
        <div className="p-3 border-t border-white/10 space-y-2">
          <button
            onClick={onAddSource}
            className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-blue-500/20 hover:bg-blue-500/30 text-blue-400 rounded-lg transition-colors"
          >
            <Plus className="w-4 h-4" />
            <span>{t.brew.addSubscription}</span>
          </button>

          <div className="flex gap-2">
            <button
              onClick={onOpenOpml}
              className="flex-1 flex items-center justify-center gap-1.5 px-3 py-1.5 text-xs text-gray-400 hover:text-white hover:bg-white/10 rounded-lg transition-colors"
              title={t.brew.importExportOpml}
            >
              <FileText className="w-3.5 h-3.5" />
              OPML
            </button>
            <button
              onClick={onShowKeyboardHelp}
              className="flex-1 flex items-center justify-center gap-1.5 px-3 py-1.5 text-xs text-gray-400 hover:text-white hover:bg-white/10 rounded-lg transition-colors"
              title={t.brew.keyboardShortcutsHint}
            >
              <Keyboard className="w-3.5 h-3.5" />
              {t.brew.shortcuts}
            </button>
          </div>
        </div>
      </aside>

      {/* 右键菜单 */}
      {contextMenu && (
        <>
          <div
            className="fixed inset-0 z-40"
            onClick={closeContextMenu}
          />
          <div
            className="fixed z-50 bg-gray-800 border border-white/10 rounded-lg shadow-xl py-1 min-w-[160px]"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            <button
              onClick={() => {
                onRefreshSource(contextMenu.sourceId)
                closeContextMenu()
              }}
              className="w-full flex items-center gap-2 px-4 py-2 text-sm text-gray-300 hover:bg-white/10"
            >
              <RefreshCw className="w-4 h-4" />
              {t.brew.refresh}
            </button>
            <button
              onClick={() => {
                onDeleteSource(contextMenu.sourceId)
                closeContextMenu()
              }}
              className="w-full flex items-center gap-2 px-4 py-2 text-sm text-red-400 hover:bg-white/10"
            >
              <Trash2 className="w-4 h-4" />
              {t.brew.delete}
            </button>
          </div>
        </>
      )}
    </>
  )
})
