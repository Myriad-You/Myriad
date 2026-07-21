/**
 * Tapp 鍟嗗簵/鍒楄〃椤甸潰
 * 鏄剧ず宸插畨瑁呯殑 Tapp 鍜屽彲鐢ㄧ殑 Tapp
 */

import type { ToastType } from '../../components/Toast'

import type { TappInstance, TappPermission } from '../types'
import type { IconStyle } from '../utils/tappColors'
import {
  FaCog,
  FaFileAlt,
  FaFolder,
  FaLock,
  FaPause,
  FaPlay,
  FaPlus,
  FaTh,
  FaTimes,
  FaTrash,
  FaUpload,
  MyriadStoreIcon,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {
  forwardRef,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react'

import { useNavigate } from 'react-router-dom'
import AnimatedView from '../../components/AnimatedView'
import { Spinner } from '../../components/Spinner'
import Toast from '../../components/Toast'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { useTappScheduler, useTappStagger } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { usePerformanceProfile } from '../../hooks/usePerformanceProfile'
import { useBreakpoints } from '../../hooks/useSharedEventListener'
import { useResolvedTitleColor, useTitleFont } from '../../hooks/useTitleFont'
import { hasSessionHint } from '../../utils/sessionDetection'
import { TappPlaygroundIcon } from '../components/PlaygroundIcons'
import { TappIcon } from '../components/TappIcon'
import { UninstallConfirmDialog } from '../components/UninstallConfirmDialog'
import { TAPP_ICON_TOKENS } from '../constants/icons'
import { getTappRuntime } from '../runtime'
import { PERMISSION_LEVELS } from '../runtime/permissionConfig'
import { isWebKit } from '../runtime/TappPageSandbox'
import * as TappApiService from '../services/TappApiService'
import { resolveManifestText } from '../utils/manifestLocale'
import {
  resolveTappCategory,
  TAPP_CATEGORY_I18N_KEYS,
} from '../utils/tappCategories'
import { getTappIconStyle as getTappIconStyleFromManifest } from '../utils/tappColors'

// 使用 TappIcon 组件统一处理图标渲染

/** 包装函数，接受 TappInstance 并返回图标背景样式 */
function getTappIconStyle(tapp: TappInstance): IconStyle {
  return getTappIconStyleFromManifest(tapp.manifest)
}

const TappStore = lazy(() => import('../components/TappStore'))

/** 按后端一致的权限等级统计 Manifest 权限。 */
function getPermissionCounts(permissions: TappPermission[]): {
  basic: number
  elevated: number
  admin: number
} {
  let basic = 0
  let elevated = 0
  let admin = 0

  for (const p of permissions) {
    const level = PERMISSION_LEVELS[p]
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

interface TappCardProps {
  tapp: TappInstance
  isRunning: boolean
  onStart: () => void
  onStop: () => void
  onUninstall: () => void
  onConfigure: () => void
  onOpen: () => void
  /** 鍗＄墖绱㈠紩锛堢敤浜庝氦閿欏姩鐢伙級 */
  index: number
}

const TappCard = forwardRef<HTMLDivElement, TappCardProps>(
  (
    {
      tapp,
      isRunning,
      onStart,
      onStop,
      onUninstall,
      onConfigure,
      onOpen,
      index,
    },
    ref,
  ) => {
    const { manifest } = tapp
    const { t, locale } = useI18n()
    const { name: displayName, description: displayDescription } =
      resolveManifestText(manifest, locale)
    const perf = usePerformanceProfile()
    const animConfig = useAnimationLevel()
    const [isHovered, setIsHovered] = useState(false)

    // 交错延迟完全由协调器管理；none 模式直接显示。
    const animationsEnabled = animConfig.level !== 'none'
    const { canAnimate, onComplete } = useTappStagger(index, {
      enabled: animationsEnabled,
    })

    // 鑾峰彇鏉冮檺缁熻鍜屽簲鐢ㄧ被鍒?
    const permissionCounts = getPermissionCounts(tapp.grantedPermissions)
    const categoryId = resolveTappCategory(manifest)
    // 权限检查：判断当前用户是否可以执行操作
    // - admin: 可以操作所有 Tapp
    // - user: 只能操作自己临时安装的 Tapp（isTemporary=true），不能操作管理员的 Tapp
    // - guest: 只能查看，不能操作
    const canStartStop = tapp.userRole !== 'guest'
    const canUninstall =
      tapp.userRole === 'admin' ||
      (tapp.userRole === 'user' && tapp.isTemporary === true)
    const canConfigure = tapp.userRole !== 'guest'
    const category = t.tapp[TAPP_CATEGORY_I18N_KEYS[categoryId]]
    const totalPermissions =
      permissionCounts.basic +
      permissionCounts.elevated +
      permissionCounts.admin

    // 是否有页面模块可打开
    const hasPage = manifest.hasPage === true

    // Page Tapp 始终可以打开。运行页会按需启动当前查看者的会话；
    // 游客和公共安装的普通用户不会因此改写安装记录的持久运行状态。
    const handleCardClick = () => {
      if (hasPage) {
        onOpen()
      }
    }

    // 切换运行状态
    const handleToggleRun = (e: React.MouseEvent) => {
      e.stopPropagation()
      if (!canStartStop) return
      if (isRunning) {
        onStop()
      } else {
        onStart()
      }
    }

    // 获取图标样式用于装饰
    const iconStyle = getTappIconStyle(tapp)

    return (
      <motion.div
        ref={ref}
        layout={animConfig.level !== 'none'}
        initial={animationsEnabled ? { opacity: 0, y: 20 } : false}
        animate={
          !animationsEnabled || canAnimate
            ? { opacity: 1, y: 0 }
            : { opacity: 0, y: 20 }
        }
        exit={animationsEnabled ? { opacity: 0, scale: 0.95 } : undefined}
        onAnimationComplete={onComplete}
        transition={
          !animationsEnabled
            ? { duration: 0 }
            : !animConfig.spring ||
                perf.lowEndDevice ||
                animConfig.level !== 'standard'
              ? { type: 'tween', duration: 0.25 }
              : {
                  type: 'spring',
                  stiffness: 400,
                  damping: 28,
                }
        }
        whileHover={
          animConfig.level === 'standard' && !perf.lowEndDevice ? { y: -4 } : {}
        }
        whileTap={animationsEnabled ? { scale: 0.98 } : {}}
        onClick={handleCardClick}
        onMouseEnter={() => setIsHovered(true)}
        onMouseLeave={() => setIsHovered(false)}
        className={`group relative aspect-2/1 rounded-2xl overflow-hidden glass-surface glass-70 ${
          hasPage ? 'cursor-pointer' : ''
        }`}
      >
        {/* 动态渐变背景 - 基于图标颜色 */}
        <div
          className={`absolute inset-0 opacity-[0.08] transition-opacity duration-500 ${isHovered ? 'opacity-[0.15]' : ''}`}
          style={{
            background: `linear-gradient(135deg, ${iconStyle.style?.background ? iconStyle.style.background : 'var(--color-primary)'}, transparent 60%)`,
          }}
        />

        {/* 瑁呴グ鍏夋晥 - 鍙充笂 */}
        <div
          className={`absolute -right-8 -top-8 w-24 h-24 rounded-full blur-2xl transition-all duration-500 ${
            isRunning ? 'opacity-40' : 'opacity-20'
          } ${isHovered ? 'scale-150 opacity-50' : ''}`}
          style={{
            background:
              'linear-gradient(180deg, var(--color-primary), transparent)',
          }}
        />

        {/* 瑁呴グ鍏夋晥 - 宸︿笅 */}
        <div
          className={`absolute -left-6 -bottom-6 w-16 h-16 rounded-full blur-xl opacity-10 transition-all duration-500 ${isHovered ? 'scale-125 opacity-20' : ''}`}
          style={{ background: 'var(--color-primary)' }}
        />

        {/* ===== 涓诲唴瀹瑰尯鍩?===== */}
        <div className="relative z-10 h-full flex flex-col p-3">
          {/* 椤堕儴鍖哄煙锛氬浘鏍?+ 鍚嶇О + 鐘舵€? */}
          <div className="flex items-start gap-2.5 mb-auto">
            {/* 应用图标 */}
            <div
              className={`w-14 h-14 rounded-xl ${iconStyle.className} flex items-center justify-center text-white shadow-lg relative overflow-hidden shrink-0`}
              style={iconStyle.style}
            >
              <div className="absolute inset-0 bg-linear-to-br from-white/25 to-transparent" />
              <TappIcon
                icon={manifest.icon}
                iconSvg={manifest.iconSvg}
                name={displayName}
                sizeClass="w-8 h-8"
                textSizeClass="text-2xl"
                className="relative z-10"
              />
              {/* 杩愯涓殑鑴夊啿鏁堟灉 */}
              {isRunning && (
                <div className="absolute inset-0 bg-white/20 animate-pulse" />
              )}
            </div>

            {/* 鍚嶇О + 鍏冧俊鎭? */}
            <div className="flex-1 min-w-0 pt-1">
              <div className="flex items-center gap-2">
                <h3 className="font-bold text-gray-800 dark:text-gray-100 truncate text-base leading-tight">
                  {displayName}
                </h3>
                {isRunning && (
                  <span className="w-2 h-2 rounded-full bg-green-500 animate-pulse shrink-0" />
                )}
              </div>
              <div className="flex items-center gap-2 mt-1">
                <span className="text-xs px-2 py-0.5 rounded-md bg-black/5 dark:bg-white/10 text-gray-600 dark:text-gray-400 font-medium">
                  {category}
                </span>
                <span className="text-xs text-gray-400 dark:text-gray-500">
                  v{manifest.version}
                </span>
              </div>
            </div>

            {/* 鍙充笂瑙掓搷浣滃尯 - 鎮诞鏄剧ず瀹屾暣锛岄粯璁ゅ彧鏄剧ず涓绘寜閽? */}
            <div className="flex items-center gap-0.5 shrink-0">
              {/* 涓绘搷浣滄寜閽細鍚姩/鍋滄 - 仅有权限时显示 */}
              {canStartStop && (
                <motion.button
                  onClick={handleToggleRun}
                  className={`p-2 rounded-xl transition-all shadow-sm ${
                    isRunning
                      ? 'bg-amber-500/15 text-amber-600 dark:text-amber-400 hover:bg-amber-500/25'
                      : 'bg-green-500/15 text-green-600 dark:text-green-400 hover:bg-green-500/25'
                  }`}
                  whileHover={{ scale: 1.1 }}
                  whileTap={{ scale: 0.95 }}
                  title={isRunning ? t.tapp.stop : t.tapp.start}
                >
                  {isRunning ? (
                    <FaPause className="w-3 h-3" />
                  ) : (
                    <FaPlay className="w-3 h-3" />
                  )}
                </motion.button>
              )}

              {/* 次要操作：悬浮时展开 - 仅有权限时显示 */}
              {(canConfigure || canUninstall) && (
                <motion.div
                  className="flex items-center gap-0.5 overflow-hidden"
                  initial={{ width: 0, opacity: 0 }}
                  animate={{
                    width: isHovered ? 'auto' : 0,
                    opacity: isHovered ? 1 : 0,
                  }}
                  transition={{ duration: 0.2 }}
                >
                  {canConfigure && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation()
                        onConfigure()
                      }}
                      className="p-2 rounded-xl transition-all text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-500/10"
                      title={t.tapp.settings}
                    >
                      <FaCog className="w-3 h-3" />
                    </button>
                  )}
                  {canUninstall && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation()
                        onUninstall()
                      }}
                      className="p-2 rounded-xl transition-all text-gray-400 hover:text-red-500 hover:bg-red-500/10"
                      title={t.tapp.uninstall}
                    >
                      <FaTrash className="w-3 h-3" />
                    </button>
                  )}
                </motion.div>
              )}
            </div>
          </div>

          {/* 底部区域：描述 + 权限信息 */}
          <div className="mt-auto">
            {/* 搴旂敤鎻忚堪 - 鏈€澶?琛屽彲婊氬姩 */}
            {displayDescription && (
              <div className="max-h-10 overflow-y-auto mb-2 scrollbar-thin scrollbar-thumb-gray-300 dark:scrollbar-thumb-gray-600 scrollbar-track-transparent">
                <p className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed pr-1">
                  {displayDescription}
                </p>
              </div>
            )}

            {/* 搴曢儴淇℃伅鏉★細鏉冮檺鎽樿 + 鍙墦寮€鎻愮ず */}
            <div className="flex items-center justify-between">
              {/* 鏉冮檺鎽樿 */}
              <div className="flex items-center gap-1">
                {totalPermissions > 0 ? (
                  <>
                    <FaLock className="w-2.5 h-2.5 text-gray-400" />
                    <div className="flex items-center gap-0.5 text-[9px]">
                      {permissionCounts.admin > 0 && (
                        <span className="px-1 py-0.5 rounded bg-red-500/10 text-red-500 dark:text-red-400 font-medium">
                          {permissionCounts.admin}
                        </span>
                      )}
                      {permissionCounts.elevated > 0 && (
                        <span className="px-1 py-0.5 rounded bg-amber-500/10 text-amber-600 dark:text-amber-400 font-medium">
                          {permissionCounts.elevated}
                        </span>
                      )}
                      {permissionCounts.basic > 0 && (
                        <span className="px-1 py-0.5 rounded bg-green-500/10 text-green-600 dark:text-green-400 font-medium">
                          {permissionCounts.basic}
                        </span>
                      )}
                      <span className="text-gray-400 dark:text-gray-500 ml-0.5">
                        {t.tapp.permissions}
                      </span>
                    </div>
                  </>
                ) : (
                  <span className="text-[9px] text-gray-400 dark:text-gray-500 flex items-center gap-1">
                    <FaLock className="w-2.5 h-2.5" />
                    {t.tapp.noPermissions}
                  </span>
                )}
              </div>

              {/* 可点击提示 */}
              {hasPage && (
                <motion.span
                  className="text-[9px] text-gray-400 dark:text-gray-500 flex items-center gap-1"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: isHovered ? 1 : 0.5 }}
                >
                  <span className="w-1 h-1 rounded-full bg-current" />
                  {t.tapp.clickToOpen}
                </motion.span>
              )}
            </div>
          </div>
        </div>

        {/* 杈规鏁堟灉 */}
        <div className="absolute inset-0 rounded-2xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />

        {/* 鎮诞鏃剁殑楂樺厜杈规 */}
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

TappCard.displayName = 'TappCard'

/**
 * 瀹夎 Tapp 妯℃€佹
 */
function InstallTappModal({
  onClose,
  onInstall,
  onSuccess,
}: {
  onClose: () => void
  onInstall: () => void
  onSuccess?: (name: string) => void
}) {
  const { t } = useI18n()
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [dragOver, setDragOver] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // 处理文件上传
  const handleFileUpload = async (file: File) => {
    if (!file.name.endsWith('.tapp')) {
      setError(t.tapp.selectTappFile)
      return
    }

    setError('')
    setLoading(true)

    try {
      const result = await TappApiService.installTappFile(file)
      onInstall()
      onSuccess?.(result.name || file.name.replace('.tapp', ''))
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : t.tapp.installFailed)
    } finally {
      setLoading(false)
    }
  }

  // 澶勭悊鎷栨斁
  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(false)
    const file = e.dataTransfer.files[0]
    if (file) handleFileUpload(file)
  }

  // 澶勭悊鏂囦欢閫夋嫨
  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (file) handleFileUpload(file)
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="surface-dialog-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm p-4"
      onClick={onClose}
    >
      <motion.div
        initial={{ scale: 0.95, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        exit={{ scale: 0.95, opacity: 0 }}
        className="surface-dialog glass rounded-2xl shadow-xl max-w-md w-full overflow-hidden"
        onClick={(e: React.MouseEvent) => e.stopPropagation()}
      >
        {/* 澶撮儴 */}
        <div className="px-6 py-4 border-b border-gray-200/50 dark:border-neutral-700/50 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-gray-800 dark:text-gray-100 flex items-center gap-2">
            <FaUpload className="text-indigo-500" />
            {t.tapp.installTappTitle}
          </h2>
          <button
            onClick={onClose}
            className="p-1.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
            title={t.tapp.storeClose}
            aria-label={t.tapp.storeClose}
          >
            <FaTimes className="w-4 h-4" />
          </button>
        </div>

        {/* 鍐呭 */}
        <div className="p-6">
          {/* 閿欒鎻愮ず */}
          {error && (
            <div className="mb-4 p-3 bg-red-50/80 dark:bg-red-900/20 border border-red-200/50 dark:border-red-800/50 rounded-lg text-red-600 dark:text-red-400 text-sm">
              {error}
            </div>
          )}

          {/* 鏂囦欢涓婁紶鍖哄煙 */}
          <div
            onDragOver={(e) => {
              e.preventDefault()
              setDragOver(true)
            }}
            onDragLeave={() => setDragOver(false)}
            onDrop={handleDrop}
            onClick={() => fileInputRef.current?.click()}
            className={`border-2 border-dashed rounded-xl p-12 text-center cursor-pointer transition-colors ${
              dragOver
                ? 'border-indigo-500 bg-indigo-50/50 dark:bg-indigo-900/20'
                : 'border-gray-300 dark:border-neutral-600 hover:border-indigo-400 hover:bg-gray-50/50 dark:hover:bg-neutral-800/50'
            } ${loading ? 'opacity-50 cursor-wait' : ''}`}
          >
            <input
              ref={fileInputRef}
              type="file"
              accept=".tapp"
              onChange={handleFileChange}
              className="hidden"
              disabled={loading}
              aria-label={t.tapp.selectTappFile}
            />
            {loading ? (
              <>
                <Spinner size="xl" color="primary" center className="mb-4" />
                <p className="text-gray-600 dark:text-gray-300 font-medium">
                  {t.tapp.installing}
                </p>
              </>
            ) : (
              <>
                <FaFileAlt className="w-12 h-12 mx-auto mb-4 text-gray-400 dark:text-gray-500" />
                <p className="text-gray-600 dark:text-gray-300 font-medium mb-2">
                  {t.tapp.dropTappFile}
                </p>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  {t.tapp.orClickToSelect}
                </p>
              </>
            )}
          </div>
        </div>

        {/* 搴曢儴 */}
        <div className="px-6 py-4 border-t border-gray-200/50 dark:border-neutral-700/50 flex justify-end">
          <button
            onClick={onClose}
            className="px-4 py-2 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
          >
            {t.tapp.cancel}
          </button>
        </div>
      </motion.div>
    </motion.div>
  )
}

/**
 * Tapp 鍒楄〃椤甸潰缁勪欢
 */
export function TappListPage() {
  const navigate = useNavigate()
  const { t, locale } = useI18n()
  const { isMobile } = useBreakpoints()
  const { isAdmin, hasChecked, checkAuth } = useAuth()
  // 🆕 标题字体 Hook
  const { currentFont, titleFontSize } = useTitleFont()
  // 自适应色对齐 Tapp 音乐播放器歌词：对比度推导
  const titleColorCss = useResolvedTitleColor()

  const [tapps, setTapps] = useState<TappInstance[]>([])
  const [runningTapps, setRunningTapps] = useState<Set<string>>(new Set())
  const [loading, setLoading] = useState(true)
  const [showEmpty, setShowEmpty] = useState(false) // 延迟显示空状态
  const [showInstallModal, setShowInstallModal] = useState(false)
  const [showStore, setShowStore] = useState(false)
  const [toastMessage, setToastMessage] = useState('')
  const [toastType, setToastType] = useState<ToastType>('info')
  // 卸载确认对话框状态
  const [showUninstallDialog, setShowUninstallDialog] = useState(false)
  const [uninstallTargetId, setUninstallTargetId] = useState<string | null>(
    null,
  )
  const [uninstallTargetName, setUninstallTargetName] = useState('')
  const runtime = getTappRuntime()

  const showToastMessage = useCallback(
    (message: string, type: ToastType = 'info') => {
      setToastType(type)
      setToastMessage(message)
    },
    [],
  )

  // 馃幀 鍒濆鍖?Tapp 椤甸潰璋冨害鍣紙缁熶竴鍔ㄧ敾鍗忚皟锛?
  useTappScheduler()

  // 鍔犺浇 Tapp 鍒楄〃
  const loadTapps = useCallback(
    async (forceSync: boolean = false) => {
      // 如果需要强制同步（如安装后），先从后端刷新
      if (forceSync) {
        await runtime.syncFromBackend(true)
      }

      const allTapps = runtime.getAllTapps()
      setTapps(allTapps)
      setLoading(false)

      const running = new Set<string>()
      allTapps.forEach((tapp) => {
        if (runtime.isRunning(tapp.id)) {
          running.add(tapp.id)
        }
      })
      setRunningTapps(running)
    },
    [runtime],
  )

  // 延迟显示空状态 - 避免加载快时闪烁
  useEffect(() => {
    if (!loading && tapps.length === 0) {
      const timer = setTimeout(() => {
        setShowEmpty(true)
      }, 150) // 150ms 延迟，确认确实没有应用
      return () => clearTimeout(timer)
    } else {
      setShowEmpty(false)
    }
  }, [loading, tapps.length])

  useEffect(() => {
    let mounted = true

    // 首次访问时检查认证状态
    if (!hasChecked && hasSessionHint()) {
      checkAuth()
    }

    // 初始加载：等待同步完成后再获取 Tapp 列表
    const initLoad = async () => {
      await runtime.waitForSync()
      if (mounted) {
        loadTapps()
      }
    }

    initLoad()

    // 鐩戝惉浜嬩欢
    const handleTappChange = () => loadTapps()
    const unsubInstalled = runtime.on('tapp:installed', handleTappChange)
    const unsubUninstalled = runtime.on('tapp:uninstalled', handleTappChange)
    const unsubStarted = runtime.on('tapp:started', handleTappChange)
    const unsubStopped = runtime.on('tapp:stopped', handleTappChange)
    const unsubSyncComplete = runtime.on('sync:complete', handleTappChange)

    return () => {
      mounted = false
      unsubInstalled()
      unsubUninstalled()
      unsubStarted()
      unsubStopped()
      unsubSyncComplete()
    }
  }, [loadTapps, runtime, hasChecked, checkAuth])

  const handleStart = async (tappId: string) => {
    try {
      await runtime.startTapp(tappId)
    } catch (error) {
      console.error('Failed to start Tapp:', error)
    }
  }

  const handleStop = async (tappId: string) => {
    try {
      await runtime.stopTapp(tappId)
    } catch (error) {
      console.error('Failed to stop Tapp:', error)
    }
  }

  const handleUninstall = async (tappId: string) => {
    // 找到对应的 Tapp 获取名称
    const tapp = tapps.find((t) => t.id === tappId)
    setUninstallTargetId(tappId)
    setUninstallTargetName(
      tapp ? resolveManifestText(tapp.manifest, locale).name : tappId,
    )
    setShowUninstallDialog(true)
  }

  // 确认卸载
  const handleConfirmUninstall = async (keepData: boolean) => {
    if (!uninstallTargetId) return
    try {
      await runtime.uninstallTapp(uninstallTargetId, { keepData })
      setShowUninstallDialog(false)
      setUninstallTargetId(null)
    } catch (error) {
      console.error('Failed to uninstall Tapp:', error)
      showToastMessage(t.tapp.uninstallFailed || 'Uninstall failed', 'error')
      throw error // 让组件处理 loading 状态
    }
  }

  // 取消卸载
  const cancelUninstall = () => {
    setShowUninstallDialog(false)
    setUninstallTargetId(null)
  }

  const handleOpen = (tappId: string) => {
    navigate(`/tapp/run/${tappId}`)
  }

  const handleConfigure = (tappId: string) => {
    navigate(`/tapp/detail/${tappId}`)
  }

  return (
    <AnimatedView className="min-h-screen">
      <div className="h-full flex flex-col pt-20 pb-6 px-3 xs:px-4 sm:px-6">
        <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col gap-3 p-2 relative min-h-0">
          {/* 涓婂崐閮ㄥ垎鐣欑櫧锛屼笌棣栭〉 Widget 鍖哄煙瀵归綈 */}
          <div className="hidden lg:block flex-1 min-h-[30vh]" />

          {/* 椤堕儴淇℃伅鏉?- 涓庨椤靛竷灞€涓€鑷? */}
          <div className="relative h-12 shrink-0 z-10">
            {/* 鑳屾櫙鏍囬 */}
            <div
              className="absolute left-0 whitespace-nowrap pointer-events-none z-0 hidden md:block"
              style={{
                top: `calc(24px - ${7.5 * titleFontSize}rem)`,
                fontSize: `${6 * titleFontSize}rem`,
                color: titleColorCss,
                WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorCss} 30%, transparent)`,
                fontFamily: currentFont.family,
                fontWeight: 700,
              }}
            >
              Tapp
            </div>

            <div className="h-full flex items-center justify-between">
              {/* 宸︿晶淇℃伅鍗＄墖 */}
              <div className="h-full glass rounded-xl px-4 py-1 flex items-center gap-3 shadow-sm relative z-10">
                <TappIcon
                  icon={TAPP_ICON_TOKENS.store}
                  name={t.tapp.storeTitle}
                  sizeClass="w-7 h-7"
                />
                <div className="flex flex-col justify-center">
                  <div className="text-sm font-bold text-gray-800 dark:text-gray-200 leading-tight">
                    {t.tapp.listTitle}
                  </div>
                  <div className="text-[10px] text-gray-500 dark:text-gray-400 max-w-50 truncate leading-tight">
                    {t.tapp.listSubtitle}
                  </div>
                </div>

                {/* 鍒嗛殧绾? */}
                <div className="hidden sm:block h-6 w-px bg-gray-200 dark:bg-white/10 mx-1" />

                {/* 鎿嶄綔鎸夐挳 */}
                <div className="hidden sm:flex items-center gap-2">
                  {isAdmin && (
                    <button
                      onClick={() => navigate('/tapp/playground')}
                      className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                      style={{ color: 'var(--color-primary)' }}
                      title={t.tapp.playgroundTitle}
                    >
                      <TappPlaygroundIcon className="w-3.5 h-3.5" />
                      {t.tapp.playground}
                    </button>
                  )}
                  <button
                    onClick={() => setShowStore(true)}
                    className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                    style={{ color: 'var(--color-primary)' }}
                  >
                    <MyriadStoreIcon className="w-4 h-4" />
                    {t.tapp.store}
                  </button>
                  {/* 多任务入口 - 仅平板和PC端显示，Safari 不支持 */}
                  {!isMobile && !isWebKit && (
                    <button
                      onClick={() => navigate('/tapp/run?multi=true')}
                      className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                      style={{ color: 'var(--color-primary)' }}
                      title={t.tapp.multiWindow}
                    >
                      <FaTh className="w-3 h-3" />
                      {t.tapp.multiWindow}
                    </button>
                  )}
                  {isAdmin && (
                    <button
                      onClick={() => setShowInstallModal(true)}
                      className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10 cursor-pointer"
                      style={{ color: 'var(--color-primary)' }}
                      title={t.tapp.install}
                    >
                      <FaPlus className="w-3 h-3" />
                      {t.tapp.install}
                    </button>
                  )}
                </div>
              </div>

              {/* 右侧移动端按钮 — Playground 仅桌面端入口 */}
              <div className="flex sm:hidden items-center gap-2">
                <button
                  onClick={() => setShowStore(true)}
                  className="p-2 rounded-lg glass shadow-sm"
                  title={t.tapp.storeTitle}
                >
                  <MyriadStoreIcon
                    className="w-5 h-5"
                    style={{ color: 'var(--color-primary)' }}
                  />
                </button>
                {isAdmin && (
                  <button
                    onClick={() => setShowInstallModal(true)}
                    className="p-2 rounded-lg glass shadow-sm"
                    title={t.tapp.manualInstall}
                  >
                    <FaPlus
                      className="w-5 h-5"
                      style={{ color: 'var(--color-primary)' }}
                    />
                  </button>
                )}
              </div>
            </div>
          </div>

          {/* 鍐呭鍖哄煙 */}
          {tapps.length === 0 && showEmpty ? (
            /* 绌虹姸鎬?- 涓庡崱鐗囪璁￠鏍间竴鑷? */
            <div className="relative rounded-2xl overflow-hidden">
              {/* 鑳屾櫙 */}
              <div className="absolute inset-0 glass-surface glass-70" />

              {/* 瑁呴グ鍏夋晥 */}
              <div
                className="absolute -right-20 -top-20 w-48 h-48 rounded-full blur-3xl opacity-20"
                style={{ background: 'var(--color-primary)' }}
              />
              <div
                className="absolute -left-16 -bottom-16 w-32 h-32 rounded-full blur-2xl opacity-15"
                style={{ background: 'var(--color-primary)' }}
              />

              {/* 鍐呭 */}
              <div className="relative z-10 p-8 md:p-12 text-center">
                <div className="w-20 h-20 mx-auto mb-5 rounded-2xl bg-linear-to-br from-gray-100 to-gray-200 dark:from-white/10 dark:to-white/5 flex items-center justify-center shadow-lg">
                  <FaFolder className="w-10 h-10 text-gray-400 dark:text-gray-500" />
                </div>
                <h3 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">
                  {t.tapp.noAppsInstalled}
                </h3>
                <p className="text-gray-500 dark:text-gray-400 mb-6 text-sm max-w-sm mx-auto">
                  {t.tapp.noAppsInstalledDesc}
                </p>
                <div className="flex items-center justify-center gap-3">
                  <motion.button
                    onClick={() => setShowStore(true)}
                    className="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl font-bold text-sm transition-all shadow-sm"
                    style={{
                      background:
                        'linear-gradient(135deg, var(--color-primary), color-mix(in srgb, var(--color-primary) 80%, black))',
                      color: 'white',
                    }}
                    whileHover={{ scale: 1.02, y: -1 }}
                    whileTap={{ scale: 0.98 }}
                  >
                    <MyriadStoreIcon className="w-4 h-4" />
                    {t.tapp.browseStore}
                  </motion.button>
                  {!isMobile && isAdmin && (
                    <motion.button
                      onClick={() => navigate('/tapp/playground')}
                      className="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl font-bold text-sm transition-all bg-black/5 dark:bg-white/10 hover:bg-black/10 dark:hover:bg-white/15"
                      style={{ color: 'var(--color-primary)' }}
                      whileHover={{ scale: 1.02, y: -1 }}
                      whileTap={{ scale: 0.98 }}
                    >
                      <TappPlaygroundIcon className="w-4 h-4" />
                      {t.tapp.playground}
                    </motion.button>
                  )}
                  {isAdmin && (
                    <motion.button
                      onClick={() => setShowInstallModal(true)}
                      className="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl font-bold text-sm transition-all bg-black/5 dark:bg-white/10 hover:bg-black/10 dark:hover:bg-white/15"
                      style={{ color: 'var(--color-primary)' }}
                      whileHover={{ scale: 1.02, y: -1 }}
                      whileTap={{ scale: 0.98 }}
                      title={t.tapp.manualInstall}
                    >
                      <FaPlus className="w-4 h-4" />
                      {t.tapp.manualInstall}
                    </motion.button>
                  )}
                </div>
              </div>

              {/* 杈规 */}
              <div className="absolute inset-0 rounded-2xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />
            </div>
          ) : (
            /* Tapp 鍒楄〃 - 2:1 瀹介珮姣斿崱鐗囷紝4鍒楃綉鏍? */
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
              <AnimatePresence mode="popLayout">
                {tapps.map((tapp, index) => (
                  <TappCard
                    key={tapp.id}
                    tapp={tapp}
                    isRunning={runningTapps.has(tapp.id)}
                    onStart={() => handleStart(tapp.id)}
                    onStop={() => handleStop(tapp.id)}
                    onUninstall={() => handleUninstall(tapp.id)}
                    onConfigure={() => handleConfigure(tapp.id)}
                    onOpen={() => handleOpen(tapp.id)}
                    index={index}
                  />
                ))}
              </AnimatePresence>
            </div>
          )}
        </div>
      </div>

      {/* 瀹夎妯℃€佹 */}
      <AnimatePresence>
        {showInstallModal && (
          <InstallTappModal
            onClose={() => setShowInstallModal(false)}
            onInstall={() => loadTapps(true)}
            onSuccess={(name) =>
              showToastMessage(
                t.tapp.installSuccess.replace('{name}', name),
                'success',
              )
            }
          />
        )}
      </AnimatePresence>

      {/* 搴旂敤鍟嗗簵 */}
      <Suspense fallback={null}>
        <AnimatePresence>
          {showStore && (
            <TappStore
              isOpen={showStore}
              onClose={() => setShowStore(false)}
              onInstalled={() => loadTapps(true)}
            />
          )}
        </AnimatePresence>
      </Suspense>

      {/* 卸载确认对话框 */}
      <UninstallConfirmDialog
        isOpen={showUninstallDialog}
        appName={uninstallTargetName}
        onCancel={cancelUninstall}
        onConfirm={handleConfirmUninstall}
      />

      {/* Toast 提示 */}
      {toastMessage && (
        <Toast
          message={toastMessage}
          type={toastType}
          onClose={() => setToastMessage('')}
        />
      )}
    </AnimatedView>
  )
}

export default TappListPage
