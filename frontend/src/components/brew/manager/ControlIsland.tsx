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

import type {
  AddSourceInput,
  BrewSource,
  CardSize,
  FeedType,
  SourceType,
} from '../../../types/brew'

import type {
  ControlMode,
  DynamicTip,
  SortMode,
  SortOption,
  TopicFeedModeConfig,
} from './modes'
import {
  LuClock as Clock,
  LuFolderOpen as FolderOpen,
  LuGripVertical as GripVertical,
  LuShuffle as Shuffle,
  LuSortAsc as SortAsc,
  LuSparkles as Sparkles,
  LuTag as Tag,
} from '@lib/icons'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { userFacingError } from '../../../utils/userFacingError'

import { BREW_SHORTCUTS } from '../../../hooks/useBrewKeyboard'
import * as brewApi from '../../../services/brewApi'
import { getIconUrl, IslandLayout } from '../../shared/control-island'
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
  TopicFeedMode,
} from './modes'
import RSSHubConfigComponent from './RSSHubConfig'

const BREW_TIP_ICON_ASSETS = {
  unreadInbox: '/icons/brew/inbox.webp',
  subscriptions: '/icons/notifications/brew.webp',
  morning: '/icons/greeting/sunrise.webp',
  afternoon: '/icons/weather/partly-cloudy.webp',
  evening: '/icons/greeting/night.webp',
} as const

// 根据订阅源数据生成动态提示
function generateDynamicTips(
  sources: BrewSource[],
  brewTranslations: Record<string, string>,
  isAuthenticated: boolean,
): DynamicTip[] {
  const tips: DynamicTip[] = []
  const now = Date.now()

  // 找出未读最多的源
  const mostUnread = sources
    .filter((s) => s.unread_count > 0)
    .sort((a, b) => b.unread_count - a.unread_count)[0]

  if (mostUnread && mostUnread.unread_count > 0) {
    // 仅用网站图标；丢失/无图标时不展示 emoji 回退
    tips.push({
      icon: '',
      iconUrl: mostUnread.icon || undefined,
      main: `${mostUnread.name}`,
      sub: brewTranslations.tipUnreadCount.replace(
        '{count}',
        String(mostUnread.unread_count),
      ),
    })
  }

  // 找出最近成功更新的源（1小时内）
  // last_fetched_at is written even on failure; tip must use last_success_at.
  const recentlyUpdated = sources
    .filter(
      (s) =>
        s.source_type !== 'link' &&
        s.last_success_at &&
        now - s.last_success_at < 3600000,
    )
    .sort((a, b) => (b.last_success_at || 0) - (a.last_success_at || 0))

  if (recentlyUpdated.length > 0) {
    const source = recentlyUpdated[0]
    tips.push({
      icon: '',
      iconUrl: source.icon || undefined,
      main: `${source.name}`,
      sub: brewTranslations.tipJustUpdated,
    })
  }

  // 找出有新文章的源。
  // 游客侧 recent_items[].is_read 恒 false（后端用 user_id = -1 做 LEFT JOIN），
  // 不按角色跳过的话「来自 XX 的新文章」会永远命中。
  const withNewItems = isAuthenticated
    ? sources.filter(
        (s) => s.recent_items && s.recent_items.some((item) => !item.is_read),
      )
    : []
  if (withNewItems.length > 0) {
    const randomSource =
      withNewItems[Math.floor(Math.random() * withNewItems.length)]
    const newItem = randomSource.recent_items?.find((item) => !item.is_read)
    if (newItem) {
      const title =
        newItem.title.length > 16
          ? `${newItem.title.slice(0, 16)}...`
          : newItem.title
      tips.push({
        icon: '',
        iconUrl: randomSource.icon || undefined,
        main: title,
        sub: brewTranslations.tipFromSource.replace(
          '{source}',
          randomSource.name,
        ),
      })
    }
  }

  // 计算总未读数
  const totalUnread = sources.reduce((acc, s) => acc + (s.unread_count || 0), 0)
  if (totalUnread > 0) {
    tips.push({
      icon: '📥',
      iconUrl: BREW_TIP_ICON_ASSETS.unreadInbox,
      main: brewTranslations.tipUnreadCount.replace(
        '{count}',
        String(totalUnread),
      ),
      sub: brewTranslations.tipClickToView,
    })
  }

  // 基础统计
  tips.push({
    icon: '☕',
    iconUrl: BREW_TIP_ICON_ASSETS.subscriptions,
    main: brewTranslations.tipSubscriptionCount.replace(
      '{count}',
      String(sources.length),
    ),
    sub: brewTranslations.tipManageSources,
  })

  // 时段问候（作为兜底）
  const hour = new Date().getHours()
  if (hour >= 5 && hour < 12) {
    tips.push({
      icon: '🌅',
      iconUrl: BREW_TIP_ICON_ASSETS.morning,
      main: brewTranslations.tipMorning,
      sub: brewTranslations.tipStartReading,
    })
  } else if (hour >= 12 && hour < 18) {
    tips.push({
      icon: '🌤️',
      iconUrl: BREW_TIP_ICON_ASSETS.afternoon,
      main: brewTranslations.tipAfternoon,
      sub: brewTranslations.tipRelaxReading,
    })
  } else {
    tips.push({
      icon: '🌙',
      iconUrl: BREW_TIP_ICON_ASSETS.evening,
      main: brewTranslations.tipEvening,
      sub: brewTranslations.tipQuietTime,
    })
  }

  return tips
}

// 动态提示 Hook
function useDynamicTips(
  sources: BrewSource[],
  brewTranslations: Record<string, string>,
  isAuthenticated: boolean,
) {
  const tips = useMemo(
    () => generateDynamicTips(sources, brewTranslations, isAuthenticated),
    [sources, brewTranslations, isAuthenticated],
  )
  const [currentIndex, setCurrentIndex] = useState(0)

  useEffect(() => {
    if (tips.length <= 1) return

    const interval = setInterval(() => {
      setCurrentIndex((prev) => (prev + 1) % tips.length)
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

async function loadJSZip() {
  const module = await import('jszip')
  return module.default
}

// 判断是否为 base64 图片数据
function isBase64Image(str: string | null): boolean {
  if (!str) return false
  return str.startsWith('data:image/')
}

// 从 base64 提取 MIME 类型和扩展名
function getBase64Info(base64: string): { mime: string; ext: string } {
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
  onAddSource?: (input: AddSourceInput) => Promise<void>
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
  /** 主题 feed（跨源列表）模式；抄 categoryFeedMode 的形状 */
  topicFeedMode?: TopicFeedModeConfig
  /** 主题身份色（TopicFeedMode 的字标底色） */
  topicHue?: string
  isAdmin?: boolean
  isAuthenticated?: boolean
  /** 当期成卡的主题数；< 3 时 `topic` 排序置灰 */
  topicCount?: number
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
  sortMode = 'smart',
  onSortModeChange,
  isSubCategory = false,
  feedMode,
  categoryFeedMode,
  topicFeedMode,
  topicHue,
  starredMode,
  isAdmin = false,
  isAuthenticated = false,
  topicCount = 0,
}: ControlIslandProps) {
  const { t } = useI18n()

  // 根据模式确定初始状态
  const getInitialMode = (): ControlMode => {
    if (starredMode?.isEditMode) return 'starred-edit'
    if (starredMode) return 'starred'
    if (categoryFeedMode) return 'category-feed'
    if (topicFeedMode) return 'topic-feed'
    if (feedMode) return 'feed'
    return 'default'
  }

  const [mode, setMode] = useState<ControlMode>(getInitialMode)
  const { tip, key: tipKey } = useDynamicTips(
    sources,
    t.brew as unknown as Record<string, string>,
    isAuthenticated,
  )

  // 当模式变化时，自动切换
  useEffect(() => {
    if (starredMode?.isEditMode) {
      setMode('starred-edit')
    } else if (starredMode) {
      setMode('starred')
    } else if (categoryFeedMode) {
      setMode('category-feed')
    } else if (topicFeedMode) {
      setMode('topic-feed')
    } else if (feedMode) {
      setMode('feed')
    } else {
      setMode('default')
    }
  }, [
    feedMode,
    categoryFeedMode,
    topicFeedMode,
    starredMode,
    starredMode?.isEditMode,
  ])

  // 导入/导出状态
  const [importExportLoading, setImportExportLoading] = useState(false)
  const [importExportError, setImportExportError] = useState<string | null>(
    null,
  )
  const [importExportSuccess, setImportExportSuccess] = useState<string | null>(
    null,
  )
  const [importProgress, setImportProgress] = useState<{
    step: string
    current: number
    total: number
  } | null>(null)
  const [_exporting, setExporting] = useState(false)

  // Refs
  const sortDropdownRef = useRef<HTMLDivElement>(null)
  const brewExportInputRef = useRef<HTMLInputElement>(null)

  // 排序菜单状态
  const [showSortDropdown, setShowSortDropdown] = useState(false)

  // 点击外部关闭排序下拉菜单
  useEffect(() => {
    if (!showSortDropdown) return

    const handleClickOutside = (e: MouseEvent | TouchEvent) => {
      if (
        sortDropdownRef.current &&
        !sortDropdownRef.current.contains(e.target as Node)
      ) {
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
    {
      value: 'smart',
      labelKey: 'sortBySmart',
      icon: <Sparkles className="w-4 h-4" />,
    },
    {
      value: 'topic',
      labelKey: 'sortByTopic',
      icon: <Tag className="w-4 h-4" />,
      // 主题不够多时聚合没有意义（不足 3 篇的主题本来就不成卡）
      disabled: topicCount < 3,
    },
    {
      value: 'update',
      labelKey: 'sortByUpdate',
      icon: <Clock className="w-4 h-4" />,
    },
    {
      value: 'custom',
      labelKey: 'sortByCustom',
      icon: <GripVertical className="w-4 h-4" />,
    },
    {
      value: 'category',
      labelKey: 'sortByCategory',
      icon: <FolderOpen className="w-4 h-4" />,
    },
    {
      value: 'random',
      labelKey: 'sortByRandom',
      icon: <Shuffle className="w-4 h-4" />,
    },
    {
      value: 'pinyin',
      labelKey: 'sortByPinyin',
      icon: <SortAsc className="w-4 h-4" />,
    },
  ]
  const sortOptions = isSubCategory
    ? allSortOptions.filter((o) => o.value !== 'custom')
    : allSortOptions

  const currentSortOption =
    sortOptions.find((o) => o.value === sortMode) || sortOptions[0]

  // 预置分类
  const presetCategories = [t.brew.friendLinks, t.brew.me]
  const allAddCategories = [...new Set([...presetCategories, ...categories])]

  // 同步编辑模式
  useEffect(() => {
    if (isEditMode && mode !== 'edit') {
      setMode('edit')
    } else if (!isEditMode && mode === 'edit') {
      setMode('default')
    }
  }, [isEditMode, mode])

  // 切换模式
  const handleModeChange = (newMode: ControlMode) => {
    if (newMode === 'edit') {
      onEnterEditMode?.()
    } else if (mode === 'edit') {
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
  const groupedShortcuts = useMemo(
    () => ({
      navigation: BREW_SHORTCUTS.filter((s) => s.category === 'navigation'),
      article: BREW_SHORTCUTS.filter((s) => s.category === 'article'),
      source: BREW_SHORTCUTS.filter((s) => s.category === 'source'),
      other: BREW_SHORTCUTS.filter((s) => s.category === 'other'),
    }),
    [],
  )

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
      const JSZip = await loadJSZip()
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
          } else {
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

      setImportExportSuccess(
        t.brew.exportSuccess.replace('{count}', String(sources.length)),
      )
      setTimeout(setImportExportSuccess, 3000, null)
    } catch (_err) {
      setImportExportError(t.brew.errorExportFailed)
    } finally {
      setImportExportLoading(false)
    }
  }

  // 导入 Brew 格式 (ZIP)
  const handleBrewImportFile = async (
    e: React.ChangeEvent<HTMLInputElement>,
  ) => {
    const file = e.target.files?.[0]
    if (!file) return

    setImportExportLoading(true)
    setImportExportError(null)
    setImportExportSuccess(null)
    setImportProgress({
      step: t.brew.importStepReading,
      current: 0,
      total: 0,
    })

    try {
      setImportProgress({
        step: t.brew.importStepUnzipping,
        current: 0,
        total: 0,
      })
      const JSZip = await loadJSZip()
      const zip = await JSZip.loadAsync(file)

      setImportProgress({
        step: t.brew.importStepParsing,
        current: 0,
        total: 0,
      })
      const manifestFile = zip.file('manifest.json')
      if (!manifestFile) {
        throw new Error(
          t.brew.errorInvalidFormat,
        )
      }

      const manifestContent = await manifestFile.async('string')
      const manifest = JSON.parse(manifestContent) as BrewExportManifest

      if (
        !manifest.version ||
        !manifest.sources ||
        !Array.isArray(manifest.sources)
      ) {
        throw new Error(t.brew.errorInvalidFormat)
      }

      const total = manifest.sources.length
      let imported = 0
      let skipped = 0

      for (let i = 0; i < manifest.sources.length; i++) {
        const source = manifest.sources[i]
        setImportProgress({
          step:
            t.brew.importStepImporting.replace('{name}', source.name),
          current: i + 1,
          total,
        })

        try {
          const exists = sources.some((s) => s.url === source.url)
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
          } else if (source.icon_url) {
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
          const hasExtraFields =
            source.theme_color || source.card_size || source.ai_style_tags
          if (hasExtraFields && newSource?.id) {
            await brewApi.updateSource(newSource.id, {
              theme_color: source.theme_color || undefined,
              card_size: (source.card_size as CardSize) || undefined,
              ai_style_tags: source.ai_style_tags || undefined,
            })
          }

          imported++
        } catch {
          skipped++
        }
      }

      setImportProgress(null)
      setImportExportSuccess(
        t.brew.importSuccess
          .replace('{imported}', String(imported))
          .replace('{skipped}', String(skipped)),
      )
      setTimeout(setImportExportSuccess, 3000, null)
      onSourcesChange?.()
    } catch (err) {
      setImportProgress(null)
      setImportExportError(
        userFacingError(err, t.brew.errorImportFailed),
      )
    } finally {
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
    } catch (err) {
      console.error('OPML export failed:', err)
    } finally {
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
            refreshableCount={
              filteredSources.filter((source) => source.source_type !== 'link')
                .length
            }
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
              exportBrewpack: brewT.exportBrewpack,
              importBrewpack: brewT.importBrewpack,
            }}
          />
        )

      case 'feed':
        if (!feedMode) return null
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
        if (!categoryFeedMode) return null
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

      case 'topic-feed':
        if (!topicFeedMode) return null
        return (
          <TopicFeedMode
            variant={variant}
            topicFeedMode={topicFeedMode}
            hue={topicHue}
            t={{
              backToAllSources: brewT.backToAllSources,
              totalArticles: brewT.totalArticles,
            }}
          />
        )

      case 'starred':
        if (!starredMode) return null
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
        if (!starredMode) return null
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
            onSubmit={
              onAddSource
                ? async (data) => {
                    try {
                      await onAddSource({
                        url: data.url,
                        name: data.name.trim() || undefined,
                        category: data.category.trim() || undefined,
                        icon: data.customIcon || undefined,
                        sourceType: data.sourceType,
                        feedType: data.feedType,
                        notionToken: data.notionToken?.trim() || undefined,
                      })
                      return { success: true }
                    } catch (err) {
                      return {
                        success: false,
                        error: userFacingError(
                          err,
                          t.brew.errorAddFailed,
                        ),
                      }
                    }
                  }
                : undefined
            }
            onDiscover={handleDiscover}
            onImportOpml={
              handleOpmlImport
                ? async (content: string) => {
                    const result = await handleOpmlImport(content)
                    return result || { imported: 0, skipped: 0 }
                  }
                : undefined
            }
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

  return <IslandLayout>{(variant) => renderModeContent(variant)}</IslandLayout>
}

// 导出类型
export type { SortMode }
