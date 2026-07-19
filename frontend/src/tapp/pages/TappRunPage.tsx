/**
 * Tapp 运行页面
 * 在沙箱中运行 Tapp
 *
 * 🎯 重要设计：
 * - TappPageSandbox 只渲染一次，通过 CSS 切换全屏/普通模式
 * - 这样避免了切换全屏时 iframe 被销毁重建，保持应用状态
 * - 加载状态整合到顶部控制条，避免页面级状态切换
 * - WebKit 浏览器使用独立的 TappRunPageWebKit 组件
 * - 支持多窗口模式，可同时运行最多3个应用
 */

import type { TappCodeStructure, TappInstance } from '../types'

import {
  FaArrowLeft,
  FaCog,
  FaCompress,
  FaExclamationTriangle,
  FaExpand,
  FaPause,
  FaRedo,
  FaSpinner,
  FaTh,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { useNavigation } from '../../contexts/NavigationContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useBreakpoints } from '../../hooks/useSharedEventListener'
import { TappIcon } from '../components/TappIcon'
import { TappWindowManager } from '../components/TappWindowManager'
import { getTappRuntime } from '../runtime'
import { loadPageResources } from '../runtime/sandbox/resourceLoader'
import { isWebKit, TappPageSandbox } from '../runtime/TappPageSandbox'
import { getTappIconStyle } from '../utils/tappColors'

interface TappRunPageProps {
  tappId: string
}

/**
 * Tapp 运行页面入口
 * 支持单窗口模式和多窗口模式
 */
export function TappRunPage({ tappId }: TappRunPageProps) {
  const [searchParams] = useSearchParams()
  const { isMobile } = useBreakpoints()
  // 多窗口模式仅限平板和PC端，Safari/WebKit 不支持多窗口
  const isMultiWindow =
    searchParams.get('multi') === 'true' && !isMobile && !isWebKit
  const navigate = useNavigate()

  // 多窗口模式
  if (isMultiWindow) {
    return (
      <TappWindowManager
        initialTappId={tappId}
        onBack={() => navigate('/tapp')}
      />
    )
  }

  // 单窗口模式（默认）
  return <TappRunPageStandard tappId={tappId} isMobile={isMobile} />
}

interface TappRunPageStandardProps extends TappRunPageProps {
  isMobile: boolean
}

/**
 * 标准版 Tapp 运行页面组件（非 WebKit）
 */
function TappRunPageStandard({ tappId, isMobile }: TappRunPageStandardProps) {
  const navigate = useNavigate()
  const { t } = useI18n()
  const { setImmersiveMode } = useNavigation()

  // 动画配置
  const animConfig = useAnimationLevel()
  const noAnimation = animConfig.level === 'none'

  const [tapp, setTapp] = useState<TappInstance | null>(null)
  const [code, setCode] = useState<TappCodeStructure | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [retryGeneration, setRetryGeneration] = useState(0)
  const runtime = getTappRuntime()

  // 全屏时沉浸隐藏 NavigationIsland（z-50），而不是把 TApp 抬到 ≥998
  // 与 host chrome z-ladder 一致：TApp shell 保持远低于 GCP overlay(998)/bar(9999)
  useEffect(() => {
    setImmersiveMode(isFullscreen)
    return () => {
      setImmersiveMode(false)
    }
  }, [isFullscreen, setImmersiveMode])

  // 加载 Tapp
  useEffect(() => {
    let cancelled = false
    const loadTapp = async () => {
      setLoading(true)
      setError(null)
      setTapp(null)
      setCode(null)
      try {
        await runtime.waitForSync()
        if (cancelled) return

        const instance = runtime.getTapp(tappId)
        if (!instance) {
          setError(t.tapp.appNotExist)
          setLoading(false)
          return
        }

        const resources = await loadPageResources(instance)
        if (cancelled) return

        const tappCode: TappCodeStructure = {
          core: resources.core,
          page: resources.page,
          pageHtml: resources.html,
          styles: resources.styles,
          pageCSS: resources.css,
          i18n: resources.i18n,
          pageModules: resources.pageModules,
          pageModuleOrder: resources.pageModuleOrder,
        }

        if (!runtime.isRunning(tappId)) {
          await runtime.startTapp(tappId)
        }
        if (cancelled) return

        setTapp(instance)
        setCode(tappCode)
        setLoading(false)
      } catch (err) {
        if (cancelled) return
        setError(err instanceof Error ? err.message : t.tapp.loadAppFailed)
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

  // 返回
  const goBack = useCallback(() => {
    navigate('/tapp')
  }, [navigate])

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
    navigate(`/tapp/detail/${tappId}`)
  }, [navigate, tappId])

  // 🎯 稳定的 safeInsets 对象，避免每次渲染都创建新对象
  const safeInsets = useMemo(() => {
    return isFullscreen
      ? { top: 72, right: 16, left: 16, bottom: 0 }
      : undefined
  }, [isFullscreen])

  // 🎬 动画配置 - 基于性能级别
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

  // 🎯 内容状态
  const isReady = !loading && !error && !!tapp && !!code
  const hasError = !loading && (error || !tapp || !code)

  // 权限检查（只有在 tapp 存在时才有意义）
  const canStartStop = tapp?.userRole !== 'guest'
  const canConfigure = tapp?.userRole !== 'guest'
  const iconStyle = tapp ? getTappIconStyle(tapp.manifest) : null

  // 🎯 统一渲染：始终显示相同的页面结构，只是内容不同
  // 页面级动画由 App.tsx 的 FixedPageWrapper 提供（纯 opacity，不用 transform）
  //
  // Host chrome z-ladder（勿把 TApp 抬过 GCP）:
  // - TApp shell / content: 40（本壳；仍在 main z-10 内）
  // - NavigationIsland: 50 → 全屏时 immersive 隐藏，而不是抬 TApp z
  // - TApp 全屏工具栏: 900（< GCP overlay 998 / bar 9999）
  // - GCP overlay: 998 · GCP bar: 9999
  // 面板展开时 html.gcp-panel-open 会关闭本壳 pointer-events，防止 iframe 吞触摸
  return (
    <div
      data-tapp-run-shell=""
      style={{
        position: 'fixed',
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
        overflow: 'hidden',
        // 保持远低于 GCP（998/9999）。导航岛遮挡由全屏 immersive 解决，不用抬 z。
        zIndex: 40,
      }}
    >
      {/* 全屏模式工具栏 */}
      {(() => {
        const toolbar = (
          <AnimatePresence>
            {isFullscreen && isReady && tapp && (
              <motion.div
                key="fullscreen-toolbar"
                initial={{ opacity: 0, x: -16, scale: 0.92 }}
                animate={{ opacity: 1, x: 0, scale: 1 }}
                exit={{ opacity: 0, x: -16, scale: 0.92 }}
                transition={transitions.elementEnter}
                // z-900：高于内容/导航岛，但低于 GCP overlay(998) 与 bar(9999)
                className="fixed top-4 left-4 z-900 opacity-0 hover:opacity-100 transition-opacity duration-300"
              >
                <div className="glass rounded-xl px-3 py-2 flex items-center gap-3 shadow-lg">
                  <div className="flex items-center gap-2">
                    {iconStyle && (
                      <motion.div
                        className={`w-7 h-7 rounded-lg ${iconStyle.className} flex items-center justify-center text-white text-xs font-bold`}
                        style={iconStyle.style}
                        whileHover={noAnimation ? undefined : { scale: 1.1 }}
                        whileTap={noAnimation ? undefined : { scale: 0.95 }}
                      >
                        <TappIcon
                          icon={tapp.manifest.icon}
                          iconSvg={tapp.manifest.iconSvg}
                          name={tapp.manifest.name}
                          sizeClass="w-4 h-4"
                          textSizeClass="text-sm"
                        />
                      </motion.div>
                    )}
                    <div className="hidden sm:block">
                      <h1 className="font-semibold text-gray-800 dark:text-gray-100 text-xs leading-tight">
                        {tapp.manifest.name}
                      </h1>
                      <p className="text-[10px] text-gray-500 dark:text-gray-400">
                        v{tapp.manifest.version}
                      </p>
                    </div>
                  </div>
                  <div className="w-px h-6 bg-gray-200 dark:bg-neutral-700" />
                  <div className="flex items-center gap-1">
                    <motion.button
                      onClick={toggleFullscreen}
                      className="p-1.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
                      title={t.tapp.exitFullscreen}
                      whileHover={noAnimation ? undefined : { scale: 1.1 }}
                      whileTap={noAnimation ? undefined : { scale: 0.9 }}
                    >
                      <FaCompress className="w-3.5 h-3.5" />
                    </motion.button>
                    {canStartStop && (
                      <motion.button
                        onClick={handleStop}
                        className="p-1.5 text-gray-500 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors"
                        title={t.tapp.stopApp}
                        whileHover={noAnimation ? undefined : { scale: 1.1 }}
                        whileTap={noAnimation ? undefined : { scale: 0.9 }}
                      >
                        <FaPause className="w-3.5 h-3.5" />
                      </motion.button>
                    )}
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        )
        // Safari/WebKit: portal 到 body，避免被页面 fixed 壳层叠层影响
        return isWebKit ? createPortal(toolbar, document.body) : toolbar
      })()}

      {/* 🎯 普通模式 - 控制栏 + 沙箱作为一个整体 */}
      <motion.div
        className="absolute inset-0 flex flex-col overflow-hidden pointer-events-none"
        initial={noAnimation ? false : { opacity: 0, y: 35, scale: 0.95 }}
        animate={{
          opacity: isFullscreen ? 0 : 1,
          y: isFullscreen ? -30 : 0,
          scale: isFullscreen ? 0.92 : 1,
        }}
        transition={{
          type: 'spring',
          stiffness: 350,
          damping: 32,
          mass: 0.8,
        }}
        style={{ pointerEvents: isFullscreen ? 'none' : undefined }}
      >
        {/* 顶部间距 */}
        <div className="h-20 shrink-0" />

        {/* 控制栏 + 沙箱 整体容器 */}
        <div className="flex-1 flex flex-col px-4 sm:px-6 min-h-0 pb-6">
          {/* Full remaining width — do not cap at max-w-6xl (left a dead strip beside full-bleed apps like Aro). */}
          <div className="w-full flex flex-col flex-1 min-h-0 max-h-[calc(100vh-8rem)]">
            {/* 头部卡片 - 紧凑单行 */}
            <div className="glass rounded-t-xl px-3 py-2 flex items-center justify-between gap-2 shadow-sm min-h-11 shrink-0 pointer-events-auto">
              {/* 左侧：返回 + 状态/图标 + 名称 */}
              <div className="flex items-center gap-2 min-w-0">
                <motion.button
                  onClick={goBack}
                  className="p-1.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors shrink-0"
                  title={t.tapp.back}
                  aria-label={t.tapp.backToAppList}
                  whileHover={noAnimation ? undefined : { scale: 1.1, x: -2 }}
                  whileTap={noAnimation ? undefined : { scale: 0.9 }}
                >
                  <FaArrowLeft className="w-4 h-4" />
                </motion.button>

                {/* 根据状态显示不同内容 - 使用 AnimatePresence 实现平滑切换 */}
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
                      <div className="w-7 h-7 rounded-lg bg-gray-200 dark:bg-neutral-700 flex items-center justify-center shrink-0">
                        <FaSpinner className="w-4 h-4 text-gray-400 animate-spin" />
                      </div>
                      <span className="text-sm text-gray-500 dark:text-gray-400">
                        {t.tapp.loadingApp}
                      </span>
                    </motion.div>
                  ) : hasError ? (
                    <motion.div
                      key="header-error"
                      className="flex items-center gap-2 min-w-0"
                      initial={{ opacity: 0, x: -8 }}
                      animate={{ opacity: 1, x: 0 }}
                      exit={{ opacity: 0, x: 8 }}
                      transition={transitions.stateSwitch}
                    >
                      <motion.div
                        className="w-7 h-7 rounded-lg bg-red-100 dark:bg-red-900/30 flex items-center justify-center shrink-0"
                        initial={{ scale: 0.8 }}
                        animate={{ scale: 1 }}
                        transition={{
                          type: 'spring',
                          stiffness: 400,
                          damping: 20,
                        }}
                      >
                        <FaExclamationTriangle className="w-4 h-4 text-red-500" />
                      </motion.div>
                      <span className="text-sm text-red-600 dark:text-red-400 truncate">
                        {error || t.tapp.appNotExist}
                      </span>
                    </motion.div>
                  ) : tapp && iconStyle ? (
                    <motion.div
                      key="header-ready"
                      className="flex items-center gap-2 min-w-0"
                      initial={{ opacity: 0, x: -8 }}
                      animate={{ opacity: 1, x: 0 }}
                      exit={{ opacity: 0, x: 8 }}
                      transition={transitions.stateSwitch}
                    >
                      <motion.div
                        className={`w-7 h-7 rounded-lg ${iconStyle.className} flex items-center justify-center text-white text-sm font-bold shrink-0`}
                        style={iconStyle.style}
                        initial={{ scale: 0.8, rotate: -10 }}
                        animate={{ scale: 1, rotate: 0 }}
                        transition={{
                          type: 'spring',
                          stiffness: 400,
                          damping: 20,
                        }}
                        whileHover={
                          noAnimation ? undefined : { scale: 1.1, rotate: 5 }
                        }
                        whileTap={noAnimation ? undefined : { scale: 0.95 }}
                      >
                        <TappIcon
                          icon={tapp.manifest.icon}
                          iconSvg={tapp.manifest.iconSvg}
                          name={tapp.manifest.name}
                          sizeClass="w-4 h-4"
                          textSizeClass="text-sm"
                        />
                      </motion.div>
                      <motion.span
                        className="text-sm font-semibold text-gray-800 dark:text-gray-100 truncate"
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        transition={{ delay: 0.1 }}
                      >
                        {tapp.manifest.name}
                      </motion.span>
                      <motion.span
                        className="text-[10px] text-gray-400 dark:text-gray-500 shrink-0"
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        transition={{ delay: 0.15 }}
                      >
                        v{tapp.manifest.version}
                      </motion.span>
                    </motion.div>
                  ) : null}
                </AnimatePresence>
              </div>

              {/* 右侧：操作按钮 - 使用 AnimatePresence 实现平滑切换 */}
              <AnimatePresence mode="wait" initial={false}>
                {hasError ? (
                  <motion.div
                    key="actions-error"
                    className="flex items-center gap-1 shrink-0"
                    initial={{ opacity: 0, scale: 0.9 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.9 }}
                    transition={transitions.stateSwitch}
                  >
                    <motion.button
                      onClick={handleRetry}
                      className="p-1.5 text-gray-500 hover:text-indigo-600 dark:hover:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/20 rounded-lg transition-colors"
                      title={t.tapp.retry}
                      whileHover={
                        noAnimation ? undefined : { scale: 1.1, rotate: 180 }
                      }
                      whileTap={noAnimation ? undefined : { scale: 0.9 }}
                    >
                      <FaRedo className="w-3.5 h-3.5" />
                    </motion.button>
                  </motion.div>
                ) : isReady ? (
                  <motion.div
                    key="actions-ready"
                    className="flex items-center gap-1 shrink-0"
                    initial={{ opacity: 0, scale: 0.9 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.9 }}
                    transition={transitions.stateSwitch}
                  >
                    {/* 多窗口模式按钮 - 仅平板和PC端显示，Safari 不支持 */}
                    {!isMobile && !isWebKit && (
                      <motion.button
                        onClick={() =>
                          navigate(`/tapp/run/${tappId}?multi=true`)
                        }
                        className="p-1.5 text-gray-500 hover:text-indigo-600 dark:hover:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/20 rounded-lg transition-colors"
                        title={t.tapp.multiWindow}
                        initial={{ opacity: 0, y: 4 }}
                        animate={{ opacity: 1, y: 0 }}
                        transition={{ delay: 0 }}
                        whileHover={noAnimation ? undefined : { scale: 1.15 }}
                        whileTap={noAnimation ? undefined : { scale: 0.9 }}
                      >
                        <FaTh className="w-3.5 h-3.5" />
                      </motion.button>
                    )}
                    <motion.button
                      onClick={toggleFullscreen}
                      className="p-1.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
                      title={t.tapp.fullscreen}
                      initial={{ opacity: 0, y: 4 }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{ delay: 0.05 }}
                      whileHover={noAnimation ? undefined : { scale: 1.15 }}
                      whileTap={noAnimation ? undefined : { scale: 0.9 }}
                    >
                      <FaExpand className="w-3.5 h-3.5" />
                    </motion.button>
                    {canConfigure && (
                      <motion.button
                        onClick={openSettings}
                        className="p-1.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
                        title={t.tapp.settings}
                        initial={{ opacity: 0, y: 4 }}
                        animate={{ opacity: 1, y: 0 }}
                        transition={{ delay: 0.1 }}
                        whileHover={
                          noAnimation ? undefined : { scale: 1.1, rotate: 45 }
                        }
                        whileTap={noAnimation ? undefined : { scale: 0.9 }}
                      >
                        <FaCog className="w-3.5 h-3.5" />
                      </motion.button>
                    )}
                    {canStartStop && (
                      <motion.button
                        onClick={handleStop}
                        className="p-1.5 text-gray-500 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors"
                        title={t.tapp.stopApp}
                        initial={{ opacity: 0, y: 4 }}
                        animate={{ opacity: 1, y: 0 }}
                        transition={{ delay: 0.15 }}
                        whileHover={noAnimation ? undefined : { scale: 1.1 }}
                        whileTap={noAnimation ? undefined : { scale: 0.9 }}
                      >
                        <FaPause className="w-3.5 h-3.5" />
                      </motion.button>
                    )}
                  </motion.div>
                ) : (
                  <motion.div
                    key="actions-loading"
                    className="flex items-center gap-1 shrink-0"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                  />
                )}
              </AnimatePresence>
            </div>

            {/* 沙箱区域 - 与控制栏在同一容器内；真实 iframe 叠在外层绝对定位壳上 */}
            <div className="flex-1 min-h-0 rounded-b-xl overflow-hidden bg-gray-100 dark:bg-neutral-900 pointer-events-none">
              {/* 根据状态显示不同内容 */}
              <AnimatePresence mode="wait" initial={false}>
                {loading ? (
                  <div
                    key="sandbox-loading"
                    className="w-full h-full bg-gray-100 dark:bg-neutral-900"
                  />
                ) : hasError ? (
                  <motion.div
                    key="sandbox-error"
                    className="w-full h-full flex items-center justify-center bg-gray-100 dark:bg-neutral-900 pointer-events-auto"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0, scale: 0.98 }}
                    transition={transitions.stateSwitch}
                  >
                    <div className="text-center max-w-sm mx-4">
                      <FaExclamationTriangle className="w-10 h-10 mx-auto text-red-500 mb-3" />
                      <h3 className="text-gray-800 dark:text-gray-100 font-medium mb-2">
                        {t.tapp.cannotLoadApp}
                      </h3>
                      <p className="text-gray-500 dark:text-gray-400 text-sm mb-4">
                        {error || t.tapp.appNotExist}
                      </p>
                      <button
                        onClick={handleRetry}
                        className="inline-flex items-center gap-2 px-4 py-2 bg-indigo-600 hover:bg-indigo-700 text-white text-sm font-medium rounded-lg transition-colors"
                      >
                        <FaRedo className="w-3.5 h-3.5" />
                        {t.tapp.retry}
                      </button>
                    </div>
                  </motion.div>
                ) : tapp && code ? (
                  /* 沙箱占位 - 实际沙箱在外层渲染，这里只是占位保持布局 */
                  <div className="w-full h-full" />
                ) : null}
              </AnimatePresence>
            </div>
          </div>
        </div>
      </motion.div>

      {/* 沙箱容器 - 只渲染一次，通过 CSS 切换全屏/普通模式
          使用原生 div + CSS transition（避免 framer-motion 干扰布局/触摸）
          iframe 内联在此壳内，必须 pointer-events-auto 才能收到点击 */}
      {tapp && code && (
        <div
          className={`pointer-events-auto overflow-hidden transition-all duration-300 ease-out ${
            isFullscreen ? 'rounded-none' : 'rounded-b-xl'
          }`}
          style={
            isFullscreen
              ? {
                  // 全屏层：仍在 shell(z-40)/main(z-10) 内，绝不 ≥ GCP 998
                  position: 'fixed',
                  top: 0,
                  right: 0,
                  bottom: 0,
                  left: 0,
                  zIndex: 1,
                }
              : {
                  position: 'absolute',
                  top: 'calc(5rem + 44px)',
                  right: '1rem',
                  bottom: '1.5rem',
                  left: '1rem',
                  zIndex: 1,
                  // No maxWidth — full remaining viewport (1rem inset). Capping at 72rem
                  // left a dead empty strip on wide screens for full-bleed apps (Aro feed).
                  // 独立合成层：减轻 WebKit 在 overflow:hidden 祖先下的 iframe 绘制问题
                  // （页面级 opacity 动画已在 App.tsx 对 /tapp/run 关闭）
                  // 仅非全屏使用 translateZ；全屏避免额外合成层干扰 host chrome hit-test
                  WebkitTransform: 'translateZ(0)',
                  transform: 'translateZ(0)',
                  isolation: 'isolate',
                }
          }
        >
          <div
            className="bg-gray-100 dark:bg-neutral-900"
            style={{
              position: 'absolute',
              top: 0,
              right: 0,
              bottom: 0,
              left: 0,
            }}
          >
            <TappPageSandbox
              tappInstance={tapp}
              code={code}
              onError={(err: Error) =>
                console.error('[TappRunPage] Error:', err)
              }
              safeInsets={safeInsets}
            />
          </div>
        </div>
      )}
    </div>
  )
}

export default TappRunPage
