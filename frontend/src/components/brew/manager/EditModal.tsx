/**
 * 编辑订阅弹窗组件
 * 使用 Portal 渲染到 document.body，确保全局层叠上下文
 */

import type { MouseEvent } from 'react'
import type { BrewSource, SourceType } from '../../../types/brew'
import {
  LuAlertCircle as AlertCircle,
  LuCheck as Check,
  LuChevronDown as ChevronDown,
  LuEdit3 as Edit3,
  LuEyeOff as EyeOff,
  LuRss as Rss,
  LuSparkles as Sparkles,
  LuTag as Tag,
  LuTrash2 as Trash2,
  LuUpload as Upload,
  LuX as X,
} from '@lib/icons'

import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useState } from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../../contexts/I18nContext'
import { generateStyleTags } from '../../../services/brewApi'
import { Spinner } from '../../Spinner'

// Framer Motion transition 配置常量
const TRANSITION_FAST = { duration: 0.1 } as const
const TRANSITION_NORMAL = { duration: 0.15 } as const

export interface EditModalProps {
  source: BrewSource
  categories: string[]
  onClose: () => void
  onSave: (
    id: number,
    data: {
      name?: string
      category?: string
      update_interval?: number
      enabled?: boolean
      icon?: string
      theme_color?: string | null
      source_type?: SourceType
      ai_style_tags?: string[]
      admin_only?: boolean
    },
  ) => Promise<void>
}

export default function EditModal({
  source,
  categories,
  onClose,
  onSave,
}: EditModalProps) {
  const { t } = useI18n()
  const [name, setName] = useState(source.name)
  // 支持多分类：用逗号分隔的字符串解析为数组
  const [selectedCategories, setSelectedCategories] = useState<string[]>(
    source.category
      ? source.category
          .split(',')
          .map((c) => c.trim())
          .filter(Boolean)
      : [],
  )
  const [newCategory, setNewCategory] = useState('')
  const [updateInterval, setUpdateInterval] = useState(source.update_interval)
  const [enabled, _setEnabled] = useState(source.enabled)
  const [customIcon, setCustomIcon] = useState<string | null>(null)
  const [iconPreview, setIconPreview] = useState<string | null>(source.icon)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showCategoryDropdown, setShowCategoryDropdown] = useState(false)
  const [showIntervalDropdown, setShowIntervalDropdown] = useState(false)
  const [isEditingInfo, setIsEditingInfo] = useState(false) // 名称/分类编辑模式
  const [themeColor, setThemeColor] = useState<string>(
    source.theme_color || '#f97316',
  )
  const [sourceType, _setSourceType] = useState<SourceType>(
    source.source_type || 'rss',
  )
  // AI 风格标签状态
  const [styleTags, setStyleTags] = useState<string[]>(
    source.ai_style_tags || [],
  )
  const [generatingTags, setGeneratingTags] = useState(false)
  // 用户手动输入标签状态
  const [newTagInput, setNewTagInput] = useState('')
  // 仅管理员可见
  const [adminOnly, setAdminOnly] = useState(source.admin_only || false)

  // 判断原始订阅类型（基于 feed_type 和 source_type）
  // feed_type 表示实际的订阅协议：rss/atom/json_feed/notion/rsshub
  // source_type 表示订阅模式：link/rss/brewlia/rsshub
  const isRssHub = source.feed_type === 'rsshub'
  const isNotion = source.feed_type === 'notion'
  const isLink = source.source_type === 'link'
  // 原始 Feed 类型用于显示
  const feedTypeLabel = isLink
    ? t.brew.pureLink
    : isRssHub
      ? 'RSSHub'
      : isNotion
        ? 'Notion'
        : 'RSS'

  // 订阅模式：停止订阅 / 普通订阅 / AI增强订阅
  // 只有 rss/notion/rsshub 类型才有这个选项
  type SubscriptionMode = 'disabled' | 'normal' | 'brewlia'
  const getInitialMode = (): SubscriptionMode => {
    if (!source.enabled) return 'disabled'
    if (source.source_type === 'brewlia') return 'brewlia'
    return 'normal'
  }
  const [subscriptionMode, setSubscriptionMode] =
    useState<SubscriptionMode>(getInitialMode)

  // 预置分类（只有选中这些分类才能添加第二个分类）
  const presetCategories = [t.brew.friendLink, t.brew.categoryMe]
  const allCategories = [...new Set([...presetCategories, ...categories])]

  // 检查是否已选中预置分类
  const hasPresetCategory = selectedCategories.some((cat) =>
    presetCategories.includes(cat),
  )
  // 只有选中预置分类才能选择第二个分类
  const canAddSecondCategory =
    selectedCategories.length === 0 ||
    (selectedCategories.length === 1 && hasPresetCategory)

  // 更新间隔选项
  const intervalOptions = [
    { value: 15, label: t.brew.interval15min },
    { value: 30, label: t.brew.interval30min },
    { value: 60, label: t.brew.interval1hour },
    { value: 120, label: t.brew.interval2hour },
    { value: 360, label: t.brew.interval6hour },
    { value: 720, label: t.brew.interval12hour },
    { value: 1440, label: t.brew.intervalDaily },
  ]

  const currentIntervalLabel =
    intervalOptions.find((o) => o.value === updateInterval)?.label || '1 小时'

  // 处理图标上传
  const handleIconUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    if (!file.type.startsWith('image/')) {
      setError(t.brew.errorSelectImage)
      return
    }

    if (file.size > 500 * 1024) {
      setError(t.brew.errorImageSize)
      return
    }

    const reader = new FileReader()
    reader.onload = (event) => {
      const base64 = event.target?.result as string
      setCustomIcon(base64)
      setIconPreview(base64)
      // 图标变化时清除主题色，让系统重新提取
      setThemeColor('')
      setError(null)
    }
    reader.onerror = () => {
      setError(t.brew.errorImageRead)
    }
    reader.readAsDataURL(file)
  }

  const handleClearIcon = () => {
    setCustomIcon('')
    setIconPreview(null)
    // 清除图标时也清除主题色
    setThemeColor('')
  }

  // 生成 AI 风格标签
  const handleGenerateStyleTags = async () => {
    setGeneratingTags(true)
    setError(null)
    try {
      const result = await generateStyleTags(source.id)
      if (result.success && result.tags) {
        setStyleTags(result.tags)
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : t.brew.errorGenerateStyleTags,
      )
    } finally {
      setGeneratingTags(false)
    }
  }

  // 删除风格标签
  const handleRemoveTag = (tagToRemove: string) => {
    setStyleTags((prev) => prev.filter((tag) => tag !== tagToRemove))
  }

  // 添加用户自定义标签
  const handleAddTag = () => {
    const trimmedTag = newTagInput.trim()
    if (trimmedTag && !styleTags.includes(trimmedTag) && styleTags.length < 3) {
      setStyleTags((prev) => [...prev, trimmedTag])
      setNewTagInput('')
    }
  }

  // 回车添加标签
  const handleTagInputKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      handleAddTag()
    }
  }

  const handleSave = async () => {
    setSaving(true)
    setError(null)
    try {
      // 根据订阅模式确定 enabled 和 source_type
      let finalEnabled = enabled
      let finalSourceType: SourceType = sourceType

      if (!isLink) {
        // rss 或 rsshub 类型
        finalEnabled = subscriptionMode !== 'disabled'
        if (subscriptionMode === 'brewlia') {
          finalSourceType = 'brewlia'
        } else if (subscriptionMode === 'normal') {
          // 恢复原始类型
          finalSourceType = isRssHub ? 'rsshub' : 'rss'
        }
      }

      // 合并分类：已选分类 + 新分类（如果有）
      const finalCategories = [...selectedCategories]
      if (
        newCategory.trim() &&
        !finalCategories.includes(newCategory.trim()) &&
        finalCategories.length < 2
      ) {
        finalCategories.push(newCategory.trim())
      }
      const categoryString =
        finalCategories.length > 0 ? finalCategories.join(', ') : undefined

      await onSave(source.id, {
        name: name.trim() || undefined,
        category: categoryString,
        update_interval: isLink ? 0 : updateInterval,
        enabled: finalEnabled,
        ...(customIcon !== null && { icon: customIcon }),
        // themeColor 为空时传 null，让后端知道需要重新提取
        theme_color: themeColor || null,
        source_type: finalSourceType,
        // 传递标签（纯链接和 Brewlia 模式都支持）
        ai_style_tags: styleTags,
        // 传递仅管理员可见选项
        admin_only: adminOnly,
      })
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : t.brew.errorSaveFailed)
    } finally {
      setSaving(false)
    }
  }

  // 使用 Portal 渲染到 body
  const modalContent = (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-9999 flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm"
      onClick={onClose}
    >
      <motion.div
        initial={{ scale: 0.95, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        exit={{ scale: 0.95, opacity: 0 }}
        className="w-full max-w-3xl glass-surface glass-95 rounded-2xl shadow-2xl overflow-hidden border border-gray-200/50 dark:border-neutral-700/50"
        onClick={(e: MouseEvent) => e.stopPropagation()}
      >
        {/* 头部 */}
        <div className="px-5 py-4 border-b border-gray-200/50 dark:border-neutral-700/50 flex items-center justify-between">
          <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-100">
            {t.brew.editSource}
          </h3>
          <button
            onClick={onClose}
            className="p-2 text-gray-400 hover:text-gray-600 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700"
            title={t.brew.close}
            aria-label={t.brew.close}
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* 左右分栏内容 */}
        <div className="flex flex-col md:flex-row">
          {/* 左侧 - 信息预览区域 */}
          <div className="md:w-56 p-6 bg-gray-50/50 dark:bg-neutral-800/30 border-b md:border-b-0 md:border-r border-gray-200/50 dark:border-neutral-700/50 flex flex-col">
            {/* 图标预览 - 悬浮操作 */}
            <div className="relative group mb-4 w-24 h-24">
              <div className="w-24 h-24 rounded-2xl bg-gray-100 dark:bg-neutral-800 flex items-center justify-center overflow-hidden border-2 border-dashed border-gray-200 dark:border-neutral-600">
                {iconPreview ? (
                  <img
                    src={iconPreview}
                    alt=""
                    className="w-full h-full object-cover"
                  />
                ) : (
                  <Rss className="w-10 h-10 text-gray-400" />
                )}
              </div>
              {/* 悬浮操作层 */}
              <div className="absolute top-0 left-0 w-24 h-24 rounded-2xl bg-black/60 opacity-0 group-hover:opacity-100 transition-opacity flex flex-col p-2 gap-1.5">
                <label
                  className="flex-1 flex items-center justify-center gap-1.5 rounded-lg bg-white/20 hover:bg-white/30 cursor-pointer transition-colors text-white text-xs font-medium"
                  title={t.brew.uploadIconHint}
                >
                  <Upload className="w-4 h-4" />
                  {t.brew.replace}
                  <input
                    type="file"
                    accept="image/*"
                    onChange={handleIconUpload}
                    className="hidden"
                    aria-label={t.brew.uploadIcon}
                  />
                </label>
                {iconPreview && (
                  <button
                    type="button"
                    onClick={handleClearIcon}
                    className="flex-1 flex items-center justify-center gap-1.5 rounded-lg bg-white/20 hover:bg-red-500/80 transition-colors text-white text-xs font-medium"
                    title={t.brew.clearIcon}
                  >
                    <Trash2 className="w-4 h-4" />
                    {t.brew.delete}
                  </button>
                )}
              </div>
            </div>

            {/* 名称和分类 */}
            <div className="mb-4">
              {isEditingInfo ? (
                // 编辑模式
                <div className="space-y-2">
                  <input
                    type="text"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    className="w-full px-3 py-2 text-base rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-2 focus:ring-orange-500/50 text-gray-800 dark:text-gray-100"
                    placeholder={t.brew.sourceName}
                    autoFocus
                  />
                  {/* 多分类选择（最多2个） */}
                  <div className="space-y-2">
                    {/* 已选分类标签 */}
                    {selectedCategories.length > 0 && (
                      <div className="flex flex-wrap gap-1.5">
                        {selectedCategories.map((cat) => (
                          <span
                            key={cat}
                            className="inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-md bg-orange-100 dark:bg-orange-900/40 text-orange-700 dark:text-orange-300"
                          >
                            {cat}
                            <button
                              type="button"
                              onClick={() =>
                                setSelectedCategories((prev) =>
                                  prev.filter((c) => c !== cat),
                                )
                              }
                              className="w-3.5 h-3.5 flex items-center justify-center rounded-full hover:bg-orange-200 dark:hover:bg-orange-800 transition-colors"
                              title={t.brew.removeCategory}
                            >
                              <X className="w-2.5 h-2.5" />
                            </button>
                          </span>
                        ))}
                      </div>
                    )}
                    {/* 分类下拉选择 */}
                    <div className="relative">
                      <button
                        type="button"
                        onClick={() =>
                          setShowCategoryDropdown(!showCategoryDropdown)
                        }
                        disabled={
                          !canAddSecondCategory &&
                          selectedCategories.length >= 1
                        }
                        className={`w-full px-3 py-2 text-sm rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 text-left flex items-center justify-between ${!canAddSecondCategory && selectedCategories.length >= 1 ? 'opacity-50 cursor-not-allowed' : ''}`}
                      >
                        <span className="text-gray-400">
                          {selectedCategories.length >= 2
                            ? t.brew.maxCategories
                            : selectedCategories.length === 1 &&
                                !hasPresetCategory
                              ? t.brew.needFriendLinkFirst
                              : t.brew.addCategory}
                        </span>
                        <ChevronDown className="w-4 h-4 text-gray-400" />
                      </button>
                      <AnimatePresence>
                        {showCategoryDropdown && canAddSecondCategory && (
                          <motion.div
                            initial={{ opacity: 0, y: -4 }}
                            animate={{ opacity: 1, y: 0 }}
                            exit={{ opacity: 0, y: -4 }}
                            transition={TRANSITION_FAST}
                            className="absolute z-50 w-full mt-1 bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 rounded-lg shadow-lg overflow-hidden"
                          >
                            <div className="p-1.5 border-b border-gray-100 dark:border-neutral-700">
                              <input
                                type="text"
                                value={newCategory}
                                onChange={(e) => setNewCategory(e.target.value)}
                                onKeyDown={(e) => {
                                  if (
                                    e.key === 'Enter' &&
                                    newCategory.trim() &&
                                    !selectedCategories.includes(
                                      newCategory.trim(),
                                    )
                                  ) {
                                    setSelectedCategories((prev) => [
                                      ...prev,
                                      newCategory.trim(),
                                    ])
                                    setNewCategory('')
                                    setShowCategoryDropdown(false)
                                  }
                                }}
                                placeholder={t.brew.enterCategoryHint}
                                className="w-full px-2 py-1.5 rounded text-sm bg-gray-50 dark:bg-neutral-900 border-0 focus:outline-none focus:ring-1 focus:ring-orange-500/50 text-gray-800 dark:text-gray-100"
                                onClick={(e) => e.stopPropagation()}
                              />
                            </div>
                            <div className="max-h-32 overflow-y-auto py-0.5">
                              {selectedCategories.length > 0 && (
                                <button
                                  type="button"
                                  onClick={() => {
                                    setSelectedCategories([])
                                    setShowCategoryDropdown(false)
                                  }}
                                  className="w-full px-3 py-2 text-left text-sm hover:bg-gray-50 dark:hover:bg-neutral-700 text-red-500"
                                >
                                  {t.brew.clearAllCategories}
                                </button>
                              )}
                              {allCategories
                                .filter(
                                  (cat) => !selectedCategories.includes(cat),
                                )
                                .map((cat) => (
                                  <button
                                    key={cat}
                                    type="button"
                                    onClick={() => {
                                      setSelectedCategories((prev) => [
                                        ...prev,
                                        cat,
                                      ])
                                      setShowCategoryDropdown(false)
                                    }}
                                    className="w-full px-3 py-2 text-left text-sm hover:bg-gray-50 dark:hover:bg-neutral-700 text-gray-600 dark:text-gray-300"
                                  >
                                    {cat}
                                  </button>
                                ))}
                            </div>
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                    <p className="text-xs text-gray-400">
                      {t.brew.categoryCount.replace(
                        '{count}',
                        String(selectedCategories.length),
                      )}
                      {selectedCategories.length === 1 &&
                        !hasPresetCategory && (
                          <span className="text-orange-500 ml-1">
                            ·{t.brew.selectFriendLinkHint}
                          </span>
                        )}
                    </p>
                  </div>
                  {/* 主题色 */}
                  <div className="flex items-center gap-2 w-full">
                    <input
                      type="color"
                      value={themeColor}
                      onChange={(e) => setThemeColor(e.target.value)}
                      className="w-10 h-10 rounded-lg cursor-pointer border border-gray-200 dark:border-neutral-700 bg-transparent shrink-0 p-0"
                      title={t.brew.selectThemeColor}
                      aria-label={t.brew.selectThemeColor}
                    />
                    <input
                      type="text"
                      value={themeColor}
                      onChange={(e) => setThemeColor(e.target.value)}
                      placeholder="#f97316"
                      className="min-w-0 flex-1 px-3 py-2 text-sm rounded-lg bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-2 focus:ring-orange-500/50 text-gray-800 dark:text-gray-100 font-mono"
                    />
                  </div>
                  <button
                    type="button"
                    onClick={() => setIsEditingInfo(false)}
                    className="text-sm text-orange-500 hover:text-orange-600 py-1"
                  >
                    {t.brew.doneEditing}
                  </button>
                </div>
              ) : (
                // 预览模式
                <div className="group/info">
                  <div className="flex items-center gap-2 mb-1">
                    <div
                      className="w-3 h-3 rounded-full shrink-0"
                      style={{ backgroundColor: themeColor }}
                      title={`${t.brew.themeColor}: ${themeColor}`}
                    />
                    <h4
                      className="font-semibold text-gray-800 dark:text-gray-100 text-xl leading-snug truncate"
                      title={name}
                    >
                      {name}
                    </h4>
                  </div>
                  <p className="text-base text-gray-500 dark:text-gray-400 mt-1">
                    {selectedCategories.length > 0
                      ? selectedCategories.join(' · ')
                      : t.brew.uncategorized}
                  </p>
                  <button
                    type="button"
                    onClick={() => setIsEditingInfo(true)}
                    className="mt-2.5 text-sm text-gray-400 hover:text-orange-500 transition-colors"
                    title={t.brew.editNameCategoryTheme}
                  >
                    <Edit3 className="w-4 h-4 inline mr-1" />
                    {t.brew.edit}
                  </button>
                </div>
              )}
            </div>

            {/* 订阅类型标识（只读显示）- 移动到左侧 */}
            <div className="pt-3">
              <label className="block text-xs font-medium text-gray-400 dark:text-gray-500 mb-1.5">
                {t.brew.sourceType}
              </label>
              <div
                className={`inline-flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-xs font-medium ${
                  isLink
                    ? 'bg-gray-100 dark:bg-neutral-800 text-gray-500 dark:text-gray-400'
                    : isRssHub
                      ? 'bg-teal-50 dark:bg-teal-900/30 text-teal-600 dark:text-teal-400'
                      : isNotion
                        ? 'bg-sky-50 dark:bg-sky-900/30 text-sky-600 dark:text-sky-400'
                        : 'bg-orange-50 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400'
                }`}
              >
                {isLink ? (
                  <span className="w-1.5 h-1.5 rounded-full bg-gray-400" />
                ) : isRssHub ? (
                  <svg
                    className="w-3.5 h-3.5"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                  >
                    <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2v1.93zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39z" />
                  </svg>
                ) : isNotion ? (
                  <svg
                    className="w-3.5 h-3.5"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                  >
                    <path d="M4.459 4.208c.746.606 1.026.56 2.428.466l13.215-.793c.28 0 .047-.28-.046-.326L17.86 1.968c-.42-.326-.98-.7-2.055-.607L3.01 2.295c-.466.046-.56.28-.374.466zm.793 3.08v13.904c0 .747.373 1.027 1.214.98l14.523-.84c.841-.046.935-.56.935-1.167V6.354c0-.606-.233-.933-.748-.886l-15.177.887c-.56.047-.747.327-.747.933zm14.337.745c.093.42 0 .84-.42.888l-.7.14v10.264c-.608.327-1.168.514-1.635.514-.748 0-.935-.234-1.495-.933l-4.577-7.186v6.952l1.448.327s0 .84-1.168.84l-3.22.186c-.094-.187 0-.653.327-.746l.84-.233V9.854L7.822 9.76c-.094-.42.14-1.026.793-1.073l3.454-.233 4.764 7.279v-6.44l-1.215-.14c-.093-.514.28-.886.747-.933zM2.62 1.108l13.496-.934c1.635-.14 2.055-.047 3.08.7l4.25 2.987c.7.513.934.653.934 1.213v16.378c0 1.026-.373 1.634-1.68 1.726l-15.458.934c-.98.047-1.448-.093-1.962-.747l-3.127-4.06c-.56-.747-.793-1.306-.793-1.96V2.788c0-.84.374-1.54 1.26-1.68z" />
                  </svg>
                ) : (
                  <Rss className="w-3.5 h-3.5" />
                )}
                {feedTypeLabel}
              </div>
            </div>

            {/* 订阅地址 */}
            <div className="mt-auto pt-3">
              <label className="block text-xs font-medium text-gray-400 dark:text-gray-500 mb-1.5">
                {t.brew.sourceUrl}
              </label>
              {isRssHub && source.rsshub_route ? (
                // RSSHub 类型：分离显示实例和路由
                <div className="space-y-1.5">
                  <div className="px-2.5 py-1.5 rounded-lg bg-teal-50 dark:bg-teal-900/20 border border-teal-200/50 dark:border-teal-700/50">
                    <div className="text-[9px] text-teal-600 dark:text-teal-400 font-medium mb-0.5">
                      {t.brew.rsshubRoute}
                    </div>
                    <div className="text-[10px] text-gray-600 dark:text-gray-300 font-mono break-all">
                      {source.rsshub_route}
                    </div>
                  </div>
                  <div className="px-2.5 py-1.5 rounded-lg bg-gray-100 dark:bg-neutral-800">
                    <div className="text-[9px] text-gray-500 dark:text-gray-400 font-medium mb-0.5">
                      {t.brew.rsshubInstance}
                    </div>
                    <div className="text-[10px] text-gray-500 dark:text-gray-400 font-mono break-all">
                      {source.url.replace(source.rsshub_route, '')}
                    </div>
                  </div>
                </div>
              ) : (
                // 其他类型：显示完整 URL
                <div className="px-2.5 py-2 rounded-lg bg-gray-100 dark:bg-neutral-800 text-[10px] text-gray-500 dark:text-gray-400 break-all font-mono leading-relaxed max-h-16 overflow-y-auto">
                  {source.url}
                </div>
              )}
            </div>
          </div>

          {/* 右侧 - 设置区域 */}
          <div className="flex-1 p-6 space-y-4 overflow-y-auto max-h-[60vh] md:max-h-[50vh]">
            {/* 订阅模式切换（仅 RSS/Notion/RSSHub 显示） */}
            {!isLink && (
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                  {t.brew.subscriptionMode}
                </label>
                <div className="grid grid-cols-4 gap-1.5">
                  {/* 停止订阅 */}
                  <button
                    type="button"
                    onClick={() => setSubscriptionMode('disabled')}
                    className={`px-2.5 py-2 rounded-lg text-left transition-colors border ${
                      subscriptionMode === 'disabled'
                        ? 'bg-gray-200/80 dark:bg-neutral-700 border-gray-300 dark:border-neutral-600'
                        : 'bg-gray-50 dark:bg-neutral-800/50 border-transparent hover:bg-gray-100 dark:hover:bg-neutral-800'
                    }`}
                  >
                    <div className="flex items-center gap-1.5">
                      <X
                        className={`w-3.5 h-3.5 shrink-0 ${subscriptionMode === 'disabled' ? 'text-gray-700 dark:text-gray-200' : 'text-gray-400 dark:text-gray-500'}`}
                      />
                      <span
                        className={`text-xs font-medium ${subscriptionMode === 'disabled' ? 'text-gray-700 dark:text-gray-200' : 'text-gray-500 dark:text-gray-400'}`}
                      >
                        {t.brew.stop}
                      </span>
                    </div>
                    <p
                      className={`text-[10px] mt-0.5 ${subscriptionMode === 'disabled' ? 'text-gray-600 dark:text-gray-300' : 'text-gray-400 dark:text-gray-500'}`}
                    >
                      {t.brew.pauseFetch}
                    </p>
                  </button>

                  {/* 普通订阅 */}
                  <button
                    type="button"
                    onClick={() => setSubscriptionMode('normal')}
                    className={`px-2.5 py-2 rounded-lg text-left transition-colors border ${
                      subscriptionMode === 'normal'
                        ? isRssHub
                          ? 'bg-teal-50 dark:bg-teal-900/30 border-teal-200 dark:border-teal-700'
                          : isNotion
                            ? 'bg-sky-50 dark:bg-sky-900/30 border-sky-200 dark:border-sky-700'
                            : 'bg-orange-50 dark:bg-orange-900/30 border-orange-200 dark:border-orange-700'
                        : 'bg-gray-50 dark:bg-neutral-800/50 border-transparent hover:bg-gray-100 dark:hover:bg-neutral-800'
                    }`}
                  >
                    <div className="flex items-center gap-1.5">
                      <Rss
                        className={`w-3.5 h-3.5 shrink-0 ${subscriptionMode === 'normal' ? (isRssHub ? 'text-teal-500' : isNotion ? 'text-sky-500' : 'text-orange-500') : 'text-gray-400 dark:text-gray-500'}`}
                      />
                      <span
                        className={`text-xs font-medium ${subscriptionMode === 'normal' ? (isRssHub ? 'text-teal-600 dark:text-teal-400' : isNotion ? 'text-sky-600 dark:text-sky-400' : 'text-orange-600 dark:text-orange-400') : 'text-gray-500 dark:text-gray-400'}`}
                      >
                        {t.brew.subscribe}
                      </span>
                    </div>
                    <p
                      className={`text-[10px] mt-0.5 ${subscriptionMode === 'normal' ? (isRssHub ? 'text-teal-600/70 dark:text-teal-400/70' : isNotion ? 'text-sky-600/70 dark:text-sky-400/70' : 'text-orange-600/70 dark:text-orange-400/70') : 'text-gray-400 dark:text-gray-500'}`}
                    >
                      {t.brew.standardMode}
                    </p>
                  </button>

                  {/* Brewlia AI 订阅 - 2倍宽度 */}
                  <button
                    type="button"
                    onClick={() => setSubscriptionMode('brewlia')}
                    className={`col-span-2 px-2.5 py-2 rounded-lg text-left transition-colors border ${
                      subscriptionMode === 'brewlia'
                        ? 'bg-purple-50 dark:bg-purple-900/30 border-purple-200 dark:border-purple-700'
                        : 'bg-gray-50 dark:bg-neutral-800/50 border-transparent hover:bg-gray-100 dark:hover:bg-neutral-800'
                    }`}
                  >
                    <div className="flex items-center gap-1.5">
                      <Sparkles
                        className={`w-3.5 h-3.5 shrink-0 ${subscriptionMode === 'brewlia' ? 'text-purple-500' : 'text-gray-400 dark:text-gray-500'}`}
                      />
                      <span
                        className={`text-xs font-medium ${subscriptionMode === 'brewlia' ? 'text-purple-600 dark:text-purple-400' : 'text-gray-500 dark:text-gray-400'}`}
                      >
                        Brewlia AI
                      </span>
                    </div>
                    <p
                      className={`text-[10px] mt-0.5 ${subscriptionMode === 'brewlia' ? 'text-purple-600/70 dark:text-purple-400/70' : 'text-gray-400 dark:text-gray-500'}`}
                    >
                      {t.brew.brewliaFeatures}
                    </p>
                  </button>
                </div>
              </div>
            )}

            {/* 更新间隔 - 纯链接类型和停止订阅模式不显示 */}
            {!isLink && subscriptionMode !== 'disabled' && (
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                  {t.brew.updateInterval}
                </label>
                <div className="relative">
                  <button
                    type="button"
                    onClick={() =>
                      setShowIntervalDropdown(!showIntervalDropdown)
                    }
                    className="w-full px-3 py-2.5 rounded-xl bg-gray-50 dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 focus:outline-none focus:ring-2 focus:ring-orange-500/50 text-left flex items-center justify-between text-sm text-gray-800 dark:text-gray-100"
                  >
                    <span>{currentIntervalLabel}</span>
                    <ChevronDown
                      className={`w-4 h-4 text-gray-400 transition-transform ${showIntervalDropdown ? 'rotate-180' : ''}`}
                    />
                  </button>
                  <AnimatePresence>
                    {showIntervalDropdown && (
                      <motion.div
                        initial={{ opacity: 0, y: -8 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -8 }}
                        transition={TRANSITION_NORMAL}
                        className="absolute z-50 w-full mt-1 bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 rounded-xl shadow-lg overflow-hidden py-1"
                      >
                        {intervalOptions.map((opt) => (
                          <button
                            key={opt.value}
                            type="button"
                            onClick={() => {
                              setUpdateInterval(opt.value)
                              setShowIntervalDropdown(false)
                            }}
                            className={`w-full px-3 py-2 text-left text-sm hover:bg-gray-50 dark:hover:bg-neutral-700 flex items-center ${updateInterval === opt.value ? 'text-orange-500 bg-orange-50 dark:bg-orange-900/20' : 'text-gray-600 dark:text-gray-300'}`}
                          >
                            {opt.label}
                            {updateInterval === opt.value && (
                              <Check className="w-4 h-4 ml-auto" />
                            )}
                          </button>
                        ))}
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>
              </div>
            )}

            {/* AI 风格标签 - 仅 Brewlia 模式显示 */}
            {subscriptionMode === 'brewlia' && (
              <div className="p-3 rounded-xl bg-purple-50 dark:bg-purple-900/20 border border-purple-200/50 dark:border-purple-700/50">
                <div className="flex items-center justify-between mb-2">
                  <div className="flex items-center gap-2">
                    <Tag className="w-4 h-4 text-purple-500" />
                    <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                      {t.brew.aiStyleTags}
                    </span>
                  </div>
                  <button
                    type="button"
                    onClick={handleGenerateStyleTags}
                    disabled={generatingTags}
                    className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg bg-purple-500 text-white hover:bg-purple-600 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  >
                    {generatingTags ? (
                      <Spinner size="xs" color="current" />
                    ) : (
                      <>
                        <Sparkles className="w-3 h-3" />
                        {styleTags.length > 0
                          ? t.brew.regenerate
                          : t.brew.generateTags}
                      </>
                    )}
                  </button>
                </div>
                <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
                  {t.brew.styleTagsDesc}
                </p>
                {/* 已有标签展示 */}
                {styleTags.length > 0 ? (
                  <div className="flex flex-wrap gap-1.5">
                    {styleTags.map((tag, index) => (
                      <span
                        key={index}
                        className="inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-md bg-purple-100 dark:bg-purple-800/40 text-purple-700 dark:text-purple-300"
                      >
                        {tag}
                        <button
                          type="button"
                          onClick={() => handleRemoveTag(tag)}
                          className="w-3.5 h-3.5 flex items-center justify-center rounded-full hover:bg-purple-200 dark:hover:bg-purple-700 transition-colors"
                          title={t.brew.deleteTag}
                        >
                          <X className="w-2.5 h-2.5" />
                        </button>
                      </span>
                    ))}
                  </div>
                ) : (
                  <div className="text-xs text-gray-400 dark:text-gray-500 italic">
                    {t.brew.noTagsHint}
                  </div>
                )}
              </div>
            )}

            {/* 自定义标签 - 纯链接类型显示（用户手动输入） */}
            {isLink && (
              <div className="p-3 rounded-xl bg-orange-50 dark:bg-orange-900/20 border border-orange-200/50 dark:border-orange-700/50">
                <div className="flex items-center gap-2 mb-2">
                  <Tag className="w-4 h-4 text-orange-500" />
                  <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                    {t.brew.customTag}
                  </span>
                </div>
                <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
                  {t.brew.customTagDesc}
                </p>
                {/* 标签输入框 */}
                <div className="flex gap-2 mb-2">
                  <input
                    type="text"
                    value={newTagInput}
                    onChange={(e) => setNewTagInput(e.target.value)}
                    onKeyDown={handleTagInputKeyDown}
                    placeholder={t.brew.tagInputPlaceholder}
                    maxLength={10}
                    className="flex-1 px-3 py-1.5 text-sm rounded-lg border border-gray-200 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-gray-700 dark:text-gray-300 focus:outline-none focus:ring-2 focus:ring-orange-500/30"
                    disabled={styleTags.length >= 3}
                  />
                  <button
                    type="button"
                    onClick={handleAddTag}
                    disabled={!newTagInput.trim() || styleTags.length >= 3}
                    className="px-3 py-1.5 text-xs font-medium rounded-lg bg-orange-500 text-white hover:bg-orange-600 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  >
                    {t.brew.addTag}
                  </button>
                </div>
                {/* 已有标签展示 */}
                {styleTags.length > 0 ? (
                  <div className="flex flex-wrap gap-1.5">
                    {styleTags.map((tag, index) => (
                      <span
                        key={index}
                        className="inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-md bg-orange-100 dark:bg-orange-800/40 text-orange-700 dark:text-orange-300"
                      >
                        {tag}
                        <button
                          type="button"
                          onClick={() => handleRemoveTag(tag)}
                          className="w-3.5 h-3.5 flex items-center justify-center rounded-full hover:bg-orange-200 dark:hover:bg-orange-700 transition-colors"
                          title={t.brew.deleteTag}
                        >
                          <X className="w-2.5 h-2.5" />
                        </button>
                      </span>
                    ))}
                  </div>
                ) : (
                  <div className="text-xs text-gray-400 dark:text-gray-500 italic">
                    {t.brew.noCustomTagHint}
                  </div>
                )}
              </div>
            )}

            {/* 错误提示 */}
            {error && (
              <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-sm">
                <AlertCircle className="w-4 h-4 shrink-0" />
                {error}
              </div>
            )}

            {/* 仅管理员可见 */}
            <div className="p-3 rounded-xl bg-gray-50 dark:bg-neutral-800/50 border border-gray-200/50 dark:border-neutral-700/50">
              <label className="flex items-center justify-between cursor-pointer">
                <div className="flex items-center gap-2">
                  <EyeOff className="w-4 h-4 text-gray-500 dark:text-gray-400" />
                  <div>
                    <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                      {t.brew.adminOnlyVisible}
                    </span>
                    <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                      {t.brew.adminOnlyVisibleHint}
                    </p>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => setAdminOnly(!adminOnly)}
                  className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                    adminOnly
                      ? 'bg-orange-500'
                      : 'bg-gray-300 dark:bg-neutral-600'
                  }`}
                >
                  <span
                    className={`inline-block h-4 w-4 transform rounded-full bg-white shadow-sm transition-transform ${
                      adminOnly ? 'translate-x-6' : 'translate-x-1'
                    }`}
                  />
                </button>
              </label>
            </div>
          </div>
        </div>

        {/* 底部按钮 */}
        <div className="px-6 py-4 border-t border-gray-200/50 dark:border-neutral-700/50 flex justify-end gap-3 bg-gray-50/50 dark:bg-neutral-900/50">
          <button
            onClick={onClose}
            className="px-5 py-2.5 rounded-xl text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700 font-medium transition-colors"
          >
            {t.brew.cancel}
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            className="px-5 py-2.5 rounded-xl bg-orange-500 text-white hover:bg-orange-600 disabled:opacity-50 flex items-center gap-2 font-medium shadow-sm transition-colors"
          >
            {saving ? (
              <>
                <Spinner size="sm" color="current" />
                {t.brew.saving}
              </>
            ) : (
              <>
                <Check className="w-4 h-4" />
                {t.brew.saveChanges}
              </>
            )}
          </button>
        </div>
      </motion.div>
    </motion.div>
  )

  // 渲染到 body，将 AnimatePresence 放在 Portal 内部
  return createPortal(
    <AnimatePresence>{modalContent}</AnimatePresence>,
    document.body,
  )
}
