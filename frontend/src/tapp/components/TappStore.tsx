/**
 * Tapp 商店组件
 * 显示可用的示�?Tapp、远程商�?Tapp 和社�?Tapp
 */

import type { ExampleTapp } from '../examples'
import type {
  RemoteApp,
  RemoteStoreSource,
} from '../services/RemoteStoreService'
import type { TappCategory, TappPermission } from '../types'
import {
  FaArrowLeft,
  FaArrowUp,
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
  FaLock,
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
  MyriadStoreIcon,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {
  forwardRef,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'

import { Spinner } from '../../components/Spinner'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { sanitizeUrl } from '../../utils/inputSanitizer'
import { hasSessionHint } from '../../utils/sessionDetection'
import { TAPP_ICON_TOKENS } from '../constants/icons'
import { PERMISSION_CONFIG } from '../constants/permissions'
import { EXAMPLE_TAPPS } from '../examples'
import { getTappRuntime } from '../runtime'
import { PERMISSION_LEVELS } from '../runtime/permissionConfig'
import { RemoteStoreService } from '../services/RemoteStoreService'
import { resolveManifestText } from '../utils/manifestLocale'
import {
  normalizeTappCategory,
  TAPP_CATEGORIES,
  TAPP_CATEGORY_I18N_KEYS,
} from '../utils/tappCategories'
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
  author: { name: string; email?: string; url?: string }
  icon?: string
  /** 内联 SVG 图标代码（优先于 icon） */
  iconSvg?: string
  /** 主题色（优先于分类渐变色） */
  themeColor?: string
  category: TappCategory
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
  remoteApp?: RemoteApp & { sourceUrl: string; sourceName: string }
}

/** 分类图标映射 */
const CATEGORY_ICONS: Record<TappCategory, React.ReactNode> = {
  game: <FaGamepad className="w-3.5 h-3.5" />,
  ai: <FaRobot className="w-3.5 h-3.5" />,
  productivity: <FaCog className="w-3.5 h-3.5" />,
  developer: <FaWrench className="w-3.5 h-3.5" />,
  social: <FaLink className="w-3.5 h-3.5" />,
  media: <FaStar className="w-3.5 h-3.5" />,
  utility: <FaTools className="w-3.5 h-3.5" />,
  data: <FaDatabase className="w-3.5 h-3.5" />,
}

/** 分类筛选胶囊按钮，选中态跟随主题色 */
function CategoryPill({
  active,
  icon,
  label,
  count,
  onClick,
}: {
  active: boolean
  icon?: React.ReactNode
  label: string
  count?: number
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      className={`flex h-8 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full px-3 text-xs font-medium transition-colors ${
        active
          ? 'text-white shadow-sm'
          : 'bg-black/5 text-gray-600 hover:bg-black/10 dark:bg-white/10 dark:text-gray-300 dark:hover:bg-white/15'
      }`}
      style={active ? { background: 'var(--color-primary)' } : undefined}
    >
      {icon}
      {label}
      {count !== undefined && (
        <span
          className={
            active ? 'text-white/70' : 'text-gray-400 dark:text-gray-500'
          }
        >
          {count}
        </span>
      )}
    </button>
  )
}

/** 获取应用图标背景样式（优先使用主题色） */
function getAppIconStyle(app: UnifiedAppItem): {
  className: string
  style?: React.CSSProperties
} {
  if (app.themeColor) {
    // 使用应用自定义主题色
    return {
      className: 'bg-linear-to-br',
      style: {
        background: `linear-gradient(to bottom right, ${app.themeColor}, ${app.themeColor}99)`,
      },
    }
  }
  return { className: getCategoryGradient(app.category) }
}

/** 权限级别（与后端一致，未知权限按基础处理） */
function getPermissionLevel(
  permission: string,
): 'basic' | 'elevated' | 'privileged' {
  return PERMISSION_LEVELS[permission as TappPermission] ?? 'basic'
}

/** 权限级别排序权重 */
const LEVEL_ORDER = { basic: 0, elevated: 1, privileged: 2 } as const

/** 权限级别配色 */
const LEVEL_STYLES = {
  basic: 'bg-green-500/10 text-green-600 dark:text-green-400',
  elevated: 'bg-amber-500/10 text-amber-600 dark:text-amber-400',
  privileged: 'bg-red-500/10 text-red-500 dark:text-red-400',
} as const

/** 权限级别标签的 i18n 键 */
const LEVEL_LABEL_KEYS = {
  basic: 'basicPermission',
  elevated: 'elevatedPermission',
  privileged: 'privilegedPermission',
} as const

/** 获取各权限等级的数量统计 */
function getPermissionCounts(permissions: string[]): {
  basic: number
  elevated: number
  admin: number
} {
  let basic = 0
  let elevated = 0
  let admin = 0

  for (const p of permissions) {
    const level = getPermissionLevel(p)
    if (level === 'privileged') {
      admin++
    } else if (level === 'elevated') {
      elevated++
    } else {
      basic++
    }
  }

  return { basic, elevated, admin }
}

/**
 * 列表 ↔ 详情切换时的容器高度过渡。
 * 用 ResizeObserver 跟踪当前视图内容的自然高度，写入外层容器并以 CSS
 * transition 平滑过渡；传入 modalRef 时把高度夹紧到模态框 90vh 内的可用空间
 * （超出部分交给内部滚动）。动画级别为 none 时高度直接落位，不做过渡。
 */
function useHeightTransition({
  animConfig,
  modalRef,
}: {
  animConfig: ReturnType<typeof useAnimationLevel>
  modalRef?: React.RefObject<HTMLDivElement | null>
}) {
  const wrapperRef = useRef<HTMLDivElement | null>(null)
  const contentRef = useRef<HTMLElement | null>(null)
  const observerRef = useRef<ResizeObserver | null>(null)

  const applyHeight = useCallback(() => {
    const wrapper = wrapperRef.current
    const content = contentRef.current
    if (!wrapper || !content) return
    let target = content.offsetHeight
    const modal = modalRef?.current
    if (modal) {
      // 模态框内除本容器以外的固定部分（头部、边框）；同一时刻读取两者，
      // 差值不受高度动画进行中的影响
      const chrome = modal.offsetHeight - wrapper.offsetHeight
      target = Math.min(target, Math.floor(window.innerHeight * 0.9) - chrome)
    }
    wrapper.style.height = `${Math.max(target, 0)}px`
  }, [modalRef])

  // React 提交时子元素 ref 先于父元素 ref 触发，attachContent 首次调用时
  // wrapper 可能尚未就位，因此这里也要落位一次高度
  const attachWrapper = useCallback(
    (el: HTMLDivElement | null) => {
      wrapperRef.current = el
      if (el) applyHeight()
    },
    [applyHeight],
  )

  const attachContent = useCallback(
    (el: HTMLElement | null) => {
      observerRef.current?.disconnect()
      observerRef.current = null
      contentRef.current = el
      if (!el) return
      const observer = new ResizeObserver(applyHeight)
      observer.observe(el)
      observerRef.current = observer
      applyHeight()
    },
    [applyHeight],
  )

  useEffect(() => {
    const wrapper = wrapperRef.current
    if (!wrapper) return
    wrapper.style.transition =
      animConfig.level === 'none'
        ? ''
        : `height ${(0.25 * animConfig.durationScale).toFixed(2)}s cubic-bezier(0.4, 0, 0.2, 1)`
    applyHeight()
  }, [animConfig, applyHeight])

  // 视口尺寸变化时可用空间上限随之变化，需要重新计算
  useEffect(() => {
    if (!modalRef) return
    const onResize = () => applyHeight()
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [modalRef, applyHeight])

  return { attachWrapper, attachContent }
}

/** 比较版本号：返回 1 表示前者较新，-1 表示后者较新，0 表示相等。 */
function compareVersions(left: string, right: string): number {
  const parts1 = left.split('.').map((n) => Number.parseInt(n, 10) || 0)
  const parts2 = right.split('.').map((n) => Number.parseInt(n, 10) || 0)
  const maxLen = Math.max(parts1.length, parts2.length)

  for (let i = 0; i < maxLen; i++) {
    const p1 = parts1[i] || 0
    const p2 = parts2[i] || 0
    if (p1 > p2) return 1
    if (p1 < p2) return -1
  }
  return 0
}

/** 字节数格式化为可读大小 */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB'] as const
  let value = bytes
  let unit = -1
  do {
    value /= 1024
    unit++
  } while (value >= 1024 && unit < units.length - 1)
  return `${value >= 10 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`
}

/** 统一的应用卡片 - 支持更新功能 */
const UnifiedAppCard = forwardRef<
  HTMLDivElement,
  {
    app: UnifiedAppItem
    isInstalled: boolean
    /** 已安装的版本号（用于比较是否需要更新） */
    installedVersion?: string
    canUninstall: boolean
    onInstall: () => void
    onUpdate?: () => void
    onUninstall?: () => void
    /** 点击卡片打开详情视图 */
    onOpen: () => void
    installing: boolean
    updating?: boolean
    animConfig?: ReturnType<typeof useAnimationLevel>
    index?: number
  }
>(
  (
    {
      app,
      isInstalled,
      installedVersion,
      canUninstall,
      onInstall,
      onUpdate,
      onUninstall,
      onOpen,
      installing,
      updating,
      animConfig,
      index = 0,
    },
    ref,
  ) => {
    const [isHovered, setIsHovered] = useState(false)
    const { t } = useI18n()

    // 检查是否有更新可用
    const hasUpdate =
      isInstalled &&
      installedVersion &&
      compareVersions(app.version, installedVersion) > 0

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
    const totalPermissions =
      permissionCounts.basic +
      permissionCounts.elevated +
      permissionCounts.admin

    return (
      <motion.div
        ref={ref}
        layout={animConfig?.level !== 'none'}
        initial={animProps.initial}
        animate={animProps.animate}
        transition={animProps.transition}
        whileHover={animConfig?.level !== 'none' ? { y: -4 } : {}}
        whileTap={{ scale: 0.98 }}
        onClick={onOpen}
        onMouseEnter={() => setIsHovered(true)}
        onMouseLeave={() => setIsHovered(false)}
        className="group relative aspect-2/1 rounded-2xl overflow-hidden glass-surface glass-70 cursor-pointer"
        title={t.tapp.viewDetails}
      >
        {/* 动态渐变背�? */}
        <div
          className={`absolute inset-0 opacity-[0.08] transition-opacity duration-500 ${isHovered ? 'opacity-[0.15]' : ''}`}
          style={{
            background: `linear-gradient(135deg, var(--color-primary), transparent 60%)`,
          }}
        />

        {/* 装饰光效 - 右上 */}
        <div
          className={`absolute -right-8 -top-8 w-24 h-24 rounded-full blur-2xl transition-all duration-500 opacity-20 ${isHovered ? 'scale-150 opacity-40' : ''}`}
          style={{
            background:
              'linear-gradient(180deg, var(--color-primary), transparent)',
          }}
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
              className={`w-14 h-14 rounded-xl ${iconStyle.className} flex items-center justify-center text-white shadow-lg relative overflow-hidden shrink-0`}
              style={iconStyle.style}
            >
              <div className="absolute inset-0 bg-linear-to-br from-white/25 to-transparent" />
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
                <span
                  className={`text-xs ${hasUpdate ? 'text-amber-500 font-medium' : 'text-gray-400 dark:text-gray-500'}`}
                >
                  v{app.version}
                  {hasUpdate && installedVersion && (
                    <span className="text-gray-400 dark:text-gray-500 font-normal">
                      {' '}
                      (
                      {t.tapp.currentVersion.replace(
                        '{version}',
                        installedVersion,
                      )}
                      )
                    </span>
                  )}
                </span>
                {app.source === 'remote' && (
                  <MyriadStoreIcon
                    className="w-3 h-3 text-indigo-400"
                    title={t.tapp.remoteStore}
                  />
                )}
                {app.verified && (
                  <FaCheckCircle
                    className="w-3 h-3 text-blue-500"
                    title={t.tapp.verified}
                  />
                )}
              </div>
            </div>

            {/* 安装/更新/卸载按钮 */}
            {hasUpdate && onUpdate ? (
              // 有更新可用 - 显示更新按钮
              <motion.button
                onClick={(e: React.MouseEvent) => {
                  e.stopPropagation()
                  onUpdate()
                }}
                disabled={updating}
                className="p-2.5 rounded-xl transition-all shadow-sm shrink-0 bg-amber-500/15 text-amber-600 dark:text-amber-400 hover:bg-amber-500/25"
                whileHover={{ scale: 1.1 }}
                whileTap={{ scale: 0.95 }}
                title={t.tapp.update}
              >
                {updating ? (
                  <Spinner size="sm" color="current" />
                ) : (
                  <FaArrowUp className="w-4 h-4" />
                )}
              </motion.button>
            ) : isInstalled && canUninstall && onUninstall ? (
              <motion.button
                onClick={(e: React.MouseEvent) => {
                  e.stopPropagation()
                  onUninstall()
                }}
                className="group/btn p-2.5 rounded-xl transition-all shadow-sm shrink-0 bg-green-500/15 text-green-600 dark:text-green-400 hover:bg-red-500/15 hover:text-red-500 dark:hover:text-red-400"
                whileHover={{ scale: 1.1 }}
                whileTap={{ scale: 0.95 }}
                title={t.tapp.confirmUninstall}
              >
                <FaCheckCircle className="w-4 h-4 group-hover/btn:hidden" />
                <FaTrash className="w-4 h-4 hidden group-hover/btn:block" />
              </motion.button>
            ) : (
              <motion.button
                onClick={(e: React.MouseEvent) => {
                  e.stopPropagation()
                  onInstall()
                }}
                disabled={isInstalled || installing}
                className={`p-2.5 rounded-xl transition-all shadow-sm shrink-0 ${
                  isInstalled
                    ? 'bg-green-500/15 text-green-600 dark:text-green-400'
                    : installing
                      ? 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-400'
                      : 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-400 hover:bg-indigo-500/25'
                }`}
                whileHover={!isInstalled && !installing ? { scale: 1.1 } : {}}
                whileTap={!isInstalled && !installing ? { scale: 0.95 } : {}}
                title={
                  isInstalled
                    ? t.tapp.installed
                    : installing
                      ? t.tapp.installing
                      : t.tapp.install
                }
              >
                {installing ? (
                  <Spinner size="sm" color="current" />
                ) : isInstalled ? (
                  <FaCheckCircle className="w-4 h-4" />
                ) : (
                  <FaDownload className="w-4 h-4" />
                )}
              </motion.button>
            )}
          </div>

          {/* 底部区域：描�?+ 权限信息 */}
          <div className="mt-auto">
            {/* 应用描述 - 最�?行可滚动 */}
            {app.description && (
              <div className="max-h-10 overflow-y-auto mb-2 scrollbar-thin scrollbar-thumb-gray-300 dark:scrollbar-thumb-gray-600 scrollbar-track-transparent">
                <p className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed pr-1">
                  {app.description}
                </p>
              </div>
            )}

            {/* 底部信息条：类别 + 权限详情 */}
            <div className="flex items-center justify-between gap-2">
              {/* 类别标签 */}
              <span className="text-[10px] px-1.5 py-0.5 rounded-md bg-black/5 dark:bg-white/10 text-gray-600 dark:text-gray-400 font-medium shrink-0">
                {t.tapp[TAPP_CATEGORY_I18N_KEYS[app.category]]}
              </span>

              {/* 权限详情 - 仅各等级数量统计 */}
              <div className="flex items-center gap-1.5 flex-1 justify-end overflow-hidden">
                {totalPermissions > 0 ? (
                  <div className="flex items-center gap-0.5 text-[9px] shrink-0">
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
            boxShadow:
              'inset 0 0 0 1px rgba(var(--color-primary-rgb, 99, 102, 241), 0.3)',
          }}
        />
      </motion.div>
    )
  },
)

UnifiedAppCard.displayName = 'UnifiedAppCard'

/** 商店应用详情视图（模态框内的二级页面） */
function AppDetailView({
  app,
  isInstalled,
  installedVersion,
  canUninstall,
  installing,
  updating,
  onInstall,
  onUpdate,
  onUninstall,
}: {
  app: UnifiedAppItem
  isInstalled: boolean
  installedVersion?: string
  canUninstall: boolean
  installing: boolean
  updating: boolean
  onInstall: () => void
  onUpdate: () => void
  onUninstall: () => void
}) {
  const { t } = useI18n()
  const tappStrings = t.tapp as unknown as Record<string, string>
  const iconStyle = getAppIconStyle(app)
  const hasUpdate =
    isInstalled &&
    !!installedVersion &&
    compareVersions(app.version, installedVersion) > 0

  const homepageUrl = app.homepage ? sanitizeUrl(app.homepage) : ''
  const repositoryUrl = app.repository ? sanitizeUrl(app.repository) : ''
  const description = app.longDescription || app.description

  const sortedPermissions = app.permissions.toSorted(
    (a, b) =>
      LEVEL_ORDER[getPermissionLevel(b)] - LEVEL_ORDER[getPermissionLevel(a)],
  )

  // 版本与作者已在顶部信息区展示，这里不再重复
  const metaItems = [
    {
      label: t.tapp.categoryFilter,
      value: t.tapp[TAPP_CATEGORY_I18N_KEYS[app.category]],
    },
    ...(app.size
      ? [{ label: t.tapp.sizeLabel, value: formatSize(app.size) }]
      : []),
    ...(app.license
      ? [{ label: t.tapp.licenseLabel, value: app.license }]
      : []),
    ...(app.updatedAt
      ? [
          {
            label: t.tapp.updatedAtLabel,
            value: new Date(app.updatedAt).toLocaleDateString(),
          },
        ]
      : []),
    {
      label: t.tapp.sourceLabel,
      value:
        app.source === 'remote'
          ? (app.remoteApp?.sourceName ?? t.tapp.remoteStore)
          : t.tapp.builtinExample,
    },
  ]

  const linkClass =
    'flex h-9 items-center gap-1.5 rounded-full bg-black/5 px-4 text-sm font-medium text-gray-600 transition-colors hover:bg-black/10 dark:bg-white/10 dark:text-gray-300 dark:hover:bg-white/15'

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      {/* 应用头部：图标 + 名称 + 操作 */}
      <div className="flex items-start gap-4 sm:gap-5">
        <div
          className={`w-20 h-20 sm:w-24 sm:h-24 rounded-2xl ${iconStyle.className} relative flex shrink-0 items-center justify-center overflow-hidden text-white shadow-lg`}
          style={iconStyle.style}
        >
          <div className="absolute inset-0 bg-linear-to-br from-white/25 to-transparent" />
          <TappIcon
            icon={app.icon}
            iconSvg={app.iconSvg}
            name={app.name}
            sizeClass="w-12 h-12"
            textSizeClass="text-4xl"
            className="relative z-10"
          />
        </div>

        <div className="min-w-0 flex-1">
          <h3 className="text-xl font-bold text-gray-800 dark:text-gray-100 sm:text-2xl">
            {app.name}
          </h3>
          <div className="mt-1 flex flex-wrap items-center gap-1.5 text-sm text-gray-500 dark:text-gray-400">
            <span className="truncate">{app.author.name}</span>
            <span className="text-gray-300 dark:text-gray-600">·</span>
            <span>v{app.version}</span>
            {hasUpdate && installedVersion && (
              <span className="text-amber-500">
                ({t.tapp.currentVersion.replace('{version}', installedVersion)})
              </span>
            )}
            {app.source === 'remote' && (
              <MyriadStoreIcon
                className="w-3.5 h-3.5 text-indigo-400"
                title={t.tapp.remoteStore}
              />
            )}
            {app.verified && (
              <FaCheckCircle
                className="w-3.5 h-3.5 text-blue-500"
                title={t.tapp.verified}
              />
            )}
          </div>

          {/* 操作按钮 + 外部链接 */}
          <div className="mt-3 flex flex-wrap items-center gap-2">
            {hasUpdate ? (
              <button
                onClick={onUpdate}
                disabled={updating}
                className="flex h-9 items-center gap-2 rounded-full bg-amber-500 px-5 text-sm font-semibold text-white shadow-sm transition-opacity hover:opacity-90 disabled:opacity-60"
              >
                {updating ? (
                  <Spinner size="xs" color="current" />
                ) : (
                  <FaArrowUp className="w-3.5 h-3.5" />
                )}
                {t.tapp.update}
              </button>
            ) : isInstalled ? (
              <span className="flex h-9 items-center gap-2 rounded-full bg-green-500/15 px-5 text-sm font-semibold text-green-600 dark:text-green-400">
                <FaCheckCircle className="w-3.5 h-3.5" />
                {t.tapp.installed}
              </span>
            ) : (
              <button
                onClick={onInstall}
                disabled={installing}
                className="flex h-9 items-center gap-2 rounded-full px-5 text-sm font-semibold text-white shadow-sm transition-opacity hover:opacity-90 disabled:opacity-60"
                style={{ background: 'var(--color-primary)' }}
              >
                {installing ? (
                  <Spinner size="xs" color="current" />
                ) : (
                  <FaDownload className="w-3.5 h-3.5" />
                )}
                {installing ? t.tapp.installing : t.tapp.install}
              </button>
            )}
            {isInstalled && canUninstall && (
              <button
                onClick={onUninstall}
                className="flex h-9 items-center gap-1.5 rounded-full bg-red-500/10 px-4 text-sm font-medium text-red-500 transition-colors hover:bg-red-500/20 dark:text-red-400"
              >
                <FaTrash className="w-3.5 h-3.5" />
                {t.tapp.uninstall}
              </button>
            )}
            {homepageUrl && (
              <a
                href={homepageUrl}
                target="_blank"
                rel="noopener noreferrer"
                className={linkClass}
              >
                <FaGlobe className="w-3.5 h-3.5" />
                {t.tapp.homepage}
              </a>
            )}
            {repositoryUrl && (
              <a
                href={repositoryUrl}
                target="_blank"
                rel="noopener noreferrer"
                className={linkClass}
              >
                <FaLink className="w-3.5 h-3.5" />
                {t.tapp.repository}
              </a>
            )}
          </div>
        </div>
      </div>

      {/* 标签 */}
      {app.tags.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {app.tags.map((tag) => (
            <span
              key={tag}
              className="rounded-full bg-black/5 px-2.5 py-1 text-xs text-gray-500 dark:bg-white/10 dark:text-gray-400"
            >
              #{tag}
            </span>
          ))}
        </div>
      )}

      {/* 详细信息 */}
      <section>
        <h4 className="mb-2 text-sm font-semibold text-gray-800 dark:text-gray-100">
          {t.tapp.detailInfo}
        </h4>
        <div className="grid grid-cols-2 gap-x-4 gap-y-3 rounded-2xl bg-black/[0.03] p-4 dark:bg-white/5 sm:grid-cols-3">
          {metaItems.map((item) => (
            <div key={item.label} className="min-w-0">
              <div className="text-[11px] text-gray-400 dark:text-gray-500">
                {item.label}
              </div>
              <div
                className="truncate text-sm font-medium text-gray-700 dark:text-gray-200"
                title={item.value}
              >
                {item.value}
              </div>
            </div>
          ))}
        </div>
      </section>

      {/* 应用介绍 */}
      {description && (
        <section>
          <h4 className="mb-2 text-sm font-semibold text-gray-800 dark:text-gray-100">
            {t.tapp.appDescription}
          </h4>
          <p className="whitespace-pre-wrap text-sm leading-relaxed text-gray-600 dark:text-gray-300">
            {description}
          </p>
        </section>
      )}

      {/* 权限列表 */}
      <section>
        <h4 className="mb-2 text-sm font-semibold text-gray-800 dark:text-gray-100">
          {t.tapp.permissions}
          {sortedPermissions.length > 0 && (
            <span className="ml-1.5 font-normal text-gray-400 dark:text-gray-500">
              {sortedPermissions.length}
            </span>
          )}
        </h4>
        {sortedPermissions.length > 0 ? (
          <div className="divide-y divide-black/5 overflow-hidden rounded-2xl bg-black/[0.03] dark:divide-white/5 dark:bg-white/5">
            {sortedPermissions.map((perm) => {
              const config = PERMISSION_CONFIG[perm as TappPermission]
              const level = getPermissionLevel(perm)
              const Icon = config?.icon ?? FaLock
              const label = config
                ? (tappStrings[config.labelKey] ?? perm)
                : perm
              const desc = config
                ? tappStrings[config.descriptionKey]
                : undefined
              return (
                <div key={perm} className="flex items-center gap-3 px-4 py-3">
                  <span
                    className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg ${LEVEL_STYLES[level]}`}
                  >
                    <Icon className="w-3.5 h-3.5" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="text-sm font-medium text-gray-700 dark:text-gray-200">
                      {label}
                    </div>
                    {desc && (
                      <div className="truncate text-xs text-gray-400 dark:text-gray-500">
                        {desc}
                      </div>
                    )}
                  </div>
                  <span
                    className={`shrink-0 rounded-md px-1.5 py-0.5 text-[10px] font-medium ${LEVEL_STYLES[level]}`}
                  >
                    {tappStrings[LEVEL_LABEL_KEYS[level]]}
                  </span>
                </div>
              )
            })}
          </div>
        ) : (
          <p className="text-sm text-gray-400 dark:text-gray-500">
            {t.tapp.noPermissions}
          </p>
        )}
      </section>
    </div>
  )
}

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
      // eslint-disable-next-line no-new
      new URL(newSourceUrl)
    } catch {
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
    } catch (error) {
      setAddError(
        error instanceof Error ? error.message : t.tapp.addSourceFailed,
      )
    }
  }

  if (!isOpen) return null

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="surface-dialog-backdrop fixed inset-0 z-60 flex items-center justify-center bg-black/30 backdrop-blur-sm p-4"
      onClick={onClose}
    >
      <motion.div
        initial={{ scale: 0.95, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        exit={{ scale: 0.95, opacity: 0 }}
        className="surface-dialog glass rounded-2xl shadow-xl max-w-lg w-full max-h-[70vh] overflow-hidden flex flex-col"
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
                {refreshing ? (
                  <Spinner size="sm" color="current" />
                ) : (
                  <FaSync className="w-4 h-4" />
                )}
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
                onChange={(e) => setNewSourceName(e.target.value)}
                placeholder={t.tapp.sourceName}
                className="w-full px-3 py-2 bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-lg text-sm"
              />
              <input
                type="url"
                value={newSourceUrl}
                onChange={(e) => setNewSourceUrl(e.target.value)}
                placeholder={t.tapp.sourceUrl}
                className="w-full px-3 py-2 bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-lg text-sm"
              />
              {addError && <p className="text-xs text-red-500">{addError}</p>}
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
            {sources.map((source) => (
              <div
                key={source.url}
                className="p-4 bg-white/50 dark:bg-neutral-800/50 rounded-xl flex items-center gap-3"
              >
                <div className="w-9 h-9 shrink-0 flex items-center justify-center">
                  <TappIcon
                    icon={
                      source.icon ||
                      (source.official
                        ? TAPP_ICON_TOKENS.store
                        : TAPP_ICON_TOKENS.package)
                    }
                    name={source.name}
                    sizeClass="w-8 h-8"
                    textSizeClass="text-2xl"
                  />
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
                <div className="flex items-center gap-2 shrink-0">
                  <button
                    onClick={() => onToggle(source.url, !source.enabled)}
                    className={`p-2 rounded-lg transition-colors ${
                      source.enabled
                        ? 'text-green-500 hover:bg-green-50 dark:hover:bg-green-900/20'
                        : 'text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700'
                    }`}
                    title={source.enabled ? t.tapp.disable : t.tapp.enable}
                  >
                    {source.enabled ? (
                      <FaCheck className="w-4 h-4" />
                    ) : (
                      <FaTimesCircle className="w-4 h-4" />
                    )}
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
  const { t, format, locale } = useI18n()
  const { isAuthenticated, isAdmin, hasChecked, checkAuth } = useAuth()
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<
    TappCategory | '__installed__' | null
  >(null)
  // 详情视图当前展示的应用（null 表示列表视图）
  const [detailApp, setDetailApp] = useState<UnifiedAppItem | null>(null)
  const [showSourcesSettings, setShowSourcesSettings] = useState(false)
  // 存储已安装应用的信息：id -> { userRole, isTemporary, version }
  const [installedTapps, setInstalledTapps] = useState<
    Map<string, { userRole: string; isTemporary?: boolean; version: string }>
  >(new Map())
  const [installing, setInstalling] = useState<string | null>(null)
  const [updating, setUpdating] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // 卸载确认对话框状态
  const [showUninstallDialog, setShowUninstallDialog] = useState(false)
  const [uninstallTargetId, setUninstallTargetId] = useState<string | null>(
    null,
  )
  const [uninstallTargetName, setUninstallTargetName] = useState('')

  // 动画配置
  const animConfig = useAnimationLevel()

  // 远程应用列表
  const [remoteApps, setRemoteApps] = useState<
    Array<RemoteApp & { sourceUrl: string; sourceName: string }>
  >([])
  const [sources, setSources] = useState<RemoteStoreSource[]>([])

  const runtime = getTappRuntime()

  // 已安装应用 ID 集合（兼容性）
  const installedIds = useMemo(
    () => new Set(installedTapps.keys()),
    [installedTapps],
  )

  // 加载已安装 Tapp 的辅助函数
  const loadInstalledTapps = useCallback(() => {
    const allTapps = runtime.getAllTapps()
    const tappsMap = new Map<
      string,
      { userRole: string; isTemporary?: boolean; version: string }
    >()
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
      const errors = result.sources.filter((s) => s.error)
      if (errors.length > 0 && result.apps.length === 0) {
        setError(`无法加载远程商店: ${errors[0].error}`)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : '加载失败')
    } finally {
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
  const localApps: UnifiedAppItem[] = EXAMPLE_TAPPS.map((tapp) => {
    const text = resolveManifestText(tapp.manifest, locale)
    return {
      id: tapp.manifest.id,
      name: text.name,
      version: tapp.manifest.version,
      description: text.description || '',
      author: tapp.manifest.author || { name: 'Unknown' },
      icon: tapp.manifest.icon,
      iconSvg: tapp.manifest.iconSvg,
      themeColor: tapp.manifest.themeColor,
      category: tapp.manifest.category,
      tags: tapp.tags,
      permissions: tapp.manifest.permissions,
      source: 'local' as const,
      localTapp: tapp,
    }
  })

  // 转换远程应用为统一格式（name/description 按宿主语言解析 locales）
  const remoteAppsUnified: UnifiedAppItem[] = remoteApps.map((app) => {
    const text = resolveManifestText(
      {
        name: app.name,
        description: app.description,
        locales: app.locales,
      },
      locale,
    )
    return {
      id: app.id,
      name: text.name,
      version: app.version,
      description: text.description || '',
      longDescription: app.long_description,
      author: app.author,
      icon: app.icon,
      iconSvg: app.icon_svg,
      themeColor: app.theme_color,
      category: normalizeTappCategory(app.category),
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
    }
  })

  // 合并应用列表（去重，远程优先）
  const allApps: UnifiedAppItem[] = [...remoteAppsUnified]
  for (const localApp of localApps) {
    if (!remoteAppsUnified.some((r) => r.id === localApp.id)) {
      allApps.push(localApp)
    }
  }

  // 过滤 Tapp
  const filteredApps = allApps.filter((app) => {
    // 搜索过滤：解析后文案 + 远程原始 name/description/locales 均可命中
    if (searchQuery) {
      const query = searchQuery.toLowerCase()
      const matchName = app.name.toLowerCase().includes(query)
      const matchDesc = app.description.toLowerCase().includes(query)
      const matchTags = app.tags.some((t) => t.toLowerCase().includes(query))
      const remote = app.remoteApp
      const matchRaw =
        !!remote &&
        (remote.name.toLowerCase().includes(query) ||
          remote.description.toLowerCase().includes(query) ||
          Object.values(remote.locales ?? {}).some(
            (entry) =>
              (entry.name?.toLowerCase().includes(query) ?? false) ||
              (entry.description?.toLowerCase().includes(query) ?? false),
          ))
      if (!matchName && !matchDesc && !matchTags && !matchRaw) return false
    }
    // 分类过滤
    if (selectedCategory === '__installed__') {
      // 已安装分类：只显示已安装的应�?
      return installedIds.has(app.id)
    }
    if (selectedCategory && app.category !== selectedCategory) return false
    return true
  })

  // 卸载应用 - 显示确认对话框
  const handleUninstall = useCallback(
    (appId: string) => {
      // 找到对应的应用名称
      const app = filteredApps.find((a) => a.id === appId)
      setUninstallTargetId(appId)
      setUninstallTargetName(app?.name || appId)
      setShowUninstallDialog(true)
    },
    [filteredApps],
  )

  // 确认卸载
  const handleConfirmUninstall = useCallback(
    async (keepData: boolean) => {
      if (!uninstallTargetId) return
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
      } catch (error) {
        console.error('Failed to uninstall Tapp:', error)
        alert(
          `${t.tapp.uninstallFailed}: ${error instanceof Error ? error.message : t.tapp.unknownError}`,
        )
        throw error // 让组件处理 loading 状态
      }
    },
    [runtime, onInstalled, uninstallTargetId, t],
  )

  // 取消卸载
  const cancelUninstall = useCallback(() => {
    setShowUninstallDialog(false)
    setUninstallTargetId(null)
  }, [])

  // 安装应用
  const handleInstall = useCallback(
    async (app: UnifiedAppItem) => {
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
        } else if (app.source === 'remote' && app.remoteApp) {
          // 安装远程应用 - 使用新的 API，让后端直接下载
          // 找到该应用所在商店源的数据库 ID
          const source = sources.find((s) => s.url === app.remoteApp!.sourceUrl)
          if (!source?.id && !source?.url) {
            throw new Error('无法找到商店源')
          }

          // 通过后端 API 从远程商店安装（后端直接下载所有资源）
          const { installFromStore } =
            await import('../services/TappApiService')
          await installFromStore({
            source: source.id ? String(source.id) : source.url,
            tappId: app.id,
            permissions: app.permissions,
          })

          // 刷新 runtime 缓存
          await runtime.syncFromBackend(true)
        }

        // 安装 owner 与临时性必须使用后端同步结果；管理员安装属于规范公共
        // owner，不能在这里硬编码成普通用户临时副本。
        const installed = runtime.getTapp(app.id)
        if (installed) {
          setInstalledTapps(
            (prev) =>
              new Map([
                ...prev,
                [
                  app.id,
                  {
                    userRole: installed.userRole,
                    isTemporary: installed.isTemporary,
                    version: installed.manifest.version,
                  },
                ],
              ]),
          )
        }
        onInstalled()
      } catch (error) {
        console.error('Failed to install Tapp:', error)
        alert(
          `${t.tapp.installFailed}: ${error instanceof Error ? error.message : t.tapp.unknownError}`,
        )
      } finally {
        setInstalling(null)
      }
    },
    [runtime, onInstalled, sources, t, isAuthenticated],
  )

  // 更新应用
  const handleUpdate = useCallback(
    async (app: UnifiedAppItem) => {
      // 游客无法更新应用
      if (!isAuthenticated) {
        alert(t.tapp.loginRequiredToInstall)
        return
      }

      setUpdating(app.id)
      try {
        if (app.source === 'local' && app.localTapp) {
          const { updateTappFromCode } =
            await import('../services/TappApiService')
          await updateTappFromCode(app.localTapp.manifest, app.localTapp.code)
        } else if (app.source === 'remote' && app.remoteApp) {
          // 找到该应用所在商店源的数据库 ID
          const source = sources.find((s) => s.url === app.remoteApp!.sourceUrl)
          if (!source?.id && !source?.url) {
            throw new Error('无法找到商店源')
          }

          // 调用更新 API
          const { updateTappFromStore } =
            await import('../services/TappApiService')
          await updateTappFromStore(app.id, {
            source: source.id ? String(source.id) : source.url,
          })
          runtime.clearCodeCache(app.id)
        } else {
          throw new Error('Unsupported update source')
        }

        // 刷新清单并重建仍在运行的 Page/Widget/headless 实例。
        await runtime.refreshTapp(app.id)

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
      } catch (error) {
        console.error('Failed to update Tapp:', error)
        alert(
          `${t.tapp.updateFailed}: ${error instanceof Error ? error.message : t.tapp.unknownError}`,
        )
      } finally {
        setUpdating(null)
      }
    },
    [runtime, onInstalled, sources, t, isAuthenticated],
  )

  // 处理商店源操作
  const handleToggleSource = async (url: string, enabled: boolean) => {
    // 通过 URL 找到 source ID
    const source = sources.find((s) => s.url === url)
    if (source?.id) {
      try {
        await RemoteStoreService.toggleSource(source.id, enabled)
        const updatedSources = await RemoteStoreService.getSources()
        setSources(updatedSources)
      } catch (error) {
        console.error('Failed to toggle source:', error)
        alert(error instanceof Error ? error.message : '操作失败')
      }
    }
  }

  const handleRemoveSource = async (url: string) => {
    // 通过 URL 找到 source ID
    const source = sources.find((s) => s.url === url)
    if (source?.id && confirm(t.tapp.confirmDeleteSource)) {
      try {
        await RemoteStoreService.removeSource(source.id)
        const updatedSources = await RemoteStoreService.getSources()
        setSources(updatedSources)
        loadRemoteApps(true)
      } catch (error) {
        console.error('Failed to remove source:', error)
        alert(error instanceof Error ? error.message : '删除失败')
      }
    }
  }

  const handleAddSource = async (
    source: Omit<RemoteStoreSource, 'id' | 'official'>,
  ) => {
    try {
      await RemoteStoreService.addSource(source)
      const updatedSources = await RemoteStoreService.getSources()
      setSources(updatedSources)
      loadRemoteApps(true)
    } catch (error) {
      console.error('Failed to add source:', error)
      alert(error instanceof Error ? error.message : '添加失败')
    }
  }

  // 获取所有分�?
  const categoryCounts = new Map<TappCategory, number>()

  // 统计所有应用的分类
  for (const app of allApps) {
    categoryCounts.set(
      app.category,
      (categoryCounts.get(app.category) ?? 0) + 1,
    )
  }

  const categories = TAPP_CATEGORIES.flatMap((id) => {
    const count = categoryCounts.get(id)
    return count
      ? [{ id, name: t.tapp[TAPP_CATEGORY_I18N_KEYS[id]], count }]
      : []
  })

  // 详情视图的安装状态派生
  const detailTappInfo = detailApp
    ? installedTapps.get(detailApp.id)
    : undefined
  const detailCanUninstall = detailTappInfo
    ? detailTappInfo.userRole === 'admin' ||
      (detailTappInfo.userRole === 'user' &&
        detailTappInfo.isTemporary === true)
    : false

  // 列表 ↔ 详情切换：记忆列表滚动位置，返回时恢复
  const listViewRef = useRef<HTMLDivElement | null>(null)
  const listScrollPosRef = useRef(0)
  const attachListView = useCallback((el: HTMLDivElement | null) => {
    listViewRef.current = el
    if (el) el.scrollTop = listScrollPosRef.current
  }, [])
  const openDetail = useCallback((app: UnifiedAppItem) => {
    listScrollPosRef.current = listViewRef.current?.scrollTop ?? 0
    setDetailApp(app)
  }, [])

  // 列表 ↔ 详情切换时头部与内容区的高度过渡
  const modalRef = useRef<HTMLDivElement | null>(null)
  const headerHeight = useHeightTransition({ animConfig })
  const contentHeight = useHeightTransition({ animConfig, modalRef })

  // 切换动效：进入详情向左滑（详情从右侧进入），返回反向
  const viewMotionProps = useCallback(
    (dir: 1 | -1) =>
      animConfig.level === 'none'
        ? {}
        : {
            initial: { opacity: 0, x: 24 * dir },
            animate: { opacity: 1, x: 0 },
            exit: { opacity: 0, x: 24 * dir },
            transition: {
              duration: 0.18 * animConfig.durationScale,
              ease: 'easeOut' as const,
            },
          },
    [animConfig],
  )

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
      className="surface-dialog-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm p-4"
      onClick={onClose}
      data-no-ripple
    >
      <motion.div
        initial={modalAnimProps.content.initial}
        animate={modalAnimProps.content.animate}
        exit={modalAnimProps.content.exit}
        transition={
          animConfig.level !== 'none'
            ? {
                duration: 0.2 * animConfig.durationScale,
                type: animConfig.spring ? 'spring' : 'tween',
                ...(animConfig.spring ? { stiffness: 300, damping: 25 } : {}),
              }
            : undefined
        }
        ref={modalRef}
        className="surface-dialog glass-surface rounded-2xl shadow-xl max-w-6xl w-full max-h-[90vh] overflow-hidden flex flex-col border border-gray-200/50 dark:border-neutral-700/50"
        onClick={(e: React.MouseEvent) => e.stopPropagation()}
      >
        {/* 头部：列表态为 标题/搜索/操作 + 分类行；详情态为 返回/应用名/关闭 */}
        {/* box-content 让测量到的内容高度直接作为 height，内边距不参与过渡 */}
        <div
          ref={headerHeight.attachWrapper}
          className="box-content overflow-hidden px-4 sm:px-6 pt-4 pb-3 border-b border-gray-200/50 dark:border-neutral-700/50"
        >
          <AnimatePresence mode="wait" initial={false}>
            {detailApp ? (
              <motion.div
                key="detail-header"
                ref={headerHeight.attachContent}
                {...viewMotionProps(1)}
                className="flex items-center justify-between gap-3"
              >
                <button
                  onClick={() => setDetailApp(null)}
                  className="flex h-8 shrink-0 items-center gap-1.5 rounded-full bg-black/5 px-3 text-sm font-medium text-gray-600 transition-colors hover:bg-black/10 dark:bg-white/10 dark:text-gray-300 dark:hover:bg-white/15"
                >
                  <FaArrowLeft className="w-3.5 h-3.5" />
                  {t.tapp.back}
                </button>
                <button
                  onClick={onClose}
                  title={t.tapp.storeClose}
                  aria-label={t.tapp.storeClose}
                  className="shrink-0 p-1.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
                >
                  <FaTimes className="w-4 h-4" />
                </button>
              </motion.div>
            ) : (
              <motion.div
                key="list-header"
                ref={headerHeight.attachContent}
                {...viewMotionProps(-1)}
              >
                <div className="flex flex-wrap items-center gap-3">
                  <h2 className="flex shrink-0 items-center gap-2 text-lg font-semibold text-gray-800 dark:text-gray-100">
                    <TappIcon
                      icon={TAPP_ICON_TOKENS.store}
                      name={t.tapp.storeTitle}
                      sizeClass="w-7 h-7"
                    />
                    {t.tapp.storeTitle}
                  </h2>

                  <div className="relative order-last w-full min-w-0 sm:order-none sm:ml-auto sm:w-auto sm:max-w-md sm:flex-1">
                    <FaSearch className="pointer-events-none absolute left-3 top-1/2 w-3.5 h-3.5 -translate-y-1/2 text-gray-400" />
                    <input
                      type="text"
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      placeholder={t.tapp.searchApps}
                      className="h-9 w-full rounded-full border border-gray-200/50 bg-white/50 pl-9 pr-8 text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 dark:border-neutral-700/50 dark:bg-neutral-900/50"
                    />
                    {searchQuery && (
                      <button
                        onClick={() => setSearchQuery('')}
                        title={t.tapp.clearSearch}
                        aria-label={t.tapp.clearSearch}
                        className="absolute right-2.5 top-1/2 -translate-y-1/2 p-0.5 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
                      >
                        <FaTimesCircle className="w-3.5 h-3.5" />
                      </button>
                    )}
                  </div>

                  <div className="ml-auto flex shrink-0 items-center gap-1 sm:ml-0">
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
                          {loading ? (
                            <Spinner size="sm" color="current" />
                          ) : (
                            <FaSync className="w-4 h-4" />
                          )}
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

                {/* Full category sets stay visible on desktop instead of
                    disappearing behind a horizontal scroller. */}
                <div className="mt-3 hidden items-start gap-3 sm:flex">
                  <div
                    role="group"
                    aria-label={t.tapp.categoryFilter}
                    className="flex min-w-0 flex-1 flex-wrap gap-1.5"
                  >
                    <CategoryPill
                      active={selectedCategory === null}
                      label={t.tapp.allApps}
                      count={allApps.length}
                      onClick={() => setSelectedCategory(null)}
                    />
                    <CategoryPill
                      active={selectedCategory === '__installed__'}
                      icon={<FaCheckCircle className="w-3.5 h-3.5" />}
                      label={t.tapp.installed}
                      count={installedIds.size}
                      onClick={() => setSelectedCategory('__installed__')}
                    />
                    {categories.map((cat) => (
                      <CategoryPill
                        key={cat.id}
                        active={selectedCategory === cat.id}
                        icon={CATEGORY_ICONS[cat.id]}
                        label={cat.name}
                        count={cat.count}
                        onClick={() => setSelectedCategory(cat.id)}
                      />
                    ))}
                  </div>
                  <span className="shrink-0 whitespace-nowrap pt-2 text-xs text-gray-400 dark:text-gray-500">
                    {format(t.tapp.totalApps, { total: allApps.length })} ·{' '}
                    {format(t.tapp.installedCount, {
                      count: installedIds.size,
                    })}
                  </span>
                </div>

                {/* On narrow screens a native select scales to every category
                    without consuming multiple rows of the modal header. */}
                <div className="mt-3 flex items-center gap-2 sm:hidden">
                  <div className="relative min-w-0 flex-1">
                    <FaFilter className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-gray-400" />
                    <select
                      aria-label={t.tapp.categoryFilter}
                      value={selectedCategory ?? '__all__'}
                      onChange={(event) => {
                        const value = event.target.value
                        setSelectedCategory(
                          value === '__all__'
                            ? null
                            : (value as TappCategory | '__installed__'),
                        )
                      }}
                      className="h-9 w-full appearance-none rounded-full border border-gray-200/50 bg-white/60 pl-9 pr-8 text-sm text-gray-700 focus:outline-none focus:ring-2 focus:ring-indigo-500 dark:border-neutral-700/50 dark:bg-neutral-900/60 dark:text-gray-200"
                    >
                      <option value="__all__">
                        {t.tapp.allApps} ({allApps.length})
                      </option>
                      <option value="__installed__">
                        {t.tapp.installed} ({installedIds.size})
                      </option>
                      {categories.map((cat) => (
                        <option key={cat.id} value={cat.id}>
                          {cat.name} ({cat.count})
                        </option>
                      ))}
                    </select>
                    <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-[10px] text-gray-400">
                      ▾
                    </span>
                  </div>
                  <span className="shrink-0 text-xs tabular-nums text-gray-400 dark:text-gray-500">
                    {filteredApps.length}/{allApps.length}
                  </span>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {/* 内容区域：列表与详情各自持有滚动容器，切换时方向性滑动，
            外层容器高度跟随当前视图内容平滑过渡 */}
        <div
          ref={contentHeight.attachWrapper}
          className="min-h-0 overflow-hidden"
        >
          <AnimatePresence mode="wait" initial={false}>
            {detailApp ? (
              <motion.div
                key={`detail-${detailApp.id}`}
                {...viewMotionProps(1)}
                className="h-full overflow-y-auto"
              >
                <div ref={contentHeight.attachContent} className="p-4 sm:p-6">
                  <AppDetailView
                    app={detailApp}
                    isInstalled={installedIds.has(detailApp.id)}
                    installedVersion={detailTappInfo?.version}
                    canUninstall={detailCanUninstall}
                    installing={installing === detailApp.id}
                    updating={updating === detailApp.id}
                    onInstall={() => handleInstall(detailApp)}
                    onUpdate={() => handleUpdate(detailApp)}
                    onUninstall={() => handleUninstall(detailApp.id)}
                  />
                </div>
              </motion.div>
            ) : (
              <motion.div
                key="list"
                ref={attachListView}
                {...viewMotionProps(-1)}
                className="h-full overflow-y-auto"
              >
                <div ref={contentHeight.attachContent} className="p-4 sm:p-6">
                  {loading && remoteApps.length === 0 ? (
                    <div className="text-center py-12">
                      <Spinner
                        size="xl"
                        color="primary"
                        center
                        className="mb-4"
                      />
                      <p className="text-gray-500 dark:text-gray-400">
                        {t.tapp.loadingRemoteApps}
                      </p>
                    </div>
                  ) : error && remoteApps.length === 0 ? (
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
                  ) : filteredApps.length === 0 ? (
                    <div className="text-center py-12">
                      <FaFilter className="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-4" />
                      <p className="text-gray-500 dark:text-gray-400">
                        {t.tapp.noMatchingApps}
                      </p>
                    </div>
                  ) : (
                    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                      <AnimatePresence mode="popLayout">
                        {filteredApps.map((app, index) => {
                          const tappInfo = installedTapps.get(app.id)
                          const canUninstall = tappInfo
                            ? tappInfo.userRole === 'admin' ||
                              (tappInfo.userRole === 'user' &&
                                tappInfo.isTemporary === true)
                            : false
                          const canUpdate =
                            !!tappInfo &&
                            ((app.source === 'remote' && !!app.remoteApp) ||
                              (app.source === 'local' && !!app.localTapp))
                          return (
                            <UnifiedAppCard
                              key={app.id}
                              app={app}
                              isInstalled={installedIds.has(app.id)}
                              installedVersion={tappInfo?.version}
                              canUninstall={canUninstall}
                              onInstall={() => handleInstall(app)}
                              onUpdate={
                                canUpdate ? () => handleUpdate(app) : undefined
                              }
                              onUninstall={
                                canUninstall
                                  ? () => handleUninstall(app.id)
                                  : undefined
                              }
                              onOpen={() => openDetail(app)}
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
              </motion.div>
            )}
          </AnimatePresence>
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
