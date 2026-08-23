/**
 * AddMode - 添加订阅模式
 * 包含单个添加和 OPML 导入两个标签页
 */

import type { ChangeEvent, SubmitEvent } from 'react'
import type { FeedType } from '../../../../types/brew'
import {
  LuAlertCircle as AlertCircle,
  LuCheck as Check,
  LuChevronDown as ChevronDown,
  LuDownload as Download,
  LuExternalLink as ExternalLink,
  LuFileText as FileText,
  LuFolderOpen as FolderOpen,
  LuLink as Link,
  NotionIcon,
  LuRss as Rss,
  RSSHubIcon,
  LuSparkles as Sparkles,
  LuStar as Star,
  LuUpload as Upload,
  LuX as X,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useRef, useState } from 'react'
import { currentCopy } from '../../../../i18n/localeCopy'
import { userFacingError } from '../../../../utils/userFacingError'
import { Spinner } from '../../../Spinner'
import { ISLAND_GLASS, SPRING_SMOOTH, TRANSITION_QUICK } from './constants'

interface DiscoveredFeed {
  url: string
  autocompleted: boolean
  title: string
  feed_type: string
}

export interface AddModeProps {
  variant: 'mobile' | 'desktop'
  onClose: () => void
  // 分类
  allCategories: string[]
  // 订阅源数量（用于导出）
  sourcesCount: number
  // 回调
  onSubmit?: (data: {
    sourceType: 'rss' | 'brewlia' | 'link' | 'rsshub'
    feedType: FeedType
    url: string
    name: string
    category: string
    customIcon: string | null
    notionToken?: string
    rsshubConfig?: unknown
    enableBrewlia?: boolean
  }) => Promise<{ success: boolean; error?: string; title?: string }>
  onDiscover?: (url: string) => Promise<DiscoveredFeed | null>
  onImportOpml?: (
    content: string,
  ) => Promise<{ imported: number; skipped: number }>
  onExportOpml?: () => void
  // RSSHub 配置组件（可选）
  RSSHubConfigComponent?: React.ComponentType<{
    onConfigChange: (config: unknown, fullUrl: string) => void
    disabled?: boolean
  }>
  // 翻译
  t: {
    sourceTypeLabel: string
    pureLink: string
    notionDesc: string
    rsshubDesc: string
    linkDesc: string
    rssDesc: string
    brewliaShortDesc: string
    brewliaFeatures: string
    disableAI: string
    enableAI: string
    notionUrlLabel: string
    linkUrlLabel: string
    subscriptionUrlLabel: string
    discover: string
    nameLabel: string
    enterName: string
    autoFetch: string
    category: string
    selectCategory: string
    inputNewCategory: string
    noCategory: string
    siteIcon: string
    upload: string
    deleteIcon: string
    deleteCustomIcon: string
    addLink: string
    addBrewlia: string
    addRsshub: string
    addSubscription: string
    dropOpmlHere: string
    supportedFormats: string
    selectOpmlFile: string
    startImport: string
    exportOpml: string
    importResult: string
    skippedCount: string
    singleAdd: string
    closeSearch: string
    close: string
  }
}

export function AddMode({
  variant,
  onClose,
  allCategories,
  sourcesCount,
  onSubmit,
  onDiscover,
  onImportOpml,
  onExportOpml,
  RSSHubConfigComponent,
  t,
}: AddModeProps) {
  const isMobile = variant === 'mobile'

  // 标签页状态
  const [activeTab, setActiveTab] = useState<'single' | 'opml'>('single')

  // 单个添加相关状态
  const [sourceType, setSourceType] = useState<
    'rss' | 'brewlia' | 'link' | 'rsshub'
  >('rss')
  const [feedType, setFeedType] = useState<
    'rss' | 'atom' | 'notion' | 'rsshub'
  >('rss')
  const [url, setUrl] = useState('')
  const [name, setName] = useState('')
  const [category, setCategory] = useState('')
  const [customIcon, setCustomIcon] = useState<string | null>(null)
  const [notionToken, setNotionToken] = useState('')
  const [rsshubConfig, setRsshubConfig] = useState<unknown>(null)
  const [rsshubFullUrl, setRsshubFullUrl] = useState('')
  const [enableBrewliaForRsshub, setEnableBrewliaForRsshub] = useState(false)

  const [showAddCategoryDropdown, setShowAddCategoryDropdown] = useState(false)
  const [loading, setLoading] = useState(false)
  const [discovering, setDiscovering] = useState(false)
  const [discovered, setDiscovered] = useState<DiscoveredFeed | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)

  // OPML 相关状态
  const [opmlContent, setOpmlContent] = useState<string | null>(null)
  const [opmlLoading, setOpmlLoading] = useState(false)
  const [opmlResult, setOpmlResult] = useState<{
    imported: number
    skipped: number
  } | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const [exporting, setExporting] = useState(false)

  const iconInputRef = useRef<HTMLInputElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // 计算显示图标
  const displayIcon =
    customIcon ||
    (discovered?.title
      ? `https://www.google.com/s2/favicons?sz=64&domain=${new URL(url).hostname}`
      : null)

  // 处理探测
  const handleDiscover = async () => {
    if (!url.trim() || !onDiscover) return
    setDiscovering(true)
    setError(null)
    try {
      const result = await onDiscover(url)
      setDiscovered(result)
      if (result) {
        setUrl(result.url)
        setName(result.title)
      }
    } catch (err) {
      setError(
        userFacingError(err, currentCopy().brew.errorDiscoverFailed),
      )
    } finally {
      setDiscovering(false)
    }
  }

  // 处理图标上传
  const handleIconUpload = (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    const reader = new FileReader()
    reader.onload = (event) => {
      setCustomIcon(event.target?.result as string)
    }
    reader.readAsDataURL(file)
  }

  // 处理提交
  const handleSubmit = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault()
    if (!onSubmit) return

    setLoading(true)
    setError(null)
    setSuccess(null)

    try {
      let resolvedUrl = sourceType === 'rsshub' ? rsshubFullUrl : url
      let resolvedName = name
      const shouldAutoDiscover =
        sourceType !== 'link' &&
        sourceType !== 'rsshub' &&
        feedType !== 'notion' &&
        !!onDiscover

      if (shouldAutoDiscover) {
        setDiscovering(true)
        const feed =
          discovered?.url === url ? discovered : await onDiscover(url.trim())
        if (!feed) throw new Error(currentCopy().brew.errorDiscoverFailed)

        resolvedUrl = feed.url
        resolvedName = name.trim() || feed.title
        setUrl(feed.url)
        setName(resolvedName)
        setDiscovered(feed)
        setDiscovering(false)
      }

      const result = await onSubmit({
        sourceType:
          sourceType === 'rsshub' && enableBrewliaForRsshub
            ? 'brewlia'
            : sourceType,
        feedType,
        url: resolvedUrl,
        name: resolvedName,
        category,
        customIcon,
        notionToken: feedType === 'notion' ? notionToken : undefined,
        rsshubConfig: sourceType === 'rsshub' ? rsshubConfig : undefined,
        enableBrewlia: enableBrewliaForRsshub,
      })

      if (result.success) {
        setSuccess(result.title || currentCopy().brew.addSuccess)
        // 重置表单
        setUrl('')
        setName('')
        setCategory('')
        setCustomIcon(null)
        setDiscovered(null)
        setNotionToken('')
      } else {
        setError(
          userFacingError(result.error, currentCopy().brew.errorAddFailed),
        )
      }
    } catch (err) {
      setError(userFacingError(err, currentCopy().brew.errorAddFailed))
    } finally {
      setDiscovering(false)
      setLoading(false)
    }
  }

  // OPML 文件处理
  const handleFileSelect = (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    const reader = new FileReader()
    reader.onload = (event) => {
      setOpmlContent(event.target?.result as string)
    }
    reader.readAsText(file)
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(false)

    const file = e.dataTransfer.files[0]
    if (file && (file.name.endsWith('.opml') || file.name.endsWith('.xml'))) {
      const reader = new FileReader()
      reader.onload = (event) => {
        setOpmlContent(event.target?.result as string)
      }
      reader.readAsText(file)
    }
  }

  const handleImport = async () => {
    if (!opmlContent || !onImportOpml) return

    setOpmlLoading(true)
    setError(null)

    try {
      const result = await onImportOpml(opmlContent)
      setOpmlResult(result)
      setOpmlContent(null)
    } catch (err) {
      setError(userFacingError(err, currentCopy().brew.errorImportFailed))
    } finally {
      setOpmlLoading(false)
    }
  }

  const handleExport = () => {
    if (!onExportOpml) return
    setExporting(true)
    try {
      onExportOpml()
    } finally {
      setExporting(false)
    }
  }

  // 移动端不显示添加模式
  if (isMobile) {
    return null
  }

  return (
    <motion.div
      key="add-bar"
      initial={{ opacity: 0, y: 12, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 12, scale: 0.95 }}
      transition={SPRING_SMOOTH}
      className={`flex flex-col ${ISLAND_GLASS} w-96 max-w-[90vw] overflow-hidden`}
    >
      {/* 内容区域 */}
      <div className="p-4 max-h-[50vh] overflow-y-auto">
        {activeTab === 'single' ? (
          <form onSubmit={handleSubmit} className="space-y-3">
            {/* 来源类型选择 */}
            <div>
              <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1.5">
                {t.sourceTypeLabel}
              </label>
              <div className="grid grid-cols-4 gap-1.5">
                {/* Link */}
                <button
                  type="button"
                  onClick={() => {
                    setSourceType('link')
                    setFeedType('rss')
                    setDiscovered(null)
                    setError(null)
                  }}
                  disabled={loading}
                  className={`flex flex-col items-center gap-1 p-2 rounded-xl border transition-all ${
                    sourceType === 'link'
                      ? 'border-gray-500 bg-gray-50 dark:bg-gray-500/10'
                      : 'border-gray-200 dark:border-neutral-700 hover:border-gray-300'
                  }`}
                >
                  <ExternalLink
                    className={`w-4 h-4 ${sourceType === 'link' ? 'text-gray-600 dark:text-gray-400' : 'text-gray-400'}`}
                  />
                  <span
                    className={`text-[10px] font-medium ${sourceType === 'link' ? 'text-gray-900 dark:text-white' : 'text-gray-500'}`}
                  >
                    {t.pureLink}
                  </span>
                </button>

                {/* RSS */}
                <button
                  type="button"
                  onClick={() => {
                    setSourceType('rss')
                    setFeedType('rss')
                    setError(null)
                  }}
                  disabled={loading}
                  className={`flex flex-col items-center gap-1 p-2 rounded-xl border transition-all ${
                    (sourceType === 'rss' || sourceType === 'brewlia') &&
                    feedType !== 'notion' &&
                    feedType !== 'rsshub'
                      ? 'border-orange-500 bg-orange-50 dark:bg-orange-500/10'
                      : 'border-gray-200 dark:border-neutral-700 hover:border-gray-300'
                  }`}
                >
                  <Rss
                    className={`w-4 h-4 ${(sourceType === 'rss' || sourceType === 'brewlia') && feedType !== 'notion' && feedType !== 'rsshub' ? 'text-orange-600 dark:text-orange-400' : 'text-gray-400'}`}
                  />
                  <span
                    className={`text-[10px] font-medium ${(sourceType === 'rss' || sourceType === 'brewlia') && feedType !== 'notion' && feedType !== 'rsshub' ? 'text-gray-900 dark:text-white' : 'text-gray-500'}`}
                  >
                    RSS
                  </span>
                </button>

                {/* RSSHub */}
                <button
                  type="button"
                  onClick={() => {
                    setSourceType('rsshub')
                    setFeedType('rsshub')
                    setDiscovered(null)
                    setError(null)
                    setUrl('')
                  }}
                  disabled={loading}
                  className={`flex flex-col items-center gap-1 p-2 rounded-xl border transition-all ${
                    sourceType === 'rsshub'
                      ? 'border-teal-500 bg-teal-50 dark:bg-teal-500/10'
                      : 'border-gray-200 dark:border-neutral-700 hover:border-gray-300'
                  }`}
                >
                  <RSSHubIcon
                    className={`w-4 h-4 ${sourceType === 'rsshub' ? 'text-teal-600 dark:text-teal-400' : 'text-gray-400'}`}
                  />
                  <span
                    className={`text-[10px] font-medium ${sourceType === 'rsshub' ? 'text-gray-900 dark:text-white' : 'text-gray-500'}`}
                  >
                    RSSHub
                  </span>
                </button>

                {/* Notion */}
                <button
                  type="button"
                  onClick={() => {
                    setSourceType('rss')
                    setFeedType('notion')
                    setDiscovered(null)
                    setError(null)
                  }}
                  disabled={loading}
                  className={`flex flex-col items-center gap-1 p-2 rounded-xl border transition-all ${
                    feedType === 'notion'
                      ? 'border-slate-600 bg-slate-50 dark:bg-slate-500/10'
                      : 'border-gray-200 dark:border-neutral-700 hover:border-gray-300'
                  }`}
                >
                  <NotionIcon
                    className={`w-4 h-4 ${feedType === 'notion' ? 'text-slate-600 dark:text-slate-400' : 'text-gray-400'}`}
                  />
                  <span
                    className={`text-[10px] font-medium ${feedType === 'notion' ? 'text-gray-900 dark:text-white' : 'text-gray-500'}`}
                  >
                    Notion
                  </span>
                </button>
              </div>
              <p className="mt-1 text-[10px] text-gray-400">
                {feedType === 'notion'
                  ? t.notionDesc
                  : sourceType === 'rsshub'
                    ? t.rsshubDesc
                    : sourceType === 'link'
                      ? t.linkDesc
                      : t.rssDesc}
              </p>
            </div>

            {/* Brewlia AI 增强开关 */}
            {sourceType !== 'link' && sourceType !== 'rsshub' && (
              <div className="flex items-center justify-between px-2.5 py-2 bg-purple-50/50 dark:bg-purple-900/10 border border-purple-200/50 dark:border-purple-800/30 rounded-xl">
                <div className="flex items-center gap-2 min-w-0 flex-1">
                  <Sparkles
                    className={`w-4 h-4 shrink-0 ${sourceType === 'brewlia' ? 'text-purple-500' : 'text-purple-400'}`}
                  />
                  <div className="min-w-0">
                    <span className="text-xs font-medium text-gray-700 dark:text-gray-300">
                      Brewlia AI
                    </span>
                    <p className="text-[10px] text-gray-400 truncate">
                      {t.brewliaShortDesc}
                    </p>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() =>
                    setSourceType(sourceType === 'brewlia' ? 'rss' : 'brewlia')
                  }
                  disabled={loading}
                  title={sourceType === 'brewlia' ? t.disableAI : t.enableAI}
                  className={`relative shrink-0 w-9 h-5 rounded-full transition-colors ${
                    sourceType === 'brewlia'
                      ? 'bg-purple-500'
                      : 'bg-gray-300 dark:bg-neutral-600'
                  }`}
                >
                  <span
                    className={`absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${
                      sourceType === 'brewlia'
                        ? 'translate-x-4'
                        : 'translate-x-0'
                    }`}
                  />
                </button>
              </div>
            )}

            {/* RSSHub 配置 */}
            {sourceType === 'rsshub' && RSSHubConfigComponent && (
              <>
                <RSSHubConfigComponent
                  onConfigChange={(config, fullUrl) => {
                    setRsshubConfig(config)
                    setRsshubFullUrl(fullUrl)
                  }}
                  disabled={loading}
                />
                {/* RSSHub 的 Brewlia AI 增强开关 */}
                <div className="flex items-center justify-between px-2.5 py-2 bg-purple-50/50 dark:bg-purple-900/10 border border-purple-200/50 dark:border-purple-800/30 rounded-xl">
                  <div className="flex items-center gap-2 min-w-0 flex-1">
                    <Sparkles
                      className={`w-4 h-4 shrink-0 ${enableBrewliaForRsshub ? 'text-purple-500' : 'text-purple-400'}`}
                    />
                    <div className="min-w-0">
                      <span className="text-xs font-medium text-gray-700 dark:text-gray-300">
                        Brewlia AI
                      </span>
                      <p className="text-[10px] text-gray-400 truncate">
                        {t.brewliaFeatures}
                      </p>
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() =>
                      setEnableBrewliaForRsshub(!enableBrewliaForRsshub)
                    }
                    disabled={loading}
                    title={enableBrewliaForRsshub ? t.disableAI : t.enableAI}
                    className={`relative shrink-0 w-9 h-5 rounded-full transition-colors ${
                      enableBrewliaForRsshub
                        ? 'bg-purple-500'
                        : 'bg-gray-300 dark:bg-neutral-600'
                    }`}
                  >
                    <span
                      className={`absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${
                        enableBrewliaForRsshub
                          ? 'translate-x-4'
                          : 'translate-x-0'
                      }`}
                    />
                  </button>
                </div>
              </>
            )}

            {/* URL 输入 */}
            {sourceType !== 'rsshub' && (
              <div>
                <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                  {feedType === 'notion'
                    ? t.notionUrlLabel
                    : sourceType === 'link'
                      ? t.linkUrlLabel
                      : t.subscriptionUrlLabel}
                  <span className="text-rose-500"> *</span>
                </label>
                <div className="flex gap-2">
                  <div className="relative flex-1">
                    <Link className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-400" />
                    <input
                      type="url"
                      value={url}
                      onChange={(e) => {
                        setUrl(e.target.value)
                        setDiscovered(null)
                      }}
                      placeholder={
                        feedType === 'notion'
                          ? 'notion://database/xxx'
                          : sourceType === 'link'
                            ? 'https://example.com'
                            : 'https://example.com/feed.xml'
                      }
                      className="w-full pl-8 pr-3 py-2 bg-gray-50/80 dark:bg-neutral-800/50 border border-gray-200/80 dark:border-neutral-700/80 rounded-xl text-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 focus:ring-orange-500/30 transition-all"
                      disabled={loading}
                    />
                  </div>
                  {sourceType !== 'link' && feedType !== 'notion' && (
                    <button
                      type="button"
                      onClick={handleDiscover}
                      disabled={discovering || !url.trim()}
                      className="px-2.5 py-2 bg-gray-100 dark:bg-neutral-700/80 border border-gray-200/80 dark:border-neutral-600/80 rounded-xl text-xs font-medium text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-neutral-600 disabled:opacity-50 transition-colors"
                    >
                      {discovering ? (
                        <Spinner size="xs" color="current" />
                      ) : (
                        t.discover
                      )}
                    </button>
                  )}
                </div>
              </div>
            )}

            {/* Notion Token 输入 */}
            {feedType === 'notion' && (
              <div>
                <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                  Notion Integration Token{' '}
                  <span className="text-rose-500">*</span>
                </label>
                <input
                  type="password"
                  value={notionToken}
                  onChange={(e) => setNotionToken(e.target.value)}
                  placeholder="secret_xxx..."
                  className="w-full px-3 py-2 bg-gray-50/80 dark:bg-neutral-800/50 border border-gray-200/80 dark:border-neutral-700/80 rounded-xl text-xs placeholder:text-gray-400 focus:outline-none focus:ring-2 focus:ring-slate-500/30 transition-all font-mono"
                  disabled={loading}
                />
              </div>
            )}

            {/* 探测结果 */}
            {discovered && sourceType !== 'link' && feedType !== 'notion' && (
              <motion.div
                initial={{ opacity: 0, y: -4 }}
                animate={{ opacity: 1, y: 0 }}
                className="flex items-center gap-2 px-2.5 py-2 bg-emerald-50/80 dark:bg-emerald-950/30 border border-emerald-200/50 dark:border-emerald-800/50 rounded-xl"
              >
                <Check className="w-3.5 h-3.5 text-emerald-500 shrink-0" />
                <span className="text-xs text-emerald-700 dark:text-emerald-300 truncate">
                  {discovered.title}
                </span>
                <span className="text-[10px] px-1 py-0.5 bg-emerald-100 dark:bg-emerald-900/50 text-emerald-600 dark:text-emerald-400 rounded shrink-0">
                  {discovered.feed_type.toUpperCase()}
                </span>
                {sourceType === 'brewlia' && (
                  <span className="text-[10px] px-1 py-0.5 bg-purple-100 dark:bg-purple-900/50 text-purple-600 dark:text-purple-400 rounded shrink-0 flex items-center gap-0.5">
                    <Star className="w-2.5 h-2.5" /> AI
                  </span>
                )}
              </motion.div>
            )}

            {/* 名称和分类 */}
            <div className="grid grid-cols-2 gap-2">
              <div>
                <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                  {t.nameLabel}{' '}
                  {sourceType === 'link' && (
                    <span className="text-rose-500">*</span>
                  )}
                </label>
                <input
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={
                    sourceType === 'link' ? t.enterName : t.autoFetch
                  }
                  className="w-full px-2.5 py-2 bg-gray-50/80 dark:bg-neutral-800/50 border border-gray-200/80 dark:border-neutral-700/80 rounded-xl text-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 focus:ring-orange-500/30 transition-all"
                  disabled={loading}
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                  {t.category}
                </label>
                <div className="relative">
                  <button
                    type="button"
                    onClick={() =>
                      setShowAddCategoryDropdown(!showAddCategoryDropdown)
                    }
                    disabled={loading}
                    className="w-full px-2.5 py-2 bg-gray-50/80 dark:bg-neutral-800/50 border border-gray-200/80 dark:border-neutral-700/80 rounded-xl text-sm text-left flex items-center justify-between focus:outline-none focus:ring-2 focus:ring-orange-500/30 transition-all disabled:opacity-50"
                  >
                    <span
                      className={
                        category
                          ? 'text-gray-800 dark:text-gray-100'
                          : 'text-gray-400'
                      }
                    >
                      {category || t.selectCategory}
                    </span>
                    <ChevronDown
                      className={`w-3.5 h-3.5 text-gray-400 transition-transform ${showAddCategoryDropdown ? 'rotate-180' : ''}`}
                    />
                  </button>
                  <AnimatePresence>
                    {showAddCategoryDropdown && (
                      <motion.div
                        initial={{ opacity: 0, y: -4 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -4 }}
                        transition={TRANSITION_QUICK}
                        className="absolute z-50 w-full mt-1 bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 rounded-xl shadow-lg overflow-hidden"
                      >
                        <div className="p-1.5 border-b border-gray-100 dark:border-neutral-700">
                          <input
                            type="text"
                            value={category}
                            onChange={(e) => setCategory(e.target.value)}
                            placeholder={t.inputNewCategory}
                            className="w-full px-2.5 py-1.5 rounded-lg bg-gray-50 dark:bg-neutral-900 border-0 focus:outline-none focus:ring-2 focus:ring-orange-500/30 text-sm text-gray-800 dark:text-gray-100 placeholder:text-gray-400"
                            onClick={(e) => e.stopPropagation()}
                          />
                        </div>
                        <div className="max-h-32 overflow-y-auto py-0.5">
                          <button
                            type="button"
                            onClick={() => {
                              setCategory('')
                              setShowAddCategoryDropdown(false)
                            }}
                            className={`w-full px-2.5 py-1.5 text-left text-xs hover:bg-gray-50 dark:hover:bg-neutral-700 flex items-center ${!category ? 'text-orange-500 bg-orange-50 dark:bg-orange-900/20' : 'text-gray-600 dark:text-gray-300'}`}
                          >
                            {t.noCategory}
                            {!category && <Check className="w-3 h-3 ml-auto" />}
                          </button>
                          {allCategories.map((cat) => (
                            <button
                              key={cat}
                              type="button"
                              onClick={() => {
                                setCategory(cat)
                                setShowAddCategoryDropdown(false)
                              }}
                              className={`w-full px-2.5 py-1.5 text-left text-xs hover:bg-gray-50 dark:hover:bg-neutral-700 flex items-center ${category === cat ? 'text-orange-500 bg-orange-50 dark:bg-orange-900/20' : 'text-gray-600 dark:text-gray-300'}`}
                            >
                              {cat}
                              {category === cat && (
                                <Check className="w-3 h-3 ml-auto" />
                              )}
                            </button>
                          ))}
                        </div>
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>
              </div>
            </div>

            {/* 自定义图标 */}
            <div>
              <label className="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
                {t.siteIcon}
              </label>
              <div className="flex items-center gap-2">
                <div className="w-8 h-8 rounded-lg bg-gray-100 dark:bg-neutral-800 flex items-center justify-center overflow-hidden border border-dashed border-gray-300 dark:border-neutral-600">
                  {displayIcon ? (
                    <img
                      src={displayIcon}
                      alt=""
                      className="w-full h-full object-cover"
                    />
                  ) : (
                    <Rss className="w-3.5 h-3.5 text-gray-400" />
                  )}
                </div>
                <label className="inline-flex items-center gap-1 px-2 py-1 rounded-lg bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 text-xs font-medium cursor-pointer hover:bg-blue-100 dark:hover:bg-blue-900/50 transition-colors">
                  <Upload className="w-3 h-3" />
                  {t.upload}
                  <input
                    ref={iconInputRef}
                    type="file"
                    accept="image/*"
                    onChange={handleIconUpload}
                    className="hidden"
                    disabled={loading}
                  />
                </label>
                {customIcon && (
                  <button
                    type="button"
                    onClick={() => {
                      setCustomIcon(null)
                      if (iconInputRef.current) iconInputRef.current.value = ''
                    }}
                    className="p-1 text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded transition-colors"
                    title={t.deleteIcon}
                  >
                    <X className="w-3 h-3" />
                  </button>
                )}
              </div>
            </div>

            {/* 错误/成功提示 */}
            {error && (
              <div className="flex items-center gap-1.5 text-xs text-rose-500">
                <AlertCircle className="w-3 h-3" />
                {error}
              </div>
            )}
            {success && (
              <div className="flex items-center gap-1.5 text-xs text-emerald-500">
                <Check className="w-3 h-3" />
                {success}
              </div>
            )}

            {/* 提交按钮 */}
            <button
              type="submit"
              disabled={
                loading ||
                (sourceType === 'rsshub'
                  ? !rsshubFullUrl
                  : !url.trim() || (sourceType === 'link' && !name.trim()))
              }
              className={`w-full flex items-center justify-center gap-1.5 px-3 py-2 rounded-xl text-sm font-medium transition-all text-white ${
                sourceType === 'brewlia'
                  ? 'bg-linear-to-r from-purple-500 to-violet-500 hover:from-purple-600 hover:to-violet-600 disabled:from-purple-300 disabled:to-violet-300'
                  : sourceType === 'rsshub'
                    ? 'bg-linear-to-r from-teal-500 to-cyan-500 hover:from-teal-600 hover:to-cyan-600 disabled:from-teal-300 disabled:to-cyan-300'
                    : sourceType === 'link'
                      ? 'bg-linear-to-r from-gray-500 to-slate-500 hover:from-gray-600 hover:to-slate-600 disabled:from-gray-300 disabled:to-slate-300'
                      : 'bg-linear-to-r from-orange-500 to-amber-500 hover:from-orange-600 hover:to-amber-600 disabled:from-orange-300 disabled:to-amber-300'
              }`}
            >
              {loading && <Spinner size="xs" color="current" />}
              {sourceType === 'link'
                ? t.addLink
                : sourceType === 'brewlia'
                  ? t.addBrewlia
                  : sourceType === 'rsshub'
                    ? t.addRsshub
                    : t.addSubscription}
            </button>
          </form>
        ) : (
          /* OPML 导入/导出 */
          <div className="space-y-3">
            {/* 导入区域 */}
            <div
              onDragOver={(e) => {
                e.preventDefault()
                setDragOver(true)
              }}
              onDragLeave={() => setDragOver(false)}
              onDrop={handleDrop}
              onClick={() => fileInputRef.current?.click()}
              className={`border-2 border-dashed rounded-xl p-4 text-center cursor-pointer transition-all ${
                dragOver
                  ? 'border-violet-400 bg-violet-50/50 dark:bg-violet-950/20'
                  : 'border-gray-200 dark:border-neutral-700 hover:border-violet-300 dark:hover:border-violet-700'
              }`}
            >
              <input
                ref={fileInputRef}
                type="file"
                accept=".opml,.xml"
                onChange={handleFileSelect}
                className="hidden"
                title={t.selectOpmlFile}
              />
              <FolderOpen
                className={`w-8 h-8 mx-auto mb-1.5 ${dragOver ? 'text-violet-400' : 'text-gray-300 dark:text-gray-600'}`}
              />
              <p className="text-xs text-gray-600 dark:text-gray-400">
                {t.dropOpmlHere}
              </p>
              <p className="text-[10px] text-gray-400 mt-0.5">
                {t.supportedFormats}
              </p>
            </div>

            {/* 导入按钮 */}
            {opmlContent && (
              <button
                onClick={handleImport}
                disabled={opmlLoading}
                className="w-full flex items-center justify-center gap-1.5 px-3 py-2 bg-linear-to-r from-violet-500 to-purple-500 hover:from-violet-600 hover:to-purple-600 text-white rounded-xl text-sm font-medium transition-all"
              >
                {opmlLoading ? (
                  <Spinner size="xs" color="current" />
                ) : (
                  <Upload className="w-3.5 h-3.5" />
                )}
                {t.startImport}
              </button>
            )}

            {/* 导出按钮 */}
            <button
              onClick={handleExport}
              disabled={exporting || sourcesCount === 0}
              className="w-full flex items-center justify-center gap-1.5 px-3 py-2 bg-gray-100 dark:bg-neutral-800 hover:bg-gray-200 dark:hover:bg-neutral-700 disabled:opacity-50 text-gray-700 dark:text-gray-300 rounded-xl text-sm font-medium transition-all"
            >
              {exporting ? (
                <Spinner size="xs" color="current" />
              ) : (
                <Download className="w-3.5 h-3.5" />
              )}
              {t.exportOpml.replace('{count}', String(sourcesCount))}
            </button>

            {/* 导入结果 */}
            {opmlResult && (
              <div className="flex items-center gap-1.5 px-2.5 py-2 bg-emerald-50/80 dark:bg-emerald-950/30 border border-emerald-200/50 dark:border-emerald-800/50 rounded-xl text-xs text-emerald-600 dark:text-emerald-400">
                <Check className="w-3.5 h-3.5" />
                {t.importResult
                  .replace('{imported}', String(opmlResult.imported))
                  .replace(
                    '{skipped}',
                    opmlResult.skipped > 0
                      ? t.skippedCount.replace(
                          '{count}',
                          String(opmlResult.skipped),
                        )
                      : '',
                  )}
              </div>
            )}

            {error && (
              <div className="flex items-center gap-1.5 text-xs text-rose-500">
                <AlertCircle className="w-3 h-3" />
                {error}
              </div>
            )}
          </div>
        )}
      </div>

      {/* 底部栏：标签切换 + 关闭按钮 */}
      <div className="flex items-center gap-2 px-3 py-2.5 border-t border-gray-100 dark:border-neutral-800 bg-gray-50/50 dark:bg-neutral-800/30">
        {/* 标签切换 */}
        <button
          onClick={() => setActiveTab('single')}
          className={`flex-1 flex items-center justify-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all ${
            activeTab === 'single'
              ? 'bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400'
              : 'text-gray-500 hover:bg-gray-100 dark:hover:bg-neutral-700'
          }`}
        >
          <Link className="w-3.5 h-3.5" />
          {t.singleAdd}
        </button>
        <button
          onClick={() => setActiveTab('opml')}
          className={`flex-1 flex items-center justify-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all ${
            activeTab === 'opml'
              ? 'bg-violet-100 dark:bg-violet-900/30 text-violet-600 dark:text-violet-400'
              : 'text-gray-500 hover:bg-gray-100 dark:hover:bg-neutral-700'
          }`}
        >
          <FileText className="w-3.5 h-3.5" />
          OPML
        </button>
        {/* 关闭按钮 */}
        <button
          onClick={onClose}
          className="p-1.5 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors ml-auto"
          title={t.close}
        >
          <X className="w-4 h-4" />
        </button>
      </div>
    </motion.div>
  )
}
