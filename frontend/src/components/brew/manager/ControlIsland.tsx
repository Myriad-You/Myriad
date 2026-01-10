/**
 * Brew 控制岛组件 (重构版)
 * 底部浮动控制栏，支持多种模式切换
 *
 * 模式：
 * - default: 默认模式（动态提示 + 功能按钮）
 * - search: 搜索模式
 * - edit: 编辑模式（含多选、删除、全部刷新、调整卡片尺寸）
 * - keyboard: 快捷键模式
 * - add: 添加订阅模式
 * - feed: 文章列表模式（返回、刷新、全部已读、外部链接）
 * - category-feed: 分类合并文章列表模式
 * - starred: 收藏文章模式
 * - starred-edit: 收藏编辑模式
 */

import type { BrewSource, CardSize, FeedType, SourceType } from '../../../types/brew'
import type { ControlMode, DynamicTip, SortMode, SortOption } from './modes'
import {
  LuClock as Clock,
  LuFolderOpen as FolderOpen,
  LuGripVertical as GripVertical,
  LuShuffle as Shuffle,
  LuSortAsc as SortAsc,
} from '@lib/icons'
import { AnimatePresence } from 'framer-motion'
import JSZip from 'jszip'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { useBrewAnimationConfig } from '../../../hooks/animation/pages/brew'
import { BREW_SHORTCUTS } from '../../../hooks/useBrewKeyboard'

import * as brewApi from '../../../services/brewApi'
// 导入模式组件
import {
  AddMode,
  CategoryFeedMode,

  DefaultMode,

  EditMode,
  FeedMode,
  KeyboardMode,
  SearchMode,

  StarredEditMode,
  StarredMode,
} from './modes'
import RSSHubConfigComponent from './RSSHubConfig'

// API URL
const API_URL = import.meta.env.PUBLIC_API_URL || ''

// 处理图标 URL - 确保正确的完整路径
function getIconUrl(iconUrl: string | null | undefined): string | null {
  if (!iconUrl)
    return null
  if (iconUrl.startsWith('/api/')) {
    return `${API_URL}${iconUrl}`
  }
  if (iconUrl.startsWith('http://') || iconUrl.startsWith('https://')) {
    return `${API_URL}/api/proxy/image?url=${encodeURIComponent(iconUrl)}`
  }
  return iconUrl
}

// 根据订阅源数据生成动态提示
function generateDynamicTips(sources: BrewSource[], brewTranslations: Record<string, string>): DynamicTip[] {
  const tips: DynamicTip[] = []
  const now = Date.now()

  // 找出未读最多的源
  const mostUnread = sources
    .filter(s => s.unread_count > 0)
    .sort((a, b) => b.unread_count - a.unread_count)[0]

  if (mostUnread && mostUnread.unread_count > 0) {
    tips.push({
      icon: '📚',
      iconUrl: mostUnread.icon || undefined,
      main: `${mostUnread.name}`,
      sub: (brewTranslations.tipUnreadCount || '{count} 条未读').replace('{count}', String(mostUnread.unread_count)),
    })
  }

  // 找出最近更新的源（1小时内）
  const recentlyUpdated = sources
    .filter(s => s.last_fetched_at && (now - s.last_fetched_at * 1000) < 3600000)
    .sort((a, b) => (b.last_fetched_at || 0) - (a.last_fetched_at || 0))

  if (recentlyUpdated.length > 0) {
    const source = recentlyUpdated[0]
    tips.push({
      icon: '🔄',
      iconUrl: source.icon || undefined,
      main: `${source.name}`,
      sub: brewTranslations.tipJustUpdated || '刚刚更新',
    })
  }

  // 找出有新文章的源
  const withNewItems = sources.filter(s => s.recent_items && s.recent_items.some(item => !item.is_read))
  if (withNewItems.length > 0) {
    const randomSource = withNewItems[Math.floor(Math.random() * withNewItems.length)]
    const newItem = randomSource.recent_items?.find(item => !item.is_read)
    if (newItem) {
      const title = newItem.title.length > 16 ? `${newItem.title.slice(0, 16)}...` : newItem.title
      tips.push({
        icon: '📰',
        iconUrl: randomSource.icon || undefined,
        main: title,
        sub: (brewTranslations.tipFromSource || '来自 {source}').replace('{source}', randomSource.name),
      })
    }
  }

  // 计算总未读数
  const totalUnread = sources.reduce((acc, s) => acc + (s.unread_count || 0), 0)
  if (totalUnread > 0) {
    tips.push({ icon: '📬', main: (brewTranslations.tipUnreadCount || '{count} 条未读').replace('{count}', String(totalUnread)), sub: brewTranslations.tipClickToView || '点击卡片查看' })
  }

  // 基础统计
  tips.push({ icon: '📡', main: (brewTranslations.tipSubscriptionCount || '{count} 个订阅').replace('{count}', String(sources.length)), sub: brewTranslations.tipManageSources || '管理你的信息源' })

  // 时段问候（作为兜底）
  const hour = new Date().getHours()
  if (hour >= 5 && hour < 12) {
    tips.push({ icon: '☀️', main: brewTranslations.tipMorning || '早安', sub: brewTranslations.tipStartReading || '开启今日阅读' })
  }
  else if (hour >= 12 && hour < 18) {
    tips.push({ icon: '🌤️', main: brewTranslations.tipAfternoon || '午后时光', sub: brewTranslations.tipRelaxReading || '适合轻松阅读' })
  }
  else {
    tips.push({ icon: '🌙', main: brewTranslations.tipEvening || '晚间阅读', sub: brewTranslations.tipQuietTime || '享受安静时刻' })
  }

  return tips
}

// 动态提示 Hook
function useDynamicTips(sources: BrewSource[], brewTranslations: Record<string, string>) {
  const tips = useMemo(() => generateDynamicTips(sources, brewTranslations), [sources, brewTranslations])
  const [currentIndex, setCurrentIndex] = useState(0)

  useEffect(() => {
    if (tips.length <= 1)
      return

    const interval = setInterval(() => {
      setCurrentIndex(prev => (prev + 1) % tips.length)
    }, 8000)

    return () => clearInterval(interval)
  }, [tips.length])

  return { tip: tips[currentIndex] || tips[0], key: currentIndex }
}

// Brew 导出清单类型
interface BrewExportManifest {
  version: string
  exported_at: string
  sources: Array<{
    url: string
    name: string
    category: string | null
    icon_file: string | null
    icon_url: string | null
    source_type: SourceType
    feed_type: FeedType
    theme_color: string | null
    update_interval: number
    card_size: string | null
    rsshub_route: string | null
    ai_style_tags: string[] | null
    admin_only: boolean
  }>
}

// 判断是否为 base64 图片数据
function isBase64Image(str: string | null): boolean {
  if (!str)
    return false
  return str.startsWith('data:image/')
}

// 从 base64 提取 MIME 类型和扩展名
function getBase64Info(base64: string): { mime: string, ext: string } {
  const match = base64.match(/^data:(image\/\w+);base64,/)
  if (match) {
    const mime = match[1]
    const ext = mime.split('/')[1] || 'png'
    return { mime, ext }
  }
  return { mime: 'image/png', ext: 'png' }
}

interface ControlIslandProps {
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
  onAddSource?: (url: string, name?: string, category?: string, icon?: string, sourceType?: SourceType) => Promise<void>
  onSourcesChange?: () => void
  sortMode?: SortMode
  onSortModeChange?: (mode: SortMode) => void
  isSubCategory?: boolean
  feedMode?: {
    source: BrewSource
    total: number
    onBack: () => void
    onRefresh: () => void
    onMarkAllRead: () => void
    isRefreshing?: boolean
  }
  categoryFeedMode?: {
    categoryName: string
    categoryLabel: string
    total: number
    unreadCount: number
    onBack: () => void
    onMarkAllRead: () => void
  }
  starredMode?: {
    total: number
    selectedIds: Set<number>
    isEditMode: boolean
    onBack: () => void
    onEnterEditMode: () => void
    onExitEditMode: () => void
    onSelectAll: () => void
    onBatchUnstar: () => void
    isProcessing?: boolean
  }
  isAdmin?: boolean
  isAuthenticated?: boolean
}

export default function ControlIsland({
  sources,
  filteredSources,
  categories,
  searchQuery = '',
  setSearchQuery,
  selectedIds = new Set(),
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
  onSourcesChange,
  sortMode = 'update',
  onSortModeChange,
  isSubCategory = false,
  feedMode,
  categoryFeedMode,
  starredMode,
  isAdmin = false,
  isAuthenticated = false,
}: ControlIslandProps) {
  const { t } = useI18n()
  const animConfig = useBrewAnimationConfig()
  const enableAnimations = animConfig.level !== 'none'

  // 根据模式确定初始状态
  const getInitialMode = (): ControlMode => {
    if (starredMode?.isEditMode)
      return 'starred-edit'
    if (starredMode)
      return 'starred'
    if (categoryFeedMode)
      return 'category-feed'
    if (feedMode)
      return 'feed'
    return 'default'
  }

  const [mode, setMode] = useState<ControlMode>(getInitialMode)
  const { tip, key: tipKey } = useDynamicTips(sources, t.brew as unknown as Record<string, string>)

  // 当模式变化时，自动切换
  useEffect(() => {
    if (starredMode?.isEditMode) {
      setMode('starred-edit')
    }
    else if (starredMode) {
      setMode('starred')
    }
    else if (categoryFeedMode) {
      setMode('category-feed')
    }
    else if (feedMode) {
      setMode('feed')
    }
    else {
      setMode('default')
    }
  }, [feedMode, categoryFeedMode, starredMode, starredMode?.isEditMode])

  // 导入/导出状态
  const [importExportLoading, setImportExportLoading] = useState(false)
  const [importExportError, setImportExportError] = useState<string | null>(null)
  const [importExportSuccess, setImportExportSuccess] = useState<string | null>(null)
  const [importProgress, setImportProgress] = useState<{ step: string, current: number, total: number } | null>(null)
  const [exporting, setExporting] = useState(false)

  // Refs
  const sortDropdownRef = useRef<HTMLDivElement>(null)
  const brewExportInputRef = useRef<HTMLInputElement>(null)

  // 排序菜单状态
  const [showSortDropdown, setShowSortDropdown] = useState(false)

  // 点击外部关闭排序下拉菜单
  useEffect(() => {
    if (!showSortDropdown)
      return

    const handleClickOutside = (e: MouseEvent | TouchEvent) => {
      if (sortDropdownRef.current && !sortDropdownRef.current.contains(e.target as Node)) {
        setShowSortDropdown(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    document.addEventListener('touchstart', handleClickOutside)

    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('touchstart', handleClickOutside)
    }
  }, [showSortDropdown])

  // 排序选项
  const allSortOptions: SortOption[] = [
    { value: 'update', labelKey: 'sortByUpdate', icon: <Clock className="w-4 h-4" /> },
    { value: 'custom', labelKey: 'sortByCustom', icon: <GripVertical className="w-4 h-4" /> },
    { value: 'category', labelKey: 'sortByCategory', icon: <FolderOpen className="w-4 h-4" /> },
    { value: 'random', labelKey: 'sortByRandom', icon: <Shuffle className="w-4 h-4" /> },
    { value: 'pinyin', labelKey: 'sortByPinyin', icon: <SortAsc className="w-4 h-4" /> },
  ]
  const sortOptions = isSubCategory
    ? allSortOptions.filter(o => o.value !== 'custom')
    : allSortOptions

  const currentSortOption = sortOptions.find(o => o.value === sortMode) || sortOptions[0]

  // 预置分类
  const presetCategories = [t.brew.friendLinks, t.brew.me]
  const allAddCategories = [...new Set([...presetCategories, ...categories])]

  // 同步编辑模式
  useEffect(() => {
    if (isEditMode && mode !== 'edit') {
      setMode('edit')
    }
    else if (!isEditMode && mode === 'edit') {
      setMode('default')
    }
  }, [isEditMode, mode])

  // 切换模式
  const handleModeChange = (newMode: ControlMode) => {
    if (newMode === 'edit') {
      onEnterEditMode?.()
    }
    else if (mode === 'edit') {
      onExitEditMode?.()
    }
    if (newMode === 'search') {
      setSearchQuery?.('')
    }
    setMode(newMode)
  }

  // 关闭当前模式
  const handleClose = () => {
    if (mode === 'search') {
      setSearchQuery?.('')
    }
    if (mode === 'edit') {
      onExitEditMode?.()
    }
    setMode('default')
  }

  // 分组快捷键
  const groupedShortcuts = useMemo(() => ({
    navigation: BREW_SHORTCUTS.filter(s => s.category === 'navigation'),
    article: BREW_SHORTCUTS.filter(s => s.category === 'article'),
    source: BREW_SHORTCUTS.filter(s => s.category === 'source'),
    other: BREW_SHORTCUTS.filter(s => s.category === 'other'),
  }), [])

  const categoryLabels: Record<string, string> = {
    navigation: t.brew.shortcutNavigation,
    article: t.brew.shortcutArticle,
    source: t.brew.shortcutSource,
    other: t.brew.shortcutOther,
  }

  // 导出 Brew 格式 (ZIP)
  const handleBrewExport = async () => {
    setImportExportLoading(true)
    setImportExportError(null)
    try {
      const zip = new JSZip()
      const iconsFolder = zip.folder('icons')

      const manifestSources: BrewExportManifest['sources'] = []

      for (let i = 0; i < sources.length; i++) {
        const s = sources[i]
        let iconFile: string | null = null
        let iconUrl: string | null = null

        if (s.icon) {
          if (isBase64Image(s.icon)) {
            const { ext } = getBase64Info(s.icon)
            iconFile = `icon_${i}.${ext}`
            const base64Data = s.icon.split(',')[1]
            iconsFolder?.file(iconFile, base64Data, { base64: true })
          }
          else {
            iconUrl = s.icon
          }
        }

        manifestSources.push({
          url: s.url,
          name: s.name,
          category: s.category,
          icon_file: iconFile,
          icon_url: iconUrl,
          source_type: s.source_type,
          feed_type: s.feed_type,
          theme_color: s.theme_color,
          update_interval: s.update_interval,
          card_size: s.card_size,
          rsshub_route: s.rsshub_route,
          ai_style_tags: s.ai_style_tags,
          admin_only: s.admin_only,
        })
      }

      const manifest: BrewExportManifest = {
        version: '1.0',
        exported_at: new Date().toISOString(),
        sources: manifestSources,
      }

      zip.file('manifest.json', JSON.stringify(manifest, null, 2))

      const blob = await zip.generateAsync({ type: 'blob' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `brew-export-${new Date().toISOString().slice(0, 10)}.brewpack`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)

      setImportExportSuccess(t.brew.exportSuccess?.replace('{count}', String(sources.length)) || `已导出 ${sources.length} 个订阅源`)
      setTimeout(() => setImportExportSuccess(null), 3000)
    }
    catch (err) {
      setImportExportError(t.brew.errorExportFailed)
    }
    finally {
      setImportExportLoading(false)
    }
  }

  // 导入 Brew 格式 (ZIP)
  const handleBrewImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file)
      return

    setImportExportLoading(true)
    setImportExportError(null)
    setImportExportSuccess(null)
    setImportProgress({ step: t.brew.importStepReading || '读取文件...', current: 0, total: 0 })

    try {
      setImportProgress({ step: t.brew.importStepUnzipping || '解压文件...', current: 0, total: 0 })
      const zip = await JSZip.loadAsync(file)

      setImportProgress({ step: t.brew.importStepParsing || '解析配置...', current: 0, total: 0 })
      const manifestFile = zip.file('manifest.json')
      if (!manifestFile) {
        throw new Error(t.brew.errorInvalidFormat || '无效的导入文件格式：缺少 manifest.json')
      }

      const manifestContent = await manifestFile.async('string')
      const manifest = JSON.parse(manifestContent) as BrewExportManifest

      if (!manifest.version || !manifest.sources || !Array.isArray(manifest.sources)) {
        throw new Error(t.brew.errorInvalidFormat || '无效的导入文件格式')
      }

      const total = manifest.sources.length
      let imported = 0
      let skipped = 0

      for (let i = 0; i < manifest.sources.length; i++) {
        const source = manifest.sources[i]
        setImportProgress({
          step: t.brew.importStepImporting?.replace('{name}', source.name) || `导入: ${source.name}`,
          current: i + 1,
          total,
        })

        try {
          const exists = sources.some(s => s.url === source.url)
          if (exists) {
            skipped++
            continue
          }

          let icon: string | undefined
          if (source.icon_file) {
            const iconFile = zip.file(`icons/${source.icon_file}`)
            if (iconFile) {
              const iconData = await iconFile.async('base64')
              const ext = source.icon_file.split('.').pop() || 'png'
              const mimeType = ext === 'svg' ? 'image/svg+xml' : `image/${ext}`
              icon = `data:${mimeType};base64,${iconData}`
            }
          }
          else if (source.icon_url) {
            icon = source.icon_url
          }

          const newSource = await brewApi.addSource({
            url: source.url,
            name: source.name,
            category: source.category || undefined,
            icon,
            source_type: source.source_type,
            feed_type: source.feed_type,
            update_interval: source.update_interval,
            rsshub_route: source.rsshub_route || undefined,
            admin_only: source.admin_only,
          })

          // 更新额外字段（theme_color, card_size, ai_style_tags）
          const hasExtraFields = source.theme_color || source.card_size || source.ai_style_tags
          if (hasExtraFields && newSource?.id) {
            await brewApi.updateSource(newSource.id, {
              theme_color: source.theme_color || undefined,
              card_size: source.card_size as CardSize || undefined,
              ai_style_tags: source.ai_style_tags || undefined,
            })
          }

          imported++
        }
        catch {
          skipped++
        }
      }

      setImportProgress(null)
      setImportExportSuccess(
        (t.brew.importSuccess || '导入成功：{imported} 个，跳过：{skipped} 个')
          .replace('{imported}', String(imported))
          .replace('{skipped}', String(skipped)),
      )
      setTimeout(() => setImportExportSuccess(null), 3000)
      onSourcesChange?.()
    }
    catch (err) {
      setImportProgress(null)
      setImportExportError(err instanceof Error ? err.message : (t.brew.errorImportFailed || '导入失败'))
    }
    finally {
      setImportExportLoading(false)
      if (brewExportInputRef.current) {
        brewExportInputRef.current.value = ''
      }
    }
  }

  // OPML 导出
  const handleOpmlExport = async () => {
    setExporting(true)
    try {
      const opml = await brewApi.exportOpml()
      const blob = new Blob([opml], { type: 'text/xml' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `brew-subscriptions-${new Date().toISOString().slice(0, 10)}.opml`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
    }
    catch (err) {
      console.error('OPML export failed:', err)
    }
    finally {
      setExporting(false)
    }
  }

  // OPML 导入
  const handleOpmlImport = async (content: string) => {
    const result = await brewApi.importOpml(content)
    return { imported: result.imported || 0, skipped: result.skipped || 0 }
  }

  // 探测 Feed
  const handleDiscover = async (url: string) => {
    const result = await brewApi.discoverSource(url.trim())
    return result
  }

  // 渲染模式内容
  const renderModeContent = (variant: 'mobile' | 'desktop') => {
    const brewT = t.brew as unknown as Record<string, string>

    switch (mode) {
      case 'search':
        return (
          <SearchMode
            variant={variant}
            searchQuery={searchQuery}
            setSearchQuery={setSearchQuery}
            filteredCount={filteredSources.length}
            onClose={handleClose}
            t={{
              searchSources: brewT.searchSources,
              resultsCount: brewT.resultsCount,
              closeSearch: brewT.closeSearch,
            }}
          />
        )

      case 'edit':
        return (
          <EditMode
            variant={variant}
            selectedIds={selectedIds}
            totalCount={filteredSources.length}
            isDeleting={isDeleting}
            isRefreshing={isRefreshing}
            isAuthenticated={isAuthenticated}
            onSelectAll={onSelectAll}
            onBatchDelete={onBatchDelete}
            onBatchRefresh={onBatchRefresh}
            onMarkAllSourcesRead={onMarkAllSourcesRead}
            onClose={handleClose}
            // 导入导出
            onBrewExport={handleBrewExport}
            onBrewImportFile={handleBrewImportFile}
            importExportLoading={importExportLoading}
            importProgress={importProgress}
            importExportSuccess={importExportSuccess}
            importExportError={importExportError}
            brewExportInputRef={brewExportInputRef}
            sourcesCount={sources.length}
            t={{
              selectAll: brewT.selectAll,
              deselectAll: brewT.deselectAll,
              deleteSelected: brewT.deleteSelected,
              refreshAllSources: brewT.refreshAllSources,
              markAllAsRead: brewT.markAllAsRead,
              exitEdit: brewT.exitEdit,
              exportBrewpack: brewT.exportBrewpack || '导出订阅包',
              importBrewpack: brewT.importBrewpack || '导入订阅包',
            }}
          />
        )

      case 'feed':
        if (!feedMode)
          return null
        return (
          <FeedMode
            variant={variant}
            feedMode={feedMode}
            isAdmin={isAdmin}
            isAuthenticated={isAuthenticated}
            t={{
              backToSourceList: brewT.backToSourceList,
              articlesCount: brewT.articlesCount,
              tipUnreadCount: brewT.tipUnreadCount,
              refreshSource: brewT.refreshSource,
              markAllAsRead: brewT.markAllAsRead,
              visitWebsite: brewT.visitWebsite,
            }}
          />
        )

      case 'category-feed':
        if (!categoryFeedMode)
          return null
        return (
          <CategoryFeedMode
            variant={variant}
            categoryFeedMode={categoryFeedMode}
            isAuthenticated={isAuthenticated}
            t={{
              backToAllSources: brewT.backToAllSources,
              totalArticles: brewT.totalArticles,
              tipUnreadCount: brewT.tipUnreadCount,
              markAllAsRead: brewT.markAllAsRead,
            }}
          />
        )

      case 'starred':
        if (!starredMode)
          return null
        return (
          <StarredMode
            variant={variant}
            starredMode={starredMode}
            t={{
              backToSourceList: brewT.backToSourceList,
              starredArticles: brewT.starredArticles,
              starredCount: brewT.starredCount,
              editMode: brewT.editMode,
            }}
          />
        )

      case 'starred-edit':
        if (!starredMode)
          return null
        return (
          <StarredEditMode
            variant={variant}
            starredMode={starredMode}
            t={{
              exitEdit: brewT.exitEdit,
              selectAllToggle: brewT.selectAllToggle,
              selectedCount: brewT.selectedCount,
              selectArticles: brewT.selectArticles,
              unstar: brewT.unstar,
            }}
          />
        )

      case 'keyboard':
        return (
          <KeyboardMode
            variant={variant}
            groupedShortcuts={groupedShortcuts}
            categoryLabels={categoryLabels}
            onClose={handleClose}
            t={{
              keyboardShortcuts: brewT.keyboardShortcuts,
              close: brewT.close,
              ...brewT,
            }}
          />
        )

      case 'add':
        return (
          <AddMode
            variant={variant}
            onClose={handleClose}
            allCategories={allAddCategories}
            sourcesCount={sources.length}
            onSubmit={onAddSource
              ? async (data: {
                sourceType: 'rss' | 'brewlia' | 'link' | 'rsshub'
                url: string
                name: string
                category: string
                customIcon: string | null
              }) => {
                try {
                  await onAddSource(
                    data.url,
                    data.name,
                    data.category,
                    data.customIcon || undefined,
                    data.sourceType,
                  )
                  return { success: true }
                }
                catch (err) {
                  return { success: false, error: err instanceof Error ? err.message : 'Failed' }
                }
              }
              : undefined}
            onDiscover={handleDiscover}
            onImportOpml={handleOpmlImport
              ? async (content: string) => {
                const result = await handleOpmlImport(content)
                return result || { imported: 0, skipped: 0 }
              }
              : undefined}
            onExportOpml={handleOpmlExport}
            RSSHubConfigComponent={RSSHubConfigComponent}
            t={{
              sourceTypeLabel: brewT.sourceTypeLabel,
              pureLink: brewT.pureLink,
              linkDesc: brewT.linkDesc,
              rssDesc: brewT.rssDesc,
              rsshubDesc: brewT.rsshubDesc,
              notionDesc: brewT.notionDesc,
              brewliaShortDesc: brewT.brewliaShortDesc,
              brewliaFeatures: brewT.brewliaFeatures,
              enableAI: brewT.enableAI,
              disableAI: brewT.disableAI,
              subscriptionUrlLabel: brewT.subscriptionUrlLabel,
              linkUrlLabel: brewT.linkUrlLabel,
              notionUrlLabel: brewT.notionUrlLabel,
              discover: brewT.discover,
              nameLabel: brewT.nameLabel,
              enterName: brewT.enterName,
              autoFetch: brewT.autoFetch,
              category: brewT.category,
              selectCategory: brewT.selectCategory,
              inputNewCategory: brewT.inputNewCategory,
              noCategory: brewT.noCategory,
              siteIcon: brewT.siteIcon,
              upload: brewT.upload,
              deleteIcon: brewT.deleteIcon,
              deleteCustomIcon: brewT.deleteCustomIcon,
              addLink: brewT.addLink,
              addBrewlia: brewT.addBrewlia,
              addRsshub: brewT.addRsshub,
              addSubscription: brewT.addSubscription,
              singleAdd: brewT.singleAdd,
              dropOpmlHere: brewT.dropOpmlHere,
              supportedFormats: brewT.supportedFormats,
              startImport: brewT.startImport,
              exportOpml: brewT.exportOpml,
              importResult: brewT.importResult,
              skippedCount: brewT.skippedCount,
              selectOpmlFile: brewT.selectOpmlFile,
              closeSearch: brewT.closeSearch,
              close: brewT.close,
            }}
          />
        )

      case 'default':
      default:
        return (
          <DefaultMode
            variant={variant}
            tip={tip}
            tipKey={tipKey}
            sortMode={sortMode}
            sortOptions={sortOptions}
            currentSortOption={currentSortOption}
            showSortDropdown={showSortDropdown}
            setShowSortDropdown={setShowSortDropdown}
            sortDropdownRef={sortDropdownRef}
            onSortModeChange={onSortModeChange}
            onModeChange={handleModeChange}
            isAdmin={isAdmin}
            hasAddSource={!!onAddSource}
            t={{
              sortMethod: brewT.sortMethod,
              search: brewT.search,
              editMode: brewT.editMode,
              edit: brewT.edit,
              shortcuts: brewT.shortcuts,
              addSubscription: brewT.addSubscription,
              add: brewT.add,
              ...brewT,
            }}
            getIconUrl={getIconUrl}
          />
        )
    }
  }

  return (
    <>
      {/* 移动端顶部控制条容器 */}
      <div className="sm:hidden w-full mb-4 touch-pan-y relative z-30">
        <AnimatePresence mode="wait">
          {renderModeContent('mobile')}
        </AnimatePresence>
      </div>

      {/* 桌面端底部浮动控制条容器 */}
      <div className="hidden sm:block fixed bottom-8 left-1/2 -translate-x-1/2 z-40">
        <AnimatePresence mode="wait">
          {renderModeContent('desktop')}
        </AnimatePresence>
      </div>
    </>
  )
}

// 导出类型
export type { SortMode }
