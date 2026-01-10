/**
 * Tapp 商店组件
 * 显示可用的示�?Tapp、远程商�?Tapp 和社�?Tapp
 */

import type { ExampleTapp } from '../examples'
import type { RemoteApp, RemoteStoreSource } from '../services/RemoteStoreService'
import {
  FaArrowUp,
  FaChartBar,
  FaCheck,
  FaCheckCircle,
  FaCog,
  FaDatabase,
  FaDownload,
  FaExclamationTriangle,
  FaFilter,
  FaGamepad,
  FaGlobe,
  FaLink,
  FaMusic,
  FaPlus,
  FaRobot,
  FaSearch,
  FaStar,
  FaSync,
  FaTimes,
  FaTimesCircle,
  FaTools,
  FaTrash,
  FaWrench,
  SiAppstore,
} from '@lib/icons'
import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import { forwardRef, useCallback, useEffect, useMemo, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { hasSessionHint } from '../../utils/sessionDetection'
import { EXAMPLE_TAPPS, getCategoryName } from '../examples'
import { getTappRuntime } from '../runtime'
import {

  RemoteStoreService,

} from '../services/RemoteStoreService'
import { getCategoryGradient } from '../utils/tappColors'
import { TappIcon } from './TappIcon'
import { UninstallConfirmDialog } from './UninstallConfirmDialog'

interface TappStoreProps {
  isOpen: boolean
  onClose: () => void
  onInstalled: () => void
}

/** 应用来源类型 */
type AppSourceType = 'local' | 'remote'

/** 统一的应用列表项 */
interface UnifiedAppItem {
  id: string
  name: string
  version: string
  description: string
  /** 详细描述 */
  longDescription?: string
  author: { name: string, email?: string, url?: string }
  icon?: string
  /** 内联 SVG 图标代码（优先于 icon） */
  iconSvg?: string
  /** 主题色（优先于分类渐变色） */
  themeColor?: string
  category: string
  tags: string[]
  permissions: string[]
  /** 许可证 */
  license?: string
  /** 主页 URL */
  homepage?: string
  /** 仓库 URL */
  repository?: string
  /** 文件大小（字节） */
  size?: number
  /** 是否推荐 */
  featured?: boolean
  /** 是否验证 */
  verified?: boolean
  /** 更新时间 */
  updatedAt?: string
  source: AppSourceType
  /** 本地示例 Tapp 数据 */
  localTapp?: ExampleTapp
  /** 远程应用数据 */
  remoteApp?: RemoteApp & { sourceUrl: string, sourceName: string }
}

/** 分类图标映射 */
const CATEGORY_ICONS: Record<string, React.ReactNode> = {
  widget: <FaCog className="w-4 h-4" />,
  tool: <FaTools className="w-4 h-4" />,
  tools: <FaTools className="w-4 h-4" />,
  platform: <FaGlobe className="w-4 h-4" />,
  demo: <FaRobot className="w-4 h-4" />,
  test: <FaWrench className="w-4 h-4" />,
  game: <FaGamepad className="w-4 h-4" />,
  games: <FaGamepad className="w-4 h-4" />,
  ai: <FaRobot className="w-4 h-4" />,
  productivity: <FaCog className="w-4 h-4" />,
  entertainment: <FaStar className="w-4 h-4" />,
  development: <FaWrench className="w-4 h-4" />,
  social: <FaLink className="w-4 h-4" />,
  media: <FaStar className="w-4 h-4" />,
  utilities: <FaTools className="w-4 h-4" />,
  music: <FaMusic className="w-4 h-4" />,
  visualization: <FaChartBar className="w-4 h-4" />,
  data: <FaDatabase className="w-4 h-4" />,
}

/** 获取应用图标背景样式（优先使用主题色） */
function getAppIconStyle(app: UnifiedAppItem): { className: string, style?: React.CSSProperties } {
  if (app.themeColor) {
    // 使用应用自定义主题色
    return {
      className: 'bg-gradient-to-br',
      style: {
        background: `linear-gradient(to bottom right, ${app.themeColor}, ${app.themeColor}99)`,
      },
    }
  }
  return { className: getCategoryGradient(app.category) }
}

/** 获取应用图标背景色（优先使用主题色） - 兼容旧代码 */
function getAppIconGradient(app: UnifiedAppItem): string {
  if (app.themeColor) {
    // 返回空字符串，实际样式通过 style prop 设置
    return ''
  }
  return getCategoryGradient(app.category)
}

/** 获取各权限等级的数量统计 */
function getPermissionCounts(permissions: string[]): {
  basic: number
  elevated: number
  admin: number
} {
  const adminPermissions = ['component:theme', 'component:agent', 'platform:write', 'platform:register']
  const elevatedPermissions = ['ai:generate', 'ai:analyze', 'ai:chat', 'network:fetch', 'report:write', 'media:control']

  let basic = 0
  let elevated = 0
  let admin = 0

  for (const p of permissions) {
    if (adminPermissions.includes(p)) {
      admin++
    }
    else if (elevatedPermissions.includes(p)) {
      elevated++
    }
    else {
      basic++
    }
  }

  return { basic, elevated, admin }
}

/** 比较版本号，返回 1 表示 v1 > v2，-1 表示 v1 < v2，0 表示相等 */
function compareVersions(v1: string, v2: string): number {
  const parts1 = v1.split('.').map(n => Number.parseInt(n, 10) || 0)
  const parts2 = v2.split('.').map(n => Number.parseInt(n, 10) || 0)
  const maxLen = Math.max(parts1.length, parts2.length)

  for (let i = 0; i < maxLen; i++) {
    const p1 = parts1[i] || 0
    const p2 = parts2[i] || 0
    if (p1 > p2)
      return 1
    if (p1 < p2)
      return -1
  }
  return 0
}

/** 统一的应用卡片 - 支持更新功能 */
const UnifiedAppCard = forwardRef<HTMLDivElement, {
  app: UnifiedAppItem
  isInstalled: boolean
  /** 已安装的版本号（用于比较是否需要更新） */
  installedVersion?: string
  canUninstall: boolean
  onInstall: () => void
  onUpdate?: () => void
  onUninstall?: () => void
  installing: boolean
  updating?: boolean
  animConfig?: ReturnType<typeof useAnimationLevel>
  index?: number
}>(({ app, isInstalled, installedVersion, canUninstall, onInstall, onUpdate, onUninstall, installing, updating, animConfig, index = 0 }, ref) => {
  const [isHovered, setIsHovered] = useState(false)
  const { t } = useI18n()

  // 检查是否有更新可用
  const hasUpdate = isInstalled && installedVersion && compareVersions(app.version, installedVersion) > 0

  // 根据动画级别计算动画属�?
  const animProps = useMemo(() => {
    if (!animConfig || animConfig.level === 'none') {
      return { initial: {}, animate: {}, transition: {} }
    }
    const baseDelay = index * 0.03 * animConfig.durationScale
    return {
      initial: { opacity: 0, y: 20 },
      animate: { opacity: 1, y: 0 },
      transition: {
        delay: baseDelay,
        duration: 0.2 * animConfig.durationScale,
        type: animConfig.spring ? 'spring' : 'tween',
        ...(animConfig.spring ? { stiffness: 300, damping: 25 } : {}),
      },
    }
  }, [animConfig, index])

  const iconStyle = getAppIconStyle(app)
  const permissionCounts = getPermissionCounts(app.permissions)
  const totalPermissions = permissionCounts.basic + permissionCounts.elevated + permissionCounts.admin

  return (
    <motion.div
      ref={ref}
      layout={animConfig?.level !== 'none'}
      initial={animProps.initial}
      animate={animProps.animate}
      transition={animProps.transition}
      whileHover={animConfig?.level !== 'none' ? { y: -4 } : {}}
      whileTap={{ scale: 0.98 }}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      className="group relative aspect-[2/1] rounded-2xl overflow-hidden bg-white/70 dark:bg-black/80 backdrop-blur-xl"
    >
      {/* 动态渐变背�? */}
      <div
        className={`absolute inset-0 opacity-[0.08] transition-opacity duration-500 ${isHovered ? 'opacity-[0.15]' : ''}`}
        style={{ background: `linear-gradient(135deg, var(--color-primary), transparent 60%)` }}
      />

      {/* 装饰光效 - 右上 */}
      <div
        className={`absolute -right-8 -top-8 w-24 h-24 rounded-full blur-2xl transition-all duration-500 opacity-20 ${isHovered ? 'scale-150 opacity-40' : ''}`}
        style={{ background: 'linear-gradient(180deg, var(--color-primary), transparent)' }}
      />

      {/* 装饰光效 - 左下 */}
      <div
        className={`absolute -left-6 -bottom-6 w-16 h-16 rounded-full blur-xl opacity-10 transition-all duration-500 ${isHovered ? 'scale-125 opacity-20' : ''}`}
        style={{ background: 'var(--color-primary)' }}
      />

      {/* 主内容区�? */}
      <div className="relative z-10 h-full flex flex-col p-3">

        {/* 顶部区域：图�?+ 名称 + 安装按钮 */}
        <div className="flex items-start gap-2.5 mb-auto">
          {/* 应用图标 */}
          <div
            className={`w-14 h-14 rounded-xl ${iconStyle.className} flex items-center justify-center text-white shadow-lg relative overflow-hidden flex-shrink-0`}
            style={iconStyle.style}
          >
            <div className="absolute inset-0 bg-gradient-to-br from-white/25 to-transparent" />
            <TappIcon
              icon={app.icon}
              iconSvg={app.iconSvg}
              name={app.name}
              sizeClass="w-8 h-8"
              textSizeClass="text-2xl"
              className="relative z-10"
            />
          </div>

          {/* 名称 + 元信息 */}
          <div className="flex-1 min-w-0 pt-1">
            <h3 className="font-bold text-gray-800 dark:text-gray-100 truncate text-base leading-tight">
              {app.name}
            </h3>
            {/* 作者信息 - 强化显示 */}
            <div className="flex items-center gap-1.5 mt-0.5">
              <span className="text-xs text-gray-600 dark:text-gray-300 font-medium truncate">
                {app.author.name}
              </span>
              <span className="text-gray-300 dark:text-gray-600">·</span>
              <span className={`text-xs ${hasUpdate ? 'text-amber-500 font-medium' : 'text-gray-400 dark:text-gray-500'}`}>
                v
                {app.version}
                {hasUpdate && installedVersion && (
                  <span className="text-gray-400 dark:text-gray-500 font-normal">
                    {' '}
                    (
                    {t.tapp.currentVersion.replace('{version}', installedVersion)}
                    )
                  </span>
                )}
              </span>
              {app.source === 'remote' && (
                <SiAppstore className="w-3 h-3 text-indigo-400" title={t.tapp.remoteStore} />
              )}
              {app.verified && (
                <FaCheckCircle className="w-3 h-3 text-blue-500" title={t.tapp.verified} />
              )}
            </div>
          </div>

          {/* 安装/更新/卸载按钮 */}
          {hasUpdate && onUpdate ? (
            // 有更新可用 - 显示更新按钮
            <motion.button
              onClick={(e: React.MouseEvent) => { e.stopPropagation(); onUpdate() }}
              disabled={updating}
              className="p-2.5 rounded-xl transition-all shadow-sm flex-shrink-0 bg-amber-500/15 text-amber-600 dark:text-amber-400 hover:bg-amber-500/25"
              whileHover={{ scale: 1.1 }}
              whileTap={{ scale: 0.95 }}
              title={t.tapp.update}
            >
              {updating
                ? (
                    <span className="w-4 h-4 border-2 border-current/30 border-t-current rounded-full animate-spin block" />
                  )
                : (
                    <FaArrowUp className="w-4 h-4" />
                  )}
            </motion.button>
          ) : isInstalled && canUninstall && onUninstall
            ? (
                <motion.button
                  onClick={(e: React.MouseEvent) => { e.stopPropagation(); onUninstall() }}
                  className="group/btn p-2.5 rounded-xl transition-all shadow-sm flex-shrink-0 bg-green-500/15 text-green-600 dark:text-green-400 hover:bg-red-500/15 hover:text-red-500 dark:hover:text-red-400"
                  whileHover={{ scale: 1.1 }}
                  whileTap={{ scale: 0.95 }}
                  title={t.tapp.confirmUninstall}
                >
                  <FaCheckCircle className="w-4 h-4 group-hover/btn:hidden" />
                  <FaTrash className="w-4 h-4 hidden group-hover/btn:block" />
                </motion.button>
              )
            : (
                <motion.button
                  onClick={(e: React.MouseEvent) => { e.stopPropagation(); onInstall() }}
                  disabled={isInstalled || installing}
                  className={`p-2.5 rounded-xl transition-all shadow-sm flex-shrink-0 ${
                    isInstalled
                      ? 'bg-green-500/15 text-green-600 dark:text-green-400'
                      : installing
                        ? 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-400'
                        : 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-400 hover:bg-indigo-500/25'
                  }`}
                  whileHover={!isInstalled && !installing ? { scale: 1.1 } : {}}
                  whileTap={!isInstalled && !installing ? { scale: 0.95 } : {}}
                  title={isInstalled ? t.tapp.installed : installing ? t.tapp.installing : t.tapp.install}
                >
                  {installing
                    ? (
                        <span className="w-4 h-4 border-2 border-current/30 border-t-current rounded-full animate-spin block" />
                      )
                    : isInstalled
                      ? (
                          <FaCheckCircle className="w-4 h-4" />
                        )
                      : (
                          <FaDownload className="w-4 h-4" />
                        )}
                </motion.button>
              )}
        </div>

        {/* 底部区域：描�?+ 权限信息 */}
        <div className="mt-auto">
          {/* 应用描述 - 最�?行可滚动 */}
          {app.description && (
            <div className="max-h-[2.5rem] overflow-y-auto mb-2 scrollbar-thin scrollbar-thumb-gray-300 dark:scrollbar-thumb-gray-600 scrollbar-track-transparent">
              <p className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed pr-1">
                {app.description}
              </p>
            </div>
          )}

          {/* 底部信息条：类别 + 权限详情 */}
          <div className="flex items-center justify-between gap-2">
            {/* 类别标签 */}
            <span className="text-[10px] px-1.5 py-0.5 rounded-md bg-black/5 dark:bg-white/10 text-gray-600 dark:text-gray-400 font-medium flex-shrink-0">
              {getCategoryName(app.category)}
            </span>

            {/* 权限详情 - 统计 + 具体权限 */}
            <div className="flex items-center gap-1.5 flex-1 justify-end overflow-hidden">
              {totalPermissions > 0 ? (
                <>
                  {/* 权限统计 */}
                  <div className="flex items-center gap-0.5 text-[9px] flex-shrink-0">
                    {permissionCounts.admin > 0 && (
                      <span className="px-1 py-0.5 rounded bg-red-500/15 text-red-500 dark:text-red-400 font-medium">
                        {permissionCounts.admin}
                      </span>
                    )}
                    {permissionCounts.elevated > 0 && (
                      <span className="px-1 py-0.5 rounded bg-amber-500/15 text-amber-600 dark:text-amber-400 font-medium">
                        {permissionCounts.elevated}
                      </span>
                    )}
                    {permissionCounts.basic > 0 && (
                      <span className="px-1 py-0.5 rounded bg-green-500/15 text-green-600 dark:text-green-400 font-medium">
                        {permissionCounts.basic}
                      </span>
                    )}
                  </div>

                  {/* 分隔�? */}
                  <span className="text-gray-300 dark:text-gray-600">·</span>

                  {/* 具体权限 - 优先显示高等级，最�?�? */}
                  <div className="flex items-center gap-1 text-[9px] overflow-hidden">
                    {(() => {
                      const adminPerms = ['component:theme', 'component:agent', 'platform:write', 'platform:register']
                      const elevatedPerms = ['ai:generate', 'ai:analyze', 'ai:chat', 'network:fetch', 'report:write', 'media:control']

                      // 按优先级排序：管�?> 提升 > 基础
                      const sorted = [...app.permissions].sort((a, b) => {
                        const aLevel = adminPerms.includes(a) ? 2 : elevatedPerms.includes(a) ? 1 : 0
                        const bLevel = adminPerms.includes(b) ? 2 : elevatedPerms.includes(b) ? 1 : 0
                        return bLevel - aLevel
                      })

                      return sorted.slice(0, 2).map((perm, i) => {
                        const isAdmin = adminPerms.includes(perm)
                        const isElevated = elevatedPerms.includes(perm)

                        return (
                          <span
                            key={i}
                            className={`px-1.5 py-0.5 rounded font-medium truncate max-w-[60px] ${
                              isAdmin
                                ? 'bg-red-500/10 text-red-500 dark:text-red-400'
                                : isElevated
                                  ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400'
                                  : 'bg-green-500/10 text-green-600 dark:text-green-400'
                            }`}
                            title={perm}
                          >
                            {perm.split(':')[1] || perm}
                          </span>
                        )
                      })
                    })()}
                  </div>
                </>
              ) : (
                <span className="text-[9px] px-1.5 py-0.5 rounded bg-gray-500/10 text-gray-500 dark:text-gray-400 font-medium">
                  {t.tapp.noPermissions}
                </span>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* 边框效果 */}
      <div className="absolute inset-0 rounded-2xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />

      {/* 悬浮时的高光边框 */}
      <motion.div
        className="absolute inset-0 rounded-2xl pointer-events-none"
        initial={{ opacity: 0 }}
        animate={{ opacity: isHovered ? 1 : 0 }}
        style={{
          boxShadow: 'inset 0 0 0 1px rgba(var(--color-primary-rgb, 99, 102, 241), 0.3)',
        }}
      />
    </motion.div>
  )
})

UnifiedAppCard.displayName = 'UnifiedAppCard'

/** 商店源设置弹�? */
function SourcesSettingsModal({
  isOpen,
  onClose,
  sources,
  onToggle,
  onRemove,
  onAdd,
  onRefresh,
  refreshing,
  isAdmin,
}: {
  isOpen: boolean
  onClose: () => void
  sources: RemoteStoreSource[]
  onToggle: (url: string, enabled: boolean) => void
  onRemove: (url: string) => void
  onAdd: (source: Omit<RemoteStoreSource, 'official'>) => void
  onRefresh: () => void
  refreshing: boolean
  isAdmin: boolean
}) {
  const [showAddForm, setShowAddForm] = useState(false)
  const [newSourceUrl, setNewSourceUrl] = useState('')
  const [newSourceName, setNewSourceName] = useState('')
  const [addError, setAddError] = useState('')
  const { t } = useI18n()

  const handleAdd = () => {
    if (!newSourceUrl.trim() || !newSourceName.trim()) {
      setAddError(t.tapp.fillNameAndUrl)
      return
    }
    try {
      new URL(newSourceUrl)
    }
    catch {
      setAddError(t.tapp.invalidUrl)
      return
    }

    try {
      onAdd({
        name: newSourceName.trim(),
        url: newSourceUrl.trim(),
        enabled: true,
      })
      setNewSourceUrl('')
      setNewSourceName('')
      setShowAddForm(false)
      setAddError('')
    }
    catch (error) {
      setAddError(error instanceof Error ? error.message : t.tapp.addSourceFailed)
    }
  }

  if (!isOpen)
    return null

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/30 backdrop-blur-sm p-4"
      onClick={onClose}
    >
      <motion.div
        initial={{ scale: 0.95, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        exit={{ scale: 0.95, opacity: 0 }}
        className="glass rounded-2xl shadow-xl max-w-lg w-full max-h-[70vh] overflow-hidden flex flex-col"
        onClick={(e: React.MouseEvent) => e.stopPropagation()}
      >
        {/* 头部 */}
        <div className="px-6 py-4 border-b border-gray-200/50 dark:border-neutral-700/50 flex items-center justify-between">
          <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-100 flex items-center gap-2">
            <FaGlobe className="text-blue-500" />
            {t.tapp.sourceManagement}
          </h3>
          <div className="flex items-center gap-2">
            {isAdmin && (
              <button
                onClick={onRefresh}
                disabled={refreshing}
                className={`p-2 rounded-lg transition-colors ${
                  refreshing
                    ? 'text-gray-400 cursor-wait'
                    : 'text-gray-500 hover:bg-gray-100 dark:hover:bg-neutral-700'
                }`}
                title={t.tapp.refreshAllStores}
              >
                <FaSync className={`w-4 h-4 ${refreshing ? 'animate-spin' : ''}`} />
              </button>
            )}
            <button
              onClick={onClose}
              className="p-2 text-gray-500 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
              title={t.tapp.storeClose}
              aria-label={t.tapp.storeClose}
            >
              <FaTimes className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* 内容 */}
        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          {/* 添加按钮（仅管理员） */}
          {isAdmin && !showAddForm && (
            <button
              onClick={() => setShowAddForm(true)}
              className="w-full py-3 border-2 border-dashed border-gray-300 dark:border-neutral-600 rounded-xl text-gray-500 dark:text-gray-400 hover:border-indigo-400 hover:text-indigo-500 transition-colors flex items-center justify-center gap-2"
            >
              <FaPlus className="w-4 h-4" />
              {t.tapp.addSource}
            </button>
          )}

          {/* 添加表单 */}
          {showAddForm && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              className="p-4 bg-gray-50 dark:bg-neutral-800/50 rounded-xl space-y-3"
            >
              <input
                type="text"
                value={newSourceName}
                onChange={e => setNewSourceName(e.target.value)}
                placeholder={t.tapp.sourceName}
                className="w-full px-3 py-2 bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-lg text-sm"
              />
              <input
                type="url"
                value={newSourceUrl}
                onChange={e => setNewSourceUrl(e.target.value)}
                placeholder={t.tapp.sourceUrl}
                className="w-full px-3 py-2 bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-lg text-sm"
              />
              {addError && (
                <p className="text-xs text-red-500">{addError}</p>
              )}
              <div className="flex gap-2">
                <button
                  onClick={handleAdd}
                  className="flex-1 py-2 bg-indigo-600 hover:bg-indigo-700 text-white text-sm font-medium rounded-lg transition-colors"
                >
                  {t.tapp.addSource}
                </button>
                <button
                  onClick={() => {
                    setShowAddForm(false)
                    setAddError('')
                  }}
                  className="flex-1 py-2 bg-gray-200 dark:bg-neutral-700 text-gray-700 dark:text-gray-300 text-sm font-medium rounded-lg transition-colors"
                >
                  {t.tapp.cancel}
                </button>
              </div>
            </motion.div>
          )}

          {/* 商店列表 */}
          <div className="space-y-2">
            {sources.map(source => (
              <div
                key={source.url}
                className="p-4 bg-white/50 dark:bg-neutral-800/50 rounded-xl flex items-center gap-3"
              >
                <div className="text-2xl flex-shrink-0">
                  {source.icon || (source.official ? '🏪' : '📦')}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-gray-800 dark:text-gray-100 text-sm truncate">
                      {source.name}
                    </span>
                    {source.official && (
                      <span className="px-1.5 py-0.5 text-xs bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded">
                        {t.tapp.official}
                      </span>
                    )}
                    {!source.enabled && (
                      <span className="px-1.5 py-0.5 text-xs bg-gray-100 dark:bg-neutral-700 text-gray-500 rounded">
                        {t.tapp.disabled}
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-gray-500 dark:text-gray-400 truncate mt-0.5">
                    {source.url}
                  </p>
                </div>
                <div className="flex items-center gap-2 flex-shrink-0">
                  <button
                    onClick={() => onToggle(source.url, !source.enabled)}
                    className={`p-2 rounded-lg transition-colors ${
                      source.enabled
                        ? 'text-green-500 hover:bg-green-50 dark:hover:bg-green-900/20'
                        : 'text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700'
                    }`}
                    title={source.enabled ? t.tapp.disable : t.tapp.enable}
                  >
                    {source.enabled ? <FaCheck className="w-4 h-4" /> : <FaTimesCircle className="w-4 h-4" />}
                  </button>
                  {!source.official && (
                    <button
                      onClick={() => onRemove(source.url)}
                      className="p-2 text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors"
                      title={t.tapp.deleteSource}
                    >
                      <FaTrash className="w-4 h-4" />
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      </motion.div>
    </motion.div>
  )
}

/**
 * Tapp 商店模态框
 */
export function TappStore({ isOpen, onClose, onInstalled }: TappStoreProps) {
  const { t, format } = useI18n()
  const { isAuthenticated, isAdmin, hasChecked, checkAuth } = useAuth()
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null)
  const [showSourcesSettings, setShowSourcesSettings] = useState(false)
  // 存储已安装应用的信息：id -> { userRole, isTemporary, version }
  const [installedTapps, setInstalledTapps] = useState<Map<string, { userRole: string, isTemporary?: boolean, version: string }>>(new Map())
  const [installing, setInstalling] = useState<string | null>(null)
  const [updating, setUpdating] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // 卸载确认对话框状态
  const [showUninstallDialog, setShowUninstallDialog] = useState(false)
  const [uninstallTargetId, setUninstallTargetId] = useState<string | null>(null)
  const [uninstallTargetName, setUninstallTargetName] = useState('')

  // 动画配置
  const animConfig = useAnimationLevel()

  // 远程应用列表
  const [remoteApps, setRemoteApps] = useState<Array<RemoteApp & { sourceUrl: string, sourceName: string }>>([])
  const [sources, setSources] = useState<RemoteStoreSource[]>([])

  const runtime = getTappRuntime()

  // 已安装应用 ID 集合（兼容性）
  const installedIds = useMemo(() => new Set(installedTapps.keys()), [installedTapps])

  // 加载已安装 Tapp 的辅助函数
  const loadInstalledTapps = useCallback(() => {
    const allTapps = runtime.getAllTapps()
    const tappsMap = new Map<string, { userRole: string, isTemporary?: boolean, version: string }>()
    allTapps.forEach((tapp) => {
      tappsMap.set(tapp.id, {
        userRole: tapp.userRole,
        isTemporary: tapp.isTemporary,
        version: tapp.manifest.version,
      })
    })
    setInstalledTapps(tappsMap)
  }, [runtime])

  // 首次打开时检查认证状态
  useEffect(() => {
    if (isOpen && !hasChecked && hasSessionHint()) {
      checkAuth()
    }
  }, [isOpen, hasChecked, checkAuth])

  // 加载已安装的 Tapp（等待同步完成）
  useEffect(() => {
    let mounted = true

    const initLoad = async () => {
      // 等待 runtime 同步完成
      await runtime.waitForSync()
      if (mounted) {
        loadInstalledTapps()
      }
    }

    initLoad()

    // 监听同步完成事件，以便在后续同步时更新
    const unsubscribe = runtime.on('sync:complete', () => {
      if (mounted) {
        loadInstalledTapps()
      }
    })

    return () => {
      mounted = false
      unsubscribe()
    }
  }, [runtime, loadInstalledTapps])

  // 加载商店源
  useEffect(() => {
    const loadSources = async () => {
      const loadedSources = await RemoteStoreService.getSources()
      setSources(loadedSources)
    }
    loadSources()
  }, [])

  // 加载远程应用
  const loadRemoteApps = useCallback(async (forceRefresh = false) => {
    setLoading(true)
    setError(null)
    try {
      const result = await RemoteStoreService.fetchAllApps(forceRefresh)
      setRemoteApps(result.apps)

      // 检查是否有错误
      const errors = result.sources.filter(s => s.error)
      if (errors.length > 0 && result.apps.length === 0) {
        setError(`无法加载远程商店: ${errors[0].error}`)
      }
    }
    catch (err) {
      setError(err instanceof Error ? err.message : '加载失败')
    }
    finally {
      setLoading(false)
    }
  }, [])

  // 初始加载
  useEffect(() => {
    if (isOpen && remoteApps.length === 0) {
      loadRemoteApps()
    }
  }, [isOpen, loadRemoteApps, remoteApps.length])

  // 转换本地示例为统一格式
  const localApps: UnifiedAppItem[] = EXAMPLE_TAPPS.map(tapp => ({
    id: tapp.manifest.id,
    name: tapp.manifest.name,
    version: tapp.manifest.version,
    description: tapp.manifest.description,
    author: typeof tapp.manifest.author === 'string'
      ? { name: tapp.manifest.author }
      : tapp.manifest.author || { name: 'Unknown' },
    icon: tapp.manifest.icon,
    iconSvg: tapp.manifest.iconSvg,
    themeColor: tapp.manifest.themeColor,
    category: tapp.category,
    tags: tapp.tags,
    permissions: tapp.manifest.permissions,
    source: 'local' as const,
    localTapp: tapp,
  }))

  // 转换远程应用为统一格式
  const remoteAppsUnified: UnifiedAppItem[] = remoteApps.map(app => ({
    id: app.id,
    name: app.name,
    version: app.version,
    description: app.description,
    longDescription: app.long_description,
    author: app.author,
    icon: app.icon,
    iconSvg: app.icon_svg,
    themeColor: app.theme_color,
    category: app.category,
    tags: app.tags || [],
    permissions: app.permissions,
    license: app.license,
    homepage: app.homepage,
    repository: app.repository,
    size: app.size,
    featured: app.featured,
    verified: app.verified,
    updatedAt: app.updated_at,
    source: 'remote' as const,
    remoteApp: app,
  }))

  // 合并应用列表（去重，远程优先�?
  const allApps: UnifiedAppItem[] = [...remoteAppsUnified]
  for (const localApp of localApps) {
    if (!remoteAppsUnified.some(r => r.id === localApp.id)) {
      allApps.push(localApp)
    }
  }

  // 过滤 Tapp
  const filteredApps = allApps.filter((app) => {
    // 搜索过滤
    if (searchQuery) {
      const query = searchQuery.toLowerCase()
      const matchName = app.name.toLowerCase().includes(query)
      const matchDesc = app.description.toLowerCase().includes(query)
      const matchTags = app.tags.some(t => t.toLowerCase().includes(query))
      if (!matchName && !matchDesc && !matchTags)
        return false
    }
    // 分类过滤
    if (selectedCategory === '__installed__') {
      // 已安装分类：只显示已安装的应�?
      return installedIds.has(app.id)
    }
    if (selectedCategory && app.category !== selectedCategory)
      return false
    return true
  })

  // 卸载应用 - 显示确认对话框
  const handleUninstall = useCallback((appId: string) => {
    // 找到对应的应用名称
    const app = filteredApps.find(a => a.id === appId)
    setUninstallTargetId(appId)
    setUninstallTargetName(app?.name || appId)
    setShowUninstallDialog(true)
  }, [filteredApps])

  // 确认卸载
  const handleConfirmUninstall = useCallback(async (keepData: boolean) => {
    if (!uninstallTargetId)
      return
    try {
      await runtime.uninstallTapp(uninstallTargetId, { keepData })
      setInstalledTapps((prev) => {
        const next = new Map(prev)
        next.delete(uninstallTargetId)
        return next
      })
      onInstalled() // 刷新外部列表
      setShowUninstallDialog(false)
      setUninstallTargetId(null)
    }
    catch (error) {
      console.error('Failed to uninstall Tapp:', error)
      alert(`${t.tapp.uninstallFailed}: ${error instanceof Error ? error.message : t.tapp.unknownError}`)
      throw error // 让组件处理 loading 状态
    }
  }, [runtime, onInstalled, uninstallTargetId, t])

  // 取消卸载
  const cancelUninstall = useCallback(() => {
    setShowUninstallDialog(false)
    setUninstallTargetId(null)
  }, [])

  // 安装应用
  const handleInstall = useCallback(async (app: UnifiedAppItem) => {
    // 游客无法安装应用
    if (!isAuthenticated) {
      alert(t.tapp.loginRequiredToInstall)
      return
    }

    setInstalling(app.id)
    try {
      if (app.source === 'local' && app.localTapp) {
        // 安装本地示例
        await runtime.installTapp(app.localTapp.manifest, app.localTapp.code)
      }
      else if (app.source === 'remote' && app.remoteApp) {
        // 安装远程应用 - 使用新的 API，让后端直接下载
        // 找到该应用所在商店源的数据库 ID
        const source = sources.find(s => s.url === app.remoteApp!.sourceUrl)
        if (!source?.id && !source?.url) {
          throw new Error('无法找到商店源')
        }

        // 通过后端 API 从远程商店安装（后端直接下载所有资源）
        const { installFromStore, getTappResources, updateSeparatedCSS } = await import('../services/TappApiService')
        await installFromStore({
          source: source.id ? String(source.id) : source.url,
          tappId: app.id,
          permissions: app.permissions,
        })

        // 🎯 安装完成后，获取资源并生成分离式预编译 CSS（widget.css 和 page.css）
        try {
          const resources = await getTappResources(app.id)
          // 如果没有分离式 CSS，前端生成并更新
          if (!resources.widgetCSS && !resources.pageCSS) {
            const { generateOnDemandTailwindCSS } = await import('../runtime/sandbox/styles')

            // 🎯 生成 Widget 专用 CSS（只包含 Widget 相关源码）
            const widgetSources = [
              resources.code || '',
              resources.styles || '',
              ...Object.values(resources.widgetTemplates || {}),
            ].join('\n')
            const widgetCss = generateOnDemandTailwindCSS(widgetSources)

            // 🎯 生成 Page 专用 CSS（只包含 Page 相关源码）
            const pageSources = [
              resources.code || '',
              resources.styles || '',
              resources.pageTemplate || '',
            ].join('\n')
            const pageCss = generateOnDemandTailwindCSS(pageSources)

            // 保存分离的 CSS
            if (widgetCss || pageCss) {
              await updateSeparatedCSS(app.id, { widgetCss, pageCss })
              // 🎯 清除该 Tapp 的代码缓存，确保下次获取时能获得新的 CSS
              runtime.clearCodeCache(app.id)
            }
          }
        }
        catch (cssError) {
          // CSS 生成失败不影响安装，只记录日志
          console.warn('Failed to generate separated CSS:', cssError)
        }

        // 刷新 runtime 缓存
        await runtime.syncFromBackend(true)
      }

      // 用户安装的都是临时应用
      setInstalledTapps(prev => new Map([...prev, [app.id, { userRole: 'user', isTemporary: true, version: app.version }]]))
      onInstalled()
    }
    catch (error) {
      console.error('Failed to install Tapp:', error)
      alert(`${t.tapp.installFailed}: ${error instanceof Error ? error.message : t.tapp.unknownError}`)
    }
    finally {
      setInstalling(null)
    }
  }, [runtime, onInstalled, sources, t, isAuthenticated])

  // 更新应用
  const handleUpdate = useCallback(async (app: UnifiedAppItem) => {
    // 游客无法更新应用
    if (!isAuthenticated) {
      alert(t.tapp.loginRequiredToInstall)
      return
    }

    // 只支持远程应用更新
    if (app.source !== 'remote' || !app.remoteApp) {
      alert('只支持从商店更新应用')
      return
    }

    setUpdating(app.id)
    try {
      // 找到该应用所在商店源的数据库 ID
      const source = sources.find(s => s.url === app.remoteApp!.sourceUrl)
      if (!source?.id && !source?.url) {
        throw new Error('无法找到商店源')
      }

      // 调用更新 API
      const { updateTappFromStore, getTappResources, updateSeparatedCSS } = await import('../services/TappApiService')
      await updateTappFromStore(app.id, {
        source: source.id ? String(source.id) : source.url,
      })

      // 更新完成后，重新生成分离式 CSS
      try {
        const resources = await getTappResources(app.id)
        if (!resources.widgetCSS && !resources.pageCSS) {
          const { generateOnDemandTailwindCSS } = await import('../runtime/sandbox/styles')

          const widgetSources = [
            resources.code || '',
            resources.styles || '',
            ...Object.values(resources.widgetTemplates || {}),
          ].join('\n')
          const widgetCss = generateOnDemandTailwindCSS(widgetSources)

          const pageSources = [
            resources.code || '',
            resources.styles || '',
            resources.pageTemplate || '',
          ].join('\n')
          const pageCss = generateOnDemandTailwindCSS(pageSources)

          if (widgetCss || pageCss) {
            await updateSeparatedCSS(app.id, { widgetCss, pageCss })
            runtime.clearCodeCache(app.id)
          }
        }
      }
      catch (cssError) {
        console.warn('Failed to generate separated CSS:', cssError)
      }

      // 刷新 runtime 缓存
      await runtime.syncFromBackend(true)

      // 更新本地状态
      setInstalledTapps((prev) => {
        const newMap = new Map(prev)
        const existing = prev.get(app.id)
        if (existing) {
          newMap.set(app.id, { ...existing, version: app.version })
        }
        return newMap
      })
      onInstalled()
    }
    catch (error) {
      console.error('Failed to update Tapp:', error)
      alert(`${t.tapp.updateFailed}: ${error instanceof Error ? error.message : t.tapp.unknownError}`)
    }
    finally {
      setUpdating(null)
    }
  }, [runtime, onInstalled, sources, t, isAuthenticated])

  // 处理商店源操作
  const handleToggleSource = async (url: string, enabled: boolean) => {
    // 通过 URL 找到 source ID
    const source = sources.find(s => s.url === url)
    if (source?.id) {
      try {
        await RemoteStoreService.toggleSource(source.id, enabled)
        const updatedSources = await RemoteStoreService.getSources()
        setSources(updatedSources)
      }
      catch (error) {
        console.error('Failed to toggle source:', error)
        alert(error instanceof Error ? error.message : '操作失败')
      }
    }
  }

  const handleRemoveSource = async (url: string) => {
    // 通过 URL 找到 source ID
    const source = sources.find(s => s.url === url)
    if (source?.id && confirm(t.tapp.confirmDeleteSource)) {
      try {
        await RemoteStoreService.removeSource(source.id)
        const updatedSources = await RemoteStoreService.getSources()
        setSources(updatedSources)
        loadRemoteApps(true)
      }
      catch (error) {
        console.error('Failed to remove source:', error)
        alert(error instanceof Error ? error.message : '删除失败')
      }
    }
  }

  const handleAddSource = async (source: Omit<RemoteStoreSource, 'id' | 'official'>) => {
    try {
      await RemoteStoreService.addSource(source)
      const updatedSources = await RemoteStoreService.getSources()
      setSources(updatedSources)
      loadRemoteApps(true)
    }
    catch (error) {
      console.error('Failed to add source:', error)
      alert(error instanceof Error ? error.message : '添加失败')
    }
  }

  // 获取所有分�?
  const allCategories = new Map<string, { count: number, name: string }>()

  // 统计所有应用的分类
  for (const app of allApps) {
    const existing = allCategories.get(app.category)
    if (existing) {
      allCategories.set(app.category, { ...existing, count: existing.count + 1 })
    }
    else {
      // 使用 getCategoryName 获取分类名称
      const catName = getCategoryName(app.category)
      allCategories.set(app.category, { count: 1, name: catName })
    }
  }

  const categories = Array.from(allCategories.entries()).map(([id, data]) => ({
    id,
    name: data.name,
    count: data.count,
  }))

  // 计算模态框动画属�?
  const modalAnimProps = useMemo(() => {
    if (animConfig.level === 'none') {
      return {
        backdrop: { initial: {}, animate: {}, exit: {} },
        content: { initial: {}, animate: {}, exit: {} },
      }
    }
    return {
      backdrop: {
        initial: { opacity: 0 },
        animate: { opacity: 1 },
        exit: { opacity: 0 },
      },
      content: {
        initial: { scale: 0.95, opacity: 0 },
        animate: { scale: 1, opacity: 1 },
        exit: { scale: 0.95, opacity: 0 },
      },
    }
  }, [animConfig.level])

  return (
    <motion.div
      initial={modalAnimProps.backdrop.initial}
      animate={modalAnimProps.backdrop.animate}
      exit={modalAnimProps.backdrop.exit}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm p-4"
      onClick={onClose}
      data-no-ripple
    >
      <motion.div
        initial={modalAnimProps.content.initial}
        animate={modalAnimProps.content.animate}
        exit={modalAnimProps.content.exit}
        transition={animConfig.level !== 'none'
          ? {
              duration: 0.2 * animConfig.durationScale,
              type: animConfig.spring ? 'spring' : 'tween',
              ...(animConfig.spring ? { stiffness: 300, damping: 25 } : {}),
            }
          : undefined}
        className="bg-white/80 dark:bg-neutral-900/80 backdrop-blur-xl rounded-2xl shadow-xl max-w-6xl w-full max-h-[90vh] overflow-hidden flex flex-col border border-gray-200/50 dark:border-neutral-700/50"
        onClick={(e: React.MouseEvent) => e.stopPropagation()}
      >
        {/* 头部 */}
        <div className="px-6 py-4 border-b border-gray-200/50 dark:border-neutral-700/50">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold text-gray-800 dark:text-gray-100 flex items-center gap-2">
              <SiAppstore className="w-5 h-5" style={{ color: 'var(--bg-accent, rgb(var(--color-accent, 99 102 241)))' }} />
              {t.tapp.storeTitle}
            </h2>
            <div className="flex items-center gap-2">
              {isAdmin && (
                <>
                  <button
                    onClick={() => setShowSourcesSettings(true)}
                    className="p-1.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
                    title={t.tapp.sourceManagement}
                  >
                    <FaCog className="w-4 h-4" />
                  </button>
                  <button
                    onClick={() => loadRemoteApps(true)}
                    disabled={loading}
                    className={`p-1.5 rounded-lg transition-colors ${
                      loading
                        ? 'text-gray-400 cursor-wait'
                        : 'text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700'
                    }`}
                    title={t.tapp.refreshStore}
                  >
                    <FaSync className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
                  </button>
                </>
              )}
              <button
                onClick={onClose}
                title={t.tapp.storeClose}
                aria-label={t.tapp.storeClose}
                className="p-1.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
              >
                <FaTimes className="w-4 h-4" />
              </button>
            </div>
          </div>

          {/* 搜索和分类过�? */}
          <div className="flex flex-col sm:flex-row gap-3">
            <div className="relative flex-1">
              <FaSearch className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 w-4 h-4" />
              <input
                type="text"
                value={searchQuery}
                onChange={e => setSearchQuery(e.target.value)}
                placeholder={t.tapp.searchApps}
                className="w-full pl-10 pr-4 py-2 bg-white/50 dark:bg-neutral-900/50 border border-gray-200/50 dark:border-neutral-700/50 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500"
              />
            </div>
            <div className="flex gap-2 overflow-x-auto pb-1">
              <button
                onClick={() => setSelectedCategory(null)}
                className={`px-3 py-1.5 text-sm rounded-lg whitespace-nowrap transition-colors ${
                  selectedCategory === null
                    ? 'bg-indigo-600 text-white'
                    : 'bg-gray-100 dark:bg-neutral-800 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-neutral-700'
                }`}
              >
                {t.tapp.allApps}
              </button>
              <button
                onClick={() => setSelectedCategory('__installed__')}
                className={`px-3 py-1.5 text-sm rounded-lg whitespace-nowrap transition-colors flex items-center gap-1.5 ${
                  selectedCategory === '__installed__'
                    ? 'bg-green-600 text-white'
                    : 'bg-gray-100 dark:bg-neutral-800 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-neutral-700'
                }`}
              >
                <FaCheckCircle className="w-4 h-4" />
                {t.tapp.installed}
                <span className="opacity-60">
                  (
                  {installedIds.size}
                  )
                </span>
              </button>
              {categories.map(cat => (
                <button
                  key={cat.id}
                  onClick={() => setSelectedCategory(cat.id)}
                  className={`px-3 py-1.5 text-sm rounded-lg whitespace-nowrap transition-colors flex items-center gap-1.5 ${
                    selectedCategory === cat.id
                      ? 'bg-indigo-600 text-white'
                      : 'bg-gray-100 dark:bg-neutral-800 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-neutral-700'
                  }`}
                >
                  {CATEGORY_ICONS[cat.id] || <FaCog className="w-4 h-4" />}
                  {cat.name}
                  <span className="opacity-60">
                    (
                    {cat.count}
                    )
                  </span>
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* 内容区域 */}
        <div className="flex-1 overflow-y-auto p-6">
          {loading && remoteApps.length === 0
            ? (
                <div className="text-center py-12">
                  <span
                    className="w-12 h-12 mx-auto border-4 rounded-full animate-spin block mb-4"
                    style={{
                      borderColor: 'color-mix(in srgb, var(--color-primary) 20%, transparent)',
                      borderTopColor: 'var(--color-primary)',
                    }}
                  />
                  <p className="text-gray-500 dark:text-gray-400">
                    {t.tapp.loadingRemoteApps}
                  </p>
                </div>
              )
            : error && remoteApps.length === 0
              ? (
                  <div className="text-center py-12">
                    <FaExclamationTriangle className="w-12 h-12 mx-auto text-amber-500 mb-4" />
                    <p className="text-gray-600 dark:text-gray-300 mb-2">
                      {error}
                    </p>
                    <button
                      onClick={() => loadRemoteApps(true)}
                      className="px-4 py-2 bg-indigo-600 hover:bg-indigo-700 text-white text-sm font-medium rounded-lg transition-colors"
                    >
                      {t.tapp.retry}
                    </button>
                  </div>
                )
              : filteredApps.length === 0
                ? (
                    <div className="text-center py-12">
                      <FaFilter className="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-4" />
                      <p className="text-gray-500 dark:text-gray-400">
                        {t.tapp.noMatchingApps}
                      </p>
                    </div>
                  )
                : (
                    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                      <AnimatePresence mode="popLayout">
                        {filteredApps.map((app, index) => {
                          const tappInfo = installedTapps.get(app.id)
                          const canUninstall = tappInfo
                            ? (tappInfo.userRole === 'admin' || (tappInfo.userRole === 'user' && tappInfo.isTemporary === true))
                            : false
                          const canUpdate = app.source === 'remote' && app.remoteApp && tappInfo
                          return (
                            <UnifiedAppCard
                              key={app.id}
                              app={app}
                              isInstalled={installedIds.has(app.id)}
                              installedVersion={tappInfo?.version}
                              canUninstall={canUninstall}
                              onInstall={() => handleInstall(app)}
                              onUpdate={canUpdate ? () => handleUpdate(app) : undefined}
                              onUninstall={canUninstall ? () => handleUninstall(app.id) : undefined}
                              installing={installing === app.id}
                              updating={updating === app.id}
                              animConfig={animConfig}
                              index={index}
                            />
                          )
                        })}
                      </AnimatePresence>
                    </div>
                  )}
        </div>

        {/* 底部 */}
        <div className="px-6 py-3 border-t border-gray-200/50 dark:border-neutral-700/50 bg-gray-50/50 dark:bg-neutral-800/50">
          <p className="text-xs text-gray-500 dark:text-gray-400 text-center">
            {format(t.tapp.totalApps, { total: allApps.length })}
            {' '}
            ·
            {format(t.tapp.installedCount, { count: installedIds.size })}
          </p>
        </div>

        {/* 卸载确认对话框 */}
        <UninstallConfirmDialog
          isOpen={showUninstallDialog}
          appName={uninstallTargetName}
          onCancel={cancelUninstall}
          onConfirm={handleConfirmUninstall}
        />

        {/* 商店源设置弹窗 */}
        <AnimatePresence>
          {showSourcesSettings && (
            <SourcesSettingsModal
              isOpen={showSourcesSettings}
              onClose={() => setShowSourcesSettings(false)}
              sources={sources}
              onToggle={handleToggleSource}
              onRemove={handleRemoveSource}
              onAdd={handleAddSource}
              onRefresh={() => loadRemoteApps(true)}
              refreshing={loading}
              isAdmin={isAdmin}
            />
          )}
        </AnimatePresence>
      </motion.div>
    </motion.div>
  )
}

export default TappStore
