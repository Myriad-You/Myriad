/**
 * Tapp 运行页面
 * 在沙箱中运行 Tapp
 *
 * 布局：标题与内容同一 max-w-6xl 列；内容挂在 relative 槽内，
 * 普通模式 absolute inset-0、全屏 fixed inset-0，iframe 不卸载。
 * 支持多窗口模式（TappWindowManager）。
 */

import type { CSSProperties } from 'react'

import type { TappCodeStructure, TappInstance } from '../types'
import {
  FaCog,
  FaComments,
  FaCompress,
  FaExclamationTriangle,
  FaExpand,
  FaPause,
  FaRedo,
  FaTh,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Navigate, useNavigate, useParams } from 'react-router-dom'
import { Spinner } from '../../components/Spinner'
import { useI18n } from '../../contexts/I18nContext'
import { userFacingError } from '../../utils/userFacingError'
import { isExlight, useAnimationLevel } from '../../hooks/useAnimationLevel'
import { usePageSeo } from '../../hooks/usePageSeo'
import { useBreakpoints } from '../../hooks/useSharedEventListener'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../../utils/moduleVisibility'
import { TappAppShell } from '../components/TappAppShell'
import { TappIconBadge } from '../components/TappIconBadge'
import { TappWindowManager } from '../components/TappWindowManager'
import { HOST_PANEL_STORE_ID, isStoreHostPanel } from '../constants/hostPanels'
import { useTappFullscreenChrome } from '../hooks/useTappFullscreenChrome'
import { useTappMultiWindowSession } from '../hooks/useTappMultiWindowSession'
import { useTappShellClose } from '../hooks/useTappShellClose'
import { useTappShellPresence } from '../hooks/useTappShellPresence'
import { useWindowAgentHandler } from '../hooks/useWindowAgentHandler'
import { getTappRuntime } from '../runtime'
import { loadPageResources } from '../runtime/sandbox/resourceLoader'
import { isWebKit, TappPageSandbox } from '../runtime/TappPageSandbox'
import { resolveManifestText } from '../utils/manifestLocale'
import { getTappIconStyle } from '../utils/tappColors'
import { buildTappRunPageSeo } from '../utils/tappPageSeo'
import {
  TAPP_LIST_PATH,
  TAPP_STORE_PATH,
  tappDetailPath,
  tappRunPath,
} from '../utils/tappPaths'

/**
 * Tapp 运行页面入口（含 /tapp/run 与 /tapp/run/:id）
 * 支持单窗口 / 多窗口 / 商店宿主重定向
 */
export function TappRunPage() {
  const { id } = useParams<{ id: string }>()
  // react-router already decodes path params; avoid double-decode (throws on lone `%`)
  const tappId = id ?? ''
  const { isMobile } = useBreakpoints()
  const isMultiWindow = useTappMultiWindowSession()
  const navigate = useNavigate()

  // /tapp/run?multi=true — 无 seed id
  if (!tappId) {
    if (isMultiWindow) {
      return <TappWindowManager onBack={() => navigate(TAPP_LIST_PATH)} />
    }
    return (
      <div className="flex min-h-screen items-center justify-center bg-gray-50 dark:bg-neutral-900">
        <p className="text-gray-500 dark:text-gray-400">无效的 Tapp ID</p>
      </div>
    )
  }

  // 宿主商店：单窗口走正式商店页，多窗口进窗口管理器
  if (isStoreHostPanel(tappId)) {
    if (isMultiWindow) {
      return (
        <TappWindowManager
          initialTappId={HOST_PANEL_STORE_ID}
          onBack={() => navigate(TAPP_LIST_PATH)}
        />
      )
    }
    return <Navigate to={TAPP_STORE_PATH} replace />
  }

  if (isMultiWindow) {
    return (
      <TappWindowManager
        initialTappId={tappId}
        onBack={() => navigate(TAPP_LIST_PATH)}
      />
    )
  }

  return <TappRunPageStandard tappId={tappId} isMobile={isMobile} />
}

/**
 * 标准版单窗口运行页
 */
function TappRunPageStandard({
  tappId,
  isMobile,
}: {
  tappId: string
  isMobile: boolean
}) {
  const navigate = useNavigate()
  const { t, locale } = useI18n()
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.tapp,
    { isAuthenticated: false, isAdmin: false },
  )

  // Agent ui.open / open_window: multi-window registers via TappWindowManager;
  // single-window must still handle open_window (navigate to /tapp/run/:id).
  const windowsRef = useRef<Array<{ windowId: string; tappId: string }>>([])
  const activeWindowIdRef = useRef<string | null>(null)
  const openTappWindow = useCallback(
    async (id: string) => {
      navigate(tappRunPath(id))
    },
    [navigate],
  )
  const closeWindow = useCallback(() => {
    navigate(TAPP_LIST_PATH)
  }, [navigate])
  const focusWindow = useCallback((_windowId: string) => {
    /* single-window: already focused */
  }, [])
  useWindowAgentHandler({
    windowsRef,
    activeWindowIdRef,
    openTappWindow,
    closeWindow,
    focusWindow,
  })

  // 动画配置
  const animConfig = useAnimationLevel()
  const noAnimation = isExlight(animConfig)

  const [tapp, setTapp] = useState<TappInstance | null>(null)
  const [code, setCode] = useState<TappCodeStructure | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [retryGeneration, setRetryGeneration] = useState(0)
  const runtime = getTappRuntime()

  // 路由级 SEO：应用名 / 描述 / 可见性 noindex
  usePageSeo(
    useMemo(
      () =>
        buildTappRunPageSeo({
          tapp,
          tappId,
          locale,
          moduleOpenToAll,
        }),
      [tapp, tappId, locale, moduleOpenToAll],
    ),
  )

  useTappFullscreenChrome(isFullscreen, setIsFullscreen)

  // 加载 Tapp
  useEffect(() => {
    let cancelled = false
    const loadTapp = async () => {
      setLoading(true)
      setError(null)
      setTapp(null)
      setCode(null)
      try {
        // Force a fresh catalog sync so userRole matches the logged-in viewer
        // (stale guest role from a public list freezes soft-guest UX).
        await runtime.syncFromBackend(true)
        if (cancelled) return

        let instance = runtime.getTapp(tappId)
        if (!instance) {
          setError(t.tapp.appNotExist)
          setLoading(false)
          return
        }

        const resources = await loadPageResources(instance)
        if (cancelled) return

        const tappCode: TappCodeStructure = {
          modules: resources.modules,
          moduleResolutions: resources.moduleResolutions,
          coreEntry: resources.coreEntry,
          pageEntry: resources.pageEntry,
          pageHtml: resources.html,
          styles: resources.styles,
          pageCSS: resources.css,
          i18n: resources.i18n,
        }

        if (!runtime.isRunning(tappId)) {
          // 仅所有者可启动；访客打开已停的公开 Tapp 不得会话假启动。
          if (runtime.canControlLifecycle(instance)) {
            await runtime.startTapp(tappId)
          } else {
            setError(t.tapp.stopped || 'Tapp is not running')
            setLoading(false)
            return
          }
        }
        if (cancelled) return

        // Re-read after start/sync — sandbox must not keep a pre-start guest instance.
        instance = runtime.getTapp(tappId) || instance

        setTapp(instance)
        setCode(tappCode)
        setLoading(false)
      } catch (err) {
        if (cancelled) return
        setError(userFacingError(err, t.tapp.loadAppFailed))
        setLoading(false)
      }
    }

    void loadTapp()
    return () => {
      cancelled = true
    }
  }, [
    tappId,
    runtime,
    retryGeneration,
    t.tapp.appNotExist,
    t.tapp.loadAppFailed,
  ])

  // 安装更新完成后，资源代际已由 runtime 提升；重新走完整 Page 加载并重建 iframe。
  useEffect(() => {
    return runtime.on('tapp:updated', (data) => {
      if ((data as { id: string }).id === tappId) {
        setRetryGeneration((generation) => generation + 1)
      }
    })
  }, [runtime, tappId])

  // 重试加载
  const handleRetry = useCallback(() => {
    setRetryGeneration((generation) => generation + 1)
  }, [])

  // 壳层进退场：无回弹 + 可选淡入淡出（WebKit 默认不在列上 opacity，防 iframe）
  const {
    shellClassName,
    scrimClassName,
    shellStyle,
    onShellAnimationEnd,
    requestClose,
    isExiting,
  } = useTappShellPresence({ enabled: !isFullscreen })
  const presence = useMemo(
    () => ({
      shellClassName,
      scrimClassName,
      shellStyle,
      onShellAnimationEnd,
      isExiting,
    }),
    [
      shellClassName,
      scrimClassName,
      shellStyle,
      onShellAnimationEnd,
      isExiting,
    ],
  )

  const navigateHome = useCallback(() => {
    navigate(TAPP_LIST_PATH)
  }, [navigate])

  // 全屏时先退出全屏再壳退场，避免 presence 被关掉导致瞬间跳走
  const goBack = useTappShellClose({
    isFullscreen,
    setIsFullscreen,
    requestClose,
    onClosed: navigateHome,
  })

  // 停止应用
  const handleStop = useCallback(async () => {
    try {
      await runtime.stopTapp(tappId)
      goBack()
    } catch (err) {
      console.error('Failed to stop Tapp:', err)
    }
  }, [runtime, tappId, goBack])

  // 切换全屏
  const toggleFullscreen = useCallback(() => {
    setIsFullscreen((prev) => !prev)
  }, [])

  // 打开设置
  const openSettings = useCallback(() => {
    navigate(tappDetailPath(tappId))
  }, [navigate, tappId])

  // 稳定的 safeInsets 对象，避免每次渲染都创建新对象
  const safeInsets = useMemo(() => {
    return isFullscreen
      ? { top: 72, right: 16, left: 16, bottom: 0 }
      : undefined
  }, [isFullscreen])

  // 独立合成层：减轻 WebKit 在 overflow:hidden 祖先下的 iframe 绘制问题
  const contentLayerStyle = useMemo(
    (): CSSProperties => ({
      WebkitTransform: 'translateZ(0)',
      transform: 'translateZ(0)',
      isolation: 'isolate',
    }),
    [],
  )

  // 动画配置 - 基于性能级别
  const transitions = useMemo(() => {
    const scale = animConfig.durationScale
    return {
      // 元素进入
      elementEnter: animConfig.spring
        ? { type: 'spring' as const, stiffness: 320, damping: 28 }
        : {
            type: 'tween' as const,
            duration: 0.35 * scale,
            ease: [0.22, 1, 0.36, 1],
          },
      // 快速过渡（全屏切换）
      quick: {
        type: 'tween' as const,
        duration: 0.25 * scale,
        ease: [0.4, 0, 0.2, 1],
      },
      // 状态切换（头部内容变化）
      stateSwitch: {
        type: 'tween' as const,
        duration: 0.2 * scale,
        ease: [0.4, 0, 0.2, 1],
      },
    }
  }, [animConfig.spring, animConfig.durationScale])

  // 内容状态
  const isReady = !loading && !error && !!tapp && !!code
  const hasError = !loading && (error || !tapp || !code)

  // 与 Runtime 一致：访客/普通用户不能启停站主公开装
  const canStartStop = !!tapp && runtime.canControlLifecycle(tapp)
  const canConfigure = canStartStop
  const iconStyle = tapp ? getTappIconStyle(tapp.manifest) : null
  const displayName = tapp
    ? resolveManifestText(tapp.manifest, locale).name
    : ''

  // Host chrome z-ladder（勿把 TApp 抬过 GCP）:
  // shell 40 · NavigationIsland 50 · FS toolbar 900 · GCP 998/9999
  const fsToolbar = (
    <AnimatePresence>
      {isFullscreen && isReady && tapp && (
        <motion.div
          key="fullscreen-toolbar"
          initial={isMobile ? false : { opacity: 0, x: -16, scale: 0.92 }}
          animate={{ opacity: 1, x: 0, scale: 1 }}
          exit={{ opacity: 0, x: -16, scale: 0.92 }}
          transition={transitions.elementEnter}
          className={`fixed top-4 left-4 z-900 transition-opacity duration-300 ${
            isMobile
              ? 'opacity-100'
              : 'opacity-0 hover:opacity-100 focus-within:opacity-100'
          }`}
        >
          <div className="glass flex items-center gap-3 rounded-xl px-3 py-2 shadow-lg">
            <div className="flex items-center gap-2">
              {iconStyle && (
                <motion.div
                  whileHover={noAnimation ? undefined : { scale: 1.1 }}
                  whileTap={noAnimation ? undefined : { scale: 0.95 }}
                >
                  <TappIconBadge
                    icon={tapp.manifest.icon}
                    iconSvg={tapp.manifest.iconSvg}
                    name={displayName}
                    id={tapp.manifest.id}
                    themeColor={tapp.manifest.themeColor}
                    category={tapp.manifest.category}
                    permissions={tapp.manifest.permissions}
                    iconStyle={iconStyle}
                    shellClassName="tapp-page-icon tapp-page-icon--sm"
                    glyphSizeClass="w-4 h-4"
                    glyphTextClass="text-sm"
                  />
                </motion.div>
              )}
              <div className="hidden sm:block">
                <h1 className="text-xs font-semibold leading-tight text-gray-800 dark:text-gray-100">
                  {displayName}
                </h1>
                <p className="text-[10px] text-gray-500 dark:text-gray-400">
                  v{tapp.manifest.version}
                </p>
              </div>
            </div>
            <div className="h-6 w-px bg-gray-200 dark:bg-neutral-700" />
            <div className="flex items-center gap-1">
              <motion.button
                onClick={toggleFullscreen}
                className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-700 dark:hover:bg-neutral-700 dark:hover:text-gray-300"
                title={t.tapp.exitFullscreen}
                whileHover={noAnimation ? undefined : { scale: 1.1 }}
                whileTap={noAnimation ? undefined : { scale: 0.9 }}
              >
                <FaCompress className="h-3.5 w-3.5" />
              </motion.button>
              {canStartStop && (
                <motion.button
                  onClick={handleStop}
                  className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-900/20"
                  title={t.tapp.stopApp}
                  whileHover={noAnimation ? undefined : { scale: 1.1 }}
                  whileTap={noAnimation ? undefined : { scale: 0.9 }}
                >
                  <FaPause className="h-3.5 w-3.5" />
                </motion.button>
              )}
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )

  return (
    <TappAppShell
      shellAttr="data-tapp-run-shell"
      isMobile={isMobile}
      isFullscreen={isFullscreen}
      presence={presence}
      fullscreenToolbar={fsToolbar}
      onBack={goBack}
      backTitle={t.tapp.back}
      backAriaLabel={t.tapp.backToAppList}
      contentClassName="bg-gray-100 dark:bg-neutral-900"
      contentStyle={contentLayerStyle}
      headerLeading={
        <AnimatePresence mode="wait" initial={false}>
          {loading ? (
            <motion.div
              key="header-loading"
              className="flex items-center gap-2"
              initial={{ opacity: 0, x: -8 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 8 }}
              transition={transitions.stateSwitch}
            >
              <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-gray-200 dark:bg-neutral-700">
                <Spinner size="sm" />
              </div>
            </motion.div>
          ) : hasError ? (
            <motion.div
              key="header-error"
              className="flex min-w-0 items-center gap-2"
              initial={{ opacity: 0, x: -8 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 8 }}
              transition={transitions.stateSwitch}
            >
              <motion.div
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-red-100 dark:bg-red-900/30"
                initial={{ scale: 0.8 }}
                animate={{ scale: 1 }}
                transition={{ type: 'spring', stiffness: 400, damping: 20 }}
              >
                <FaExclamationTriangle className="h-4 w-4 text-red-500" />
              </motion.div>
              <span className="truncate text-sm text-red-600 dark:text-red-400">
                {error || t.tapp.appNotExist}
              </span>
            </motion.div>
          ) : tapp && iconStyle ? (
            <motion.div
              key="header-ready"
              className="flex min-w-0 items-center gap-2"
              initial={{ opacity: 0, x: -8 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 8 }}
              transition={transitions.stateSwitch}
            >
              <motion.div
                className="shrink-0"
                initial={{ scale: 0.8, rotate: -10 }}
                animate={{ scale: 1, rotate: 0 }}
                transition={{ type: 'spring', stiffness: 400, damping: 20 }}
                whileHover={noAnimation ? undefined : { scale: 1.1, rotate: 5 }}
                whileTap={noAnimation ? undefined : { scale: 0.95 }}
              >
                <TappIconBadge
                  icon={tapp.manifest.icon}
                  iconSvg={tapp.manifest.iconSvg}
                  name={displayName}
                  id={tapp.manifest.id}
                  themeColor={tapp.manifest.themeColor}
                  category={tapp.manifest.category}
                  permissions={tapp.manifest.permissions}
                  iconStyle={iconStyle}
                  shellClassName="tapp-page-icon tapp-page-icon--sm"
                  glyphSizeClass="w-4 h-4"
                  glyphTextClass="text-sm"
                />
              </motion.div>
              <motion.span
                className="truncate text-sm font-semibold text-gray-800 dark:text-gray-100"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ delay: 0.1 }}
              >
                {displayName}
              </motion.span>
              <motion.span
                className="shrink-0 text-[10px] text-gray-400 dark:text-gray-500"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ delay: 0.15 }}
              >
                v{tapp.manifest.version}
              </motion.span>
            </motion.div>
          ) : null}
        </AnimatePresence>
      }
      headerActions={
        <AnimatePresence mode="wait" initial={false}>
          {hasError ? (
            <motion.div
              key="actions-error"
              className="flex shrink-0 items-center gap-1"
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }}
              transition={transitions.stateSwitch}
            >
              <motion.button
                onClick={handleRetry}
                className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-indigo-50 hover:text-indigo-600 dark:hover:bg-indigo-900/20 dark:hover:text-indigo-400"
                title={t.tapp.retry}
                whileHover={
                  noAnimation ? undefined : { scale: 1.1, rotate: 180 }
                }
                whileTap={noAnimation ? undefined : { scale: 0.9 }}
              >
                <FaRedo className="h-3.5 w-3.5" />
              </motion.button>
            </motion.div>
          ) : isReady ? (
            <motion.div
              key="actions-ready"
              className="flex shrink-0 items-center gap-1"
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }}
              transition={transitions.stateSwitch}
            >
              {!isMobile && !isWebKit && (
                <motion.button
                  onClick={() => navigate(tappRunPath(tappId, { multi: true }))}
                  className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-indigo-50 hover:text-indigo-600 dark:hover:bg-indigo-900/20 dark:hover:text-indigo-400"
                  title={t.tapp.multiWindow}
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ delay: 0 }}
                  whileHover={noAnimation ? undefined : { scale: 1.15 }}
                  whileTap={noAnimation ? undefined : { scale: 0.9 }}
                >
                  <FaTh className="h-3.5 w-3.5" />
                </motion.button>
              )}
              <motion.button
                onClick={() =>
                  window.dispatchEvent(
                    new CustomEvent('arael-open-session', {
                      detail: { sessionId: '' },
                    }),
                  )
                }
                className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-indigo-50 hover:text-indigo-600 dark:hover:bg-indigo-900/20 dark:hover:text-indigo-400"
                title={t.arael.askArael}
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ delay: 0.02 }}
                whileHover={noAnimation ? undefined : { scale: 1.15 }}
                whileTap={noAnimation ? undefined : { scale: 0.9 }}
              >
                <FaComments className="h-3.5 w-3.5" />
              </motion.button>
              <motion.button
                onClick={toggleFullscreen}
                className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-700 dark:hover:bg-neutral-700 dark:hover:text-gray-300"
                title={t.tapp.fullscreen}
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ delay: 0.05 }}
                whileHover={noAnimation ? undefined : { scale: 1.15 }}
                whileTap={noAnimation ? undefined : { scale: 0.9 }}
              >
                <FaExpand className="h-3.5 w-3.5" />
              </motion.button>
              {canConfigure && (
                <motion.button
                  onClick={openSettings}
                  className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-700 dark:hover:bg-neutral-700 dark:hover:text-gray-300"
                  title={t.tapp.settings}
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ delay: 0.1 }}
                  whileHover={
                    noAnimation ? undefined : { scale: 1.1, rotate: 45 }
                  }
                  whileTap={noAnimation ? undefined : { scale: 0.9 }}
                >
                  <FaCog className="h-3.5 w-3.5" />
                </motion.button>
              )}
              {canStartStop && (
                <motion.button
                  onClick={handleStop}
                  className="rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-900/20"
                  title={t.tapp.stopApp}
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ delay: 0.15 }}
                  whileHover={noAnimation ? undefined : { scale: 1.1 }}
                  whileTap={noAnimation ? undefined : { scale: 0.9 }}
                >
                  <FaPause className="h-3.5 w-3.5" />
                </motion.button>
              )}
            </motion.div>
          ) : (
            <motion.div
              key="actions-loading"
              className="flex shrink-0 items-center gap-1"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            />
          )}
        </AnimatePresence>
      }
    >
      <AnimatePresence mode="wait" initial={false}>
        {loading ? (
          <div
            key="sandbox-loading"
            className="absolute inset-0 bg-gray-100 dark:bg-neutral-900"
          />
        ) : hasError ? (
          <motion.div
            key="sandbox-error"
            className="absolute inset-0 flex items-center justify-center bg-gray-100 dark:bg-neutral-900"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, scale: 0.98 }}
            transition={transitions.stateSwitch}
          >
            <div className="mx-4 max-w-sm text-center">
              <FaExclamationTriangle className="mx-auto mb-3 h-10 w-10 text-red-500" />
              <h3 className="mb-2 font-medium text-gray-800 dark:text-gray-100">
                {t.tapp.cannotLoadApp}
              </h3>
              <p className="mb-4 text-sm text-gray-500 dark:text-gray-400">
                {error || t.tapp.appNotExist}
              </p>
              <button
                onClick={handleRetry}
                className="inline-flex items-center gap-2 rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700"
              >
                <FaRedo className="h-3.5 w-3.5" />
                {t.tapp.retry}
              </button>
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>

      {tapp && code && (
        <div className="absolute inset-0">
          <TappPageSandbox
            tappInstance={tapp}
            code={code}
            onError={(err: Error) => console.error('[TappRunPage] Error:', err)}
            safeInsets={safeInsets}
          />
        </div>
      )}
    </TappAppShell>
  )
}

export default TappRunPage
