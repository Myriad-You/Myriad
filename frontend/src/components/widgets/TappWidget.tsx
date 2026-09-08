/**
 * Tapp Widget 组件
 * 用于在 Dashboard 中渲染 Tapp 提供的小组件
 *
 * 使用 TappWidgetSandbox 实现 Widget 范围的 Tapp SDK API
 *
 * 架构说明：
 * - Widget 从 manifest 预注册，安装后即可在 Dashboard 中添加
 * - 只有当 Tapp 运行中时，Widget 才会真正渲染
 * - 未运行时显示提示，引导用户启动 Tapp
 *
 * 预览模式优化：
 * - 预览模式下渲染美观的 Glass 风格预览卡片
 * - 支持图标、名称、主题色
 * - 添加光晕背景效果，与普通小组件保持一致
 */

import type {
  RegisteredWidget,
  TappCodeStructure,
  TappInstance,
} from '../../tapp/types'
import type { WidgetComponentProps } from '../widgetGridTypes'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { isPageVisible, onVisibility } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { TappIconBadge } from '../../tapp/components/TappIconBadge'
import { loadWidgetResources } from '../../tapp/runtime/sandbox/resourceLoader'
import { getTappRuntime } from '../../tapp/runtime/TappRuntime'
import { TappWidgetSandbox } from '../../tapp/runtime/TappWidgetSandbox'
import { widgetPerfMark } from '../../tapp/runtime/WidgetLoadPerf'
import { onTappWidgetInvalidate } from '../../tapp/runtime/WidgetRuntimeSignals'
import { resolveManifestText } from '../../tapp/utils/manifestLocale'
import { getTappIconStyle } from '../../tapp/utils/tappColors'
import { tappDetailPath } from '../../tapp/utils/tappPaths'
import { userFacingError } from '../../utils/userFacingError'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'
import {
  TAPP_WIDGET_SKELETON,
  WidgetSkeleton,
  WidgetSkeletonCover,
} from './shared/WidgetSkeleton'
import {
  intersectionKeepsTappWidgetMounted,
  TAPP_WIDGET_VIEWPORT_OFFSCREEN_RECHECKS,
  TAPP_WIDGET_VIEWPORT_RECHECK_MS,
} from './tappWidgetViewport'

export interface TappWidgetProps extends WidgetComponentProps {
  /** Tapp Widget 完整 ID (tapp.{tappId}.{widgetId}) */
  tappWidgetId: string
}

/**
 * 获取 Tapp Widget 的预览信息
 * 同步方法，从 runtime 缓存中获取信息
 */
function getTappWidgetPreviewInfo(
  tappWidgetId: string,
  locale?: string,
): {
  name: string
  icon?: string
  iconSvg?: string
  themeColor?: string
  category?: string
  id?: string
  permissions?: string[]
  description?: string
  tappName?: string
} | null {
  try {
    const runtime = getTappRuntime()
    const widgets = runtime.getRegisteredWidgets()
    const widget = widgets.find((w) => w.id === tappWidgetId)

    if (!widget) {
      // 从 ID 提取基本信息
      const parts = tappWidgetId.split('.')
      const widgetName = parts.pop() || 'Widget'
      return {
        name: widgetName,
        icon: undefined,
        iconSvg: undefined,
      }
    }

    // 获取 Tapp 实例以获取主题色 / 官方 mark id
    const tapp = runtime.getTapp(widget.tappId)

    return {
      name: widget.config.name || 'Widget',
      icon: widget.config.icon || tapp?.manifest.icon,
      iconSvg: tapp?.manifest.iconSvg,
      themeColor: tapp?.manifest.themeColor,
      category: tapp?.manifest.category,
      id: tapp?.manifest.id || widget.tappId,
      permissions: tapp?.manifest.permissions,
      description: widget.config.description,
      tappName: tapp
        ? resolveManifestText(tapp.manifest, locale).name
        : undefined,
    }
  } catch {
    // 返回默认值
    const parts = tappWidgetId.split('.')
    const widgetName = parts.pop() || 'Widget'
    return {
      name: widgetName,
      icon: undefined,
      iconSvg: undefined,
    }
  }
}

/**
 * Tapp Widget 预览组件
 * 用于在小组件库中显示实际的 widget 渲染效果
 *
 * 优化策略：
 * - 尝试渲染实际的 widget HTML 内容
 * - 只渲染一次，不监听任何更新事件（节约性能）
 * - 如果无法获取代码则回退到静态预览
 */
const TappWidgetPreview = memo(
  ({
    tappWidgetId,
    config,
    animLevel,
  }: {
    tappWidgetId: string
    config: WidgetComponentProps['config']
    animLevel: 'exlight' | 'light' | 'standard'
  }) => {
    const { locale } = useI18n()

    // 使用 ref 确保只加载一次（但尺寸变化时需要重新加载）
    const loadedRef = useRef(false)
    const prevSizeRef = useRef(config?.size)
    const prevTappWidgetIdRef = useRef(tappWidgetId)
    const [previewData, setPreviewData] = useState<{
      tappInstance: TappInstance
      code: TappCodeStructure
      widget: RegisteredWidget
    } | null>(null)
    /** loading → sandbox | static */
    const [previewPhase, setPreviewPhase] = useState<'loading' | 'static'>(
      'loading',
    )

    // 获取预览信息（用于回退显示）
    const previewInfo = useMemo(
      () => getTappWidgetPreviewInfo(tappWidgetId, locale),
      [tappWidgetId, locale],
    )

    // ⚡ 优化：监听尺寸变化，重新加载资源
    useEffect(() => {
      // 尺寸变化时重置加载状态，触发重新加载
      if (
        prevSizeRef.current !== config?.size ||
        prevTappWidgetIdRef.current !== tappWidgetId
      ) {
        prevSizeRef.current = config?.size
        prevTappWidgetIdRef.current = tappWidgetId
        loadedRef.current = false
        setPreviewData(null)
        setPreviewPhase('loading')
      }
    }, [config?.size, tappWidgetId])

    // 加载预览数据（尺寸变化时会重新触发）
    useEffect(() => {
      if (loadedRef.current) return
      loadedRef.current = true
      let cancelled = false

      const loadPreviewData = async () => {
        try {
          const runtime = getTappRuntime()

          // 等待 runtime 同步
          await runtime.waitForSync()

          // 查找 widget
          const widgets = runtime.getRegisteredWidgets()
          const widget = widgets.find((w) => w.id === tappWidgetId)
          if (!widget) {
            if (!cancelled) setPreviewPhase('static')
            return
          }

          // 获取 Tapp 实例
          const tapp = runtime.getTapp(widget.tappId)
          if (!tapp) {
            if (!cancelled) setPreviewPhase('static')
            return
          }

          // 检查 Tapp 是否运行中
          const running = runtime.isRunning(widget.tappId)
          if (!running) {
            if (!cancelled) setPreviewPhase('static')
            return
          }

          // 使用新的资源加载器获取 Widget 专用资源
          // ⚡ 优化：使用当前尺寸加载对应的资源
          const widgetSize = config?.size || widget.config.defaultSize || '4x2'

          try {
            const resources = await loadWidgetResources(
              tapp,
              widgetSize,
              widget.config.id,
            )

            // 转换为 TappWidgetSandbox 需要的 TappCodeStructure
            const tappCode: TappCodeStructure = {
              modules: resources.modules,
              moduleResolutions: resources.moduleResolutions,
              coreEntry: resources.coreEntry,
              widgetEntries: resources.widgetEntries,
              widgetHtml: resources.html,
              styles: resources.styles,
              widgetCSS: resources.css,
              i18n: resources.i18n,
            }

            if (!cancelled) {
              setPreviewData({ tappInstance: tapp, code: tappCode, widget })
            }
          } catch {
            if (!cancelled) setPreviewPhase('static')
          }
        } catch {
          if (!cancelled) setPreviewPhase('static')
        }
      }

      void loadPreviewData()
      return () => {
        cancelled = true
      }
    }, [tappWidgetId, config?.size])

    // 构造 widgetProps - 只计算一次
    const widgetProps = useMemo(() => {
      const isDark = document.documentElement.classList.contains('dark')
      const primaryColor =
        getComputedStyle(document.documentElement)
          .getPropertyValue('--color-primary')
          .trim() || '#8b5cf6'
      return {
        size: config.size,
        config: config.config || {},
        isEditMode: false,
        isPreview: true,
        theme: (isDark ? 'dark' : 'light') as 'light' | 'dark',
        primaryColor,
        locale,
      }
    }, [config.size, config.config, locale])

    // 如果有预览数据，渲染实际的 TappWidgetSandbox
    if (previewData) {
      return (
        <div
          className="relative w-full h-full rounded-xl overflow-hidden"
          style={{ pointerEvents: 'none' }}
        >
          <TappWidgetSandbox
            tappInstance={previewData.tappInstance}
            code={previewData.code}
            widgetId={
              previewData.widget.config.id ||
              previewData.widget.id.split('.').pop() ||
              ''
            }
            widgetProps={widgetProps}
            className="w-full h-full"
          />
          {/* 透明覆盖层 - 确保完全阻止交互 */}
          <div className="absolute inset-0 z-50" />
        </div>
      )
    }

    // Prefer CSS var — avoid getComputedStyle in render.
    const themeColor =
      previewInfo?.themeColor?.trim() || 'var(--color-primary, #6366f1)'

    // 加载中 / 退出淡出：盖在静态预览或空白上
    // 回退：显示静态预览

    // 根据尺寸判断布局
    const isCompact = config.size === '1x1' || config.size === '2x1'
    const isLarge =
      config.size === '4x2' || config.size === '4x4' || config.size === '2x4'

    const iconStyle = getTappIconStyle({
      icon: previewInfo?.icon,
      iconSvg: previewInfo?.iconSvg,
      themeColor: previewInfo?.themeColor,
      category: previewInfo?.category,
      id: previewInfo?.id,
      permissions: previewInfo?.permissions,
    })

    return (
      <div className="relative h-full w-full overflow-hidden rounded-xl">
        <WidgetShell
          padding={12}
          style={{ pointerEvents: 'none' }}
          contentClassName={`flex ${isCompact ? 'items-center justify-center' : 'flex-col justify-center items-center'}`}
          background={
            <>
              <GlowBackground
                color={themeColor}
                animLevel={animLevel}
                shouldAnimate={false}
                variant="single"
                size={isLarge ? 'lg' : 'md'}
                opacity={0.15}
              />
              {/* 边框效果 */}
              <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />
            </>
          }
        >
          {/* 图标 */}
          <TappIconBadge
            icon={previewInfo?.icon}
            iconSvg={previewInfo?.iconSvg}
            name={previewInfo?.name || 'Widget'}
            id={previewInfo?.id}
            themeColor={previewInfo?.themeColor}
            category={previewInfo?.category}
            permissions={previewInfo?.permissions}
            iconStyle={iconStyle}
            shellClassName={`tapp-page-icon ${
              isCompact
                ? 'w-8 h-8'
                : isLarge
                  ? 'w-14 h-14 mb-3'
                  : 'w-10 h-10 mb-2'
            }`}
            glyphSizeClass={
              isCompact ? 'w-5 h-5' : isLarge ? 'w-8 h-8' : 'w-6 h-6'
            }
            glyphTextClass={
              isCompact ? 'text-lg' : isLarge ? 'text-2xl' : 'text-xl'
            }
          />

          {/* 文本信息 - 紧凑模式不显示 */}
          {!isCompact && (
            <div className="text-center w-full px-2">
              <div
                className={`font-bold text-gray-800 dark:text-gray-100 truncate ${isLarge ? 'text-base mb-1' : 'text-sm'}`}
              >
                {previewInfo?.name || 'Widget'}
              </div>

              {/* 大尺寸显示描述 */}
              {isLarge && previewInfo?.description && (
                <div className="text-xs text-gray-500 dark:text-gray-400 line-clamp-2 leading-relaxed">
                  {previewInfo.description}
                </div>
              )}

              {/* Tapp 名称 - 仅大尺寸显示 */}
              {isLarge && previewInfo?.tappName && (
                <div className="mt-2 flex items-center justify-center gap-1.5">
                  <span className="text-[10px] px-2 py-0.5 rounded-full bg-black/5 dark:bg-white/10 text-gray-500 dark:text-gray-400">
                    {previewInfo.tappName}
                  </span>
                </div>
              )}
            </div>
          )}
        </WidgetShell>
        <WidgetSkeletonCover
          active={previewPhase === 'loading' && !previewData}
          preset={TAPP_WIDGET_SKELETON.preset}
          deferMs={TAPP_WIDGET_SKELETON.deferMs}
          accent={themeColor}
        />
      </div>
    )
  },
)

TappWidgetPreview.displayName = 'TappWidgetPreview'

/**
 * Tapp Widget 组件（使用 TappWidgetSandbox 隔离运行）
 */
interface TappWidgetRuntimeProps extends TappWidgetProps {
  anim: ReturnType<typeof useAnimationLevel>
}

function TappWidgetRuntime({
  config,
  isEditMode,
  isPreview,
  tappWidgetId,
  onConfigChange,
  anim,
}: TappWidgetRuntimeProps) {
  const runtime = getTappRuntime()
  const containerRef = useRef<HTMLDivElement>(null)
  const navigate = useNavigate()
  const { locale, t } = useI18n()
  const [widget, setWidget] = useState<RegisteredWidget | null>(null)
  const [tappInstance, setTappInstance] = useState<TappInstance | null>(null)
  const [code, setCode] = useState<TappCodeStructure | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [isRunning, setIsRunning] = useState(false)

  // ⚡ 记录上一次的尺寸，用于检测尺寸变化
  const prevSizeRef = useRef(config?.size)

  // 集成动画调度器的页面可见性感知
  // 页面不可见时跳过非必要的状态更新，减少后台 CPU 开销
  const [pageVisible, setPageVisible] = useState(isPageVisible())
  useEffect(() => {
    return onVisibility(setPageVisible)
  }, [])

  // 视口门控：widget 的 iframe 沙箱仅在进入视口（附近 300px）时挂载，
  // 远离视口则卸载以释放内存。需要后台常驻数据的 Tapp 由 TappBackgroundRunner
  // 用 headless core 保活，数据不丢；纯展示 widget 重新进入视口时重新挂载即可。
  // 默认 true 避免首屏闪烁；observer 首次回调会立即校正离屏项。
  //
  // 防误判（issue #72）：刷新后页面可能处于入场动画（transform 位移）、浏览器
  // 滚动位置恢复或布局未完成的窗口期。0×0 或未撑开的盒子不能当离屏——
  // 缓存命中时宿主挂得更早，首次回调更容易是空盒子；此后没有滚动/重排
  // 就不会再估，widget 会永久卡在 hold。非交叉且已有真实盒子时也不立即
  // 采纳：入场 spring/tween 约 480ms，复查几次再用稳定几何判定。
  const [inViewport, setInViewport] = useState(true)
  const viewportObserverRef = useRef<IntersectionObserver | null>(null)
  const viewportNodeRef = useRef<HTMLDivElement | null>(null)
  const viewportRecheckRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const viewportRecheckCountRef = useRef(0)
  // 复查等待中：期间到达的非交叉回调直接忽略，只有超时后 re-observe
  // 投递的结果才能决定离屏，避免等待期内动画/滚动抖动把 debounce 击穿。
  const viewportRecheckPendingRef = useRef(false)
  const sandboxHostRef = useCallback((node: HTMLDivElement | null) => {
    viewportObserverRef.current?.disconnect()
    viewportObserverRef.current = null
    if (viewportRecheckRef.current) {
      clearTimeout(viewportRecheckRef.current)
      viewportRecheckRef.current = null
    }
    viewportRecheckCountRef.current = 0
    viewportRecheckPendingRef.current = false
    viewportNodeRef.current = node
    if (!node || typeof IntersectionObserver === 'undefined') return
    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[0]
        if (!entry) return
        if (intersectionKeepsTappWidgetMounted(entry)) {
          viewportRecheckCountRef.current = 0
          viewportRecheckPendingRef.current = false
          setInViewport(true)
          return
        }
        // 复查等待中：忽略后续非交叉回调，等待 re-observe 的稳定结果。
        if (viewportRecheckPendingRef.current) return
        // 已复查足够次数（re-observe 后仍非交叉）才采纳离屏，
        // 保持屏外省电设计。
        if (
          viewportRecheckCountRef.current >=
          TAPP_WIDGET_VIEWPORT_OFFSCREEN_RECHECKS
        ) {
          setInViewport(false)
          return
        }
        viewportRecheckCountRef.current += 1
        viewportRecheckPendingRef.current = true
        if (viewportRecheckRef.current) clearTimeout(viewportRecheckRef.current)
        viewportRecheckRef.current = setTimeout(() => {
          viewportRecheckRef.current = null
          viewportRecheckPendingRef.current = false
          const host = viewportNodeRef.current
          const obs = viewportObserverRef.current
          if (!host || !obs || !host.isConnected) return
          obs.unobserve(host)
          obs.observe(host)
        }, TAPP_WIDGET_VIEWPORT_RECHECK_MS)
      },
      { rootMargin: '300px' },
    )
    observer.observe(node)
    viewportObserverRef.current = observer
  }, [])
  useEffect(
    () => () => {
      viewportObserverRef.current?.disconnect()
      viewportObserverRef.current = null
      viewportNodeRef.current = null
      if (viewportRecheckRef.current) {
        clearTimeout(viewportRecheckRef.current)
        viewportRecheckRef.current = null
      }
    },
    [],
  )

  // 宿主级刷新统一做去抖，避免同一批 storage 写入或多个 invalidate 请求
  // 连续销毁/重建 iframe。刷新只在页面与 Widget 都可见时执行。
  const [refreshGeneration, setRefreshGeneration] = useState(0)
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const requestRefresh = useCallback(() => {
    if (!pageVisible || !inViewport) return
    if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current)
    refreshTimerRef.current = setTimeout(() => {
      refreshTimerRef.current = null
      setRefreshGeneration((generation) => generation + 1)
    }, 500)
  }, [pageVisible, inViewport])
  useEffect(
    () => () => {
      if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current)
    },
    [],
  )

  useEffect(() => {
    if (!tappInstance || !widget) return
    const localWidgetId =
      widget.config.id || widget.id.split('.').pop() || ''
    if (!localWidgetId) return
    return onTappWidgetInvalidate((event) => {
      if (event.tappId !== tappInstance.id) return
      if (event.widgetId !== localWidgetId) return
      requestRefresh()
    })
  }, [tappInstance, widget, requestRefresh])

  // ⚡ 监听尺寸变化，重新加载资源
  useEffect(() => {
    // 尺寸变化使用独立的 widgetId + size 缓存键，无需清空整个 Tapp 缓存。
    if (prevSizeRef.current !== config?.size) {
      prevSizeRef.current = config?.size
      if (widget) {
        setError(null)
        setLoading(true)
        setCode(null)
      }
    }
  }, [config?.size, widget])

  // 加载 Widget 元数据 + 运行时资源（合并路径，避免二次 effect 多等一帧）
  useEffect(() => {
    let cancelled = false

    const loadWidget = async () => {
      setLoading(true)
      setError(null)
      setWidget(null)
      setTappInstance(null)
      setCode(null)
      setIsRunning(false)

      try {
        await runtime.waitForSync()

        const widgets = runtime.getRegisteredWidgets()
        const found = widgets.find((w) => w.id === tappWidgetId)

        if (!found) {
          if (!cancelled) {
            setError(t.tapp.widgetNotFound)
            setLoading(false)
          }
          return
        }

        const tapp = runtime.getTapp(found.tappId)
        if (!tapp) {
          if (!cancelled) {
            setError(t.tapp.appNotExist)
            setLoading(false)
          }
          return
        }

        const running = runtime.isRunning(found.tappId)
        const widgetSize = config?.size || found.config.defaultSize || '4x2'
        widgetPerfMark(
          found.tappId,
          found.config.id,
          'host-load-start',
          widgetSize,
        )

        if (cancelled) return
        setWidget(found)
        setTappInstance(tapp)
        setIsRunning(running)
        setError(null)

        if (!running) {
          setCode(null)
          setLoading(false)
          return
        }

        // 运行中：直接拉取 widget 投影资源（与元数据同一路径，减少状态机往返）
        const resources = await loadWidgetResources(
          tapp,
          widgetSize,
          found.config.id,
        )
        if (cancelled) return
        widgetPerfMark(
          found.tappId,
          found.config.id,
          'resources-ready',
          widgetSize,
        )
        setCode({
          modules: resources.modules,
          moduleResolutions: resources.moduleResolutions,
          coreEntry: resources.coreEntry,
          widgetEntries: resources.widgetEntries,
          widgetHtml: resources.html,
          styles: resources.styles,
          widgetCSS: resources.css,
          i18n: resources.i18n,
        })
        setLoading(false)
      } catch (err) {
        if (!cancelled) {
          setError(userFacingError(err, t.tapp.loadAppFailed))
          setLoading(false)
        }
      }
    }

    loadWidget()

    return () => {
      cancelled = true
    }
  }, [tappWidgetId, runtime])

  // 监听 Tapp 启动/停止/更新 — 触发资源补拉路径（不重复 waitForSync 全量重载）
  useEffect(() => {
    if (!widget) return

    const handleStarted = (data: unknown) => {
      const eventData = data as { id: string }
      if (eventData.id === widget.tappId) {
        setIsRunning(true)
        setError(null)
        setCode(null)
        setLoading(true)
      }
    }

    const handleStopped = (data: unknown) => {
      const eventData = data as { id: string }
      if (eventData.id === widget.tappId) {
        setIsRunning(false)
        setCode(null)
        setLoading(false)
      }
    }

    const handleUpdated = (data: unknown) => {
      const eventData = data as { id: string }
      if (eventData.id === widget.tappId && runtime.isRunning(widget.tappId)) {
        setError(null)
        setCode(null)
        setLoading(true)
      }
    }

    const unsubStart = runtime.on('tapp:started', handleStarted)
    const unsubStop = runtime.on('tapp:stopped', handleStopped)
    const unsubUpdated = runtime.on('tapp:updated', handleUpdated)

    return () => {
      unsubStart()
      unsubStop()
      unsubUpdated()
    }
  }, [widget, runtime])

  // 启动/更新后 loading=true 时补拉资源（与初次挂载路径共用 loadWidgetResources）
  useEffect(() => {
    if (!loading || !isRunning || !widget || !tappInstance || code) return
    let cancelled = false

    const loadCode = async () => {
      try {
        const widgetSize = config?.size || widget.config.defaultSize || '4x2'
        widgetPerfMark(
          tappInstance.id,
          widget.config.id,
          'host-load-start',
          widgetSize,
        )
        const resources = await loadWidgetResources(
          tappInstance,
          widgetSize,
          widget.config.id,
        )
        if (cancelled) return
        widgetPerfMark(
          tappInstance.id,
          widget.config.id,
          'resources-ready',
          widgetSize,
        )
        setCode({
          modules: resources.modules,
          moduleResolutions: resources.moduleResolutions,
          coreEntry: resources.coreEntry,
          widgetEntries: resources.widgetEntries,
          widgetHtml: resources.html,
          styles: resources.styles,
          widgetCSS: resources.css,
          i18n: resources.i18n,
        })
        setError(null)
        setLoading(false)
      } catch (err) {
        if (cancelled) return
        setError(userFacingError(err, t.tapp.appCodeLoadFailed))
        setLoading(false)
      }
    }

    loadCode()
    return () => {
      cancelled = true
    }
  }, [loading, isRunning, widget, tappInstance, config?.size, code])

  // 使用 useMemo 稳定 widgetProps，避免 TappWidgetSandbox 不必要的重渲染
  // scale 和 fontScale 由 TappWidgetSandbox 内部自动计算并注入到 iframe
  // 使用 JSON.stringify 稳定 config.config 的依赖比较
  const configString = JSON.stringify(config.config || {})
  const declaredDefaults = useMemo(
    () =>
      Object.fromEntries(
        (widget?.config.settings || [])
          .filter((setting) => setting.defaultValue !== undefined)
          .map((setting) => [setting.key, setting.defaultValue]),
      ),
    [widget],
  )
  const widgetProps = useMemo(() => {
    const isDark = document.documentElement.classList.contains('dark')
    // 获取主题色
    const primaryColor =
      getComputedStyle(document.documentElement)
        .getPropertyValue('--color-primary')
        .trim() || '#8b5cf6'
    return {
      size: config.size,
      config: { ...declaredDefaults, ...(config.config || {}) },
      isEditMode: isEditMode || false,
      isPreview: isPreview || false,
      theme: (isDark ? 'dark' : 'light') as 'light' | 'dark',
      primaryColor,
      locale,
    }
  }, [
    config.size,
    configString,
    declaredDefaults,
    isEditMode,
    isPreview,
    locale,
  ])

  const handleInstanceSettingsChange = useCallback(
    (patch: Record<string, unknown>): boolean => {
      if (!widget || !onConfigChange) return false
      const declarations = widget.config.settings || []
      const byKey = new Map(
        declarations.map((setting) => [setting.key, setting]),
      )
      const accepted: Record<string, unknown> = {}

      for (const [key, value] of Object.entries(patch)) {
        const setting = byKey.get(key)
        if (!setting) return false
        if (setting.type === 'toggle' && typeof value !== 'boolean')
          return false
        if (
          (setting.type === 'input' || setting.type === 'color') &&
          typeof value !== 'string'
        ) {
          return false
        }
        if (
          setting.type === 'select' &&
          (typeof value !== 'string' ||
            !setting.options?.some((option) => option.value === value))
        ) {
          return false
        }
        if (setting.type === 'number') {
          if (typeof value !== 'number' || !Number.isFinite(value)) return false
          if (setting.min !== undefined && value < setting.min) return false
          if (setting.max !== undefined && value > setting.max) return false
        }
        accepted[key] = value
      }

      onConfigChange({ ...(config.config || {}), ...accepted })
      return true
    },
    [config.config, onConfigChange, widget],
  )

  // interval 是可选策略，并且只在可见、运行中的实例上计时。
  useEffect(() => {
    const policy = widget?.config.refreshPolicy
    if (
      policy?.mode !== 'interval' ||
      !policy.intervalSeconds ||
      !pageVisible ||
      !inViewport ||
      !isRunning
    ) {
      return
    }
    const timer = setInterval(requestRefresh, policy.intervalSeconds * 1000)
    return () => clearInterval(timer)
  }, [widget, pageVisible, inViewport, isRunning, requestRefresh])

  const previousPageVisibleRef = useRef(pageVisible)
  useEffect(() => {
    if (
      pageVisible &&
      !previousPageVisibleRef.current &&
      inViewport &&
      widget?.config.refreshPolicy?.refreshOnVisible !== false
    ) {
      requestRefresh()
    }
    previousPageVisibleRef.current = pageVisible
  }, [pageVisible, inViewport, requestRefresh, widget])

  // 仅站长/临时装所有者可启动；公开 Tapp 未运行时访客不得点开。
  const canControlLifecycle = useMemo(
    () => (tappInstance ? runtime.canControlLifecycle(tappInstance) : false),
    [runtime, tappInstance],
  )

  // 启动 Tapp（仅 canControlLifecycle）
  const handleStartTapp = useCallback(async () => {
    if (!widget || !canControlLifecycle) return
    try {
      await runtime.startTapp(widget.tappId)
    } catch (error) {
      setError(userFacingError(error, t.tapp.startAppFailed))
    }
  }, [widget, runtime, canControlLifecycle, t.tapp.startAppFailed])

  // 所有者挂载小组件时：若自己的装仍是 stopped，自动拉起（不帮访客启动站主已停的 Tapp）。
  const autoStartKeyRef = useRef<string | null>(null)
  useEffect(() => {
    if (isPreview) return
    if (!widget || !tappInstance || isRunning || loading || error) return
    if (!canControlLifecycle) return
    const key = widget.tappId
    if (autoStartKeyRef.current === key) return
    autoStartKeyRef.current = key

    let cancelled = false
    setLoading(true)
    void (async () => {
      try {
        await runtime.startTapp(widget.tappId)
      } catch (err) {
        if (cancelled) return
        autoStartKeyRef.current = null
        setLoading(false)
        console.warn(
          '[TappWidget] auto-start failed:',
          err instanceof Error ? err.message : err,
        )
      }
    })()

    return () => {
      cancelled = true
    }
  }, [
    widget,
    tappInstance,
    isRunning,
    loading,
    error,
    isPreview,
    runtime,
    canControlLifecycle,
  ])

  // 跳转到 Tapp 详情
  const handleGoToTapp = useCallback(() => {
    if (!widget) return
    navigate(tappDetailPath(widget.tappId))
  }, [widget, navigate])

  // 编辑模式下禁用指针事件，允许父级处理拖拽
  const pointerEventsStyle =
    isEditMode || isPreview ? { pointerEvents: 'none' as const } : {}

  // Prefer CSS var — avoid getComputedStyle on loading path.
  const shellThemeColor =
    tappInstance?.manifest?.themeColor?.trim() ||
    'var(--color-primary, #6366f1)'

  // 加载中：整卡骨架（chunk 已过 Suspense；defer=0 接上一段）
  if (loading) {
    return (
      <div
        ref={containerRef}
        className="relative w-full h-full rounded-xl overflow-hidden"
        style={pointerEventsStyle}
      >
        <WidgetSkeletonCover
          active
          preset={TAPP_WIDGET_SKELETON.preset}
          deferMs={0}
          accent={shellThemeColor}
          label={t.common.loading}
        />
      </div>
    )
  }

  // 错误状态
  if (error || !widget || !tappInstance) {
    return (
      <div
        ref={containerRef}
        className="w-full h-full flex items-center justify-center bg-red-50 dark:bg-red-900/20 rounded-xl"
        style={pointerEventsStyle}
      >
        <div className="text-red-500 dark:text-red-400 text-sm text-center px-4">
          {error || t.tapp.widgetNotFound}
        </div>
      </div>
    )
  }

  // Tapp 未运行 - 显示启动提示（使用 Glass 风格）
  if (!isRunning || !code) {
    const themeColor =
      tappInstance.manifest.themeColor?.trim() ||
      'var(--color-primary, #6366f1)'

    const stoppedIconStyle = getTappIconStyle({
      icon: widget.config.icon || tappInstance.manifest.icon,
      iconSvg: tappInstance.manifest.iconSvg,
      themeColor: tappInstance.manifest.themeColor,
      category: tappInstance.manifest.category,
      id: tappInstance.manifest.id,
      permissions: tappInstance.manifest.permissions,
    })
    const stoppedName =
      widget.config.name ||
      resolveManifestText(tappInstance.manifest, locale).name

    return (
      <WidgetShell
        containerRef={containerRef}
        padding={{ x: 16, y: 0 }}
        style={pointerEventsStyle}
        contentClassName="flex flex-col items-center justify-center"
        background={
          <>
            <GlowBackground
              color={themeColor}
              animLevel={anim.level}
              shouldAnimate={false}
              variant="single"
              size="md"
              opacity={0.12}
            />
            {/* 边框效果 */}
            <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />
          </>
        }
      >
        {/* 图标 */}
        <TappIconBadge
          icon={widget.config.icon || tappInstance.manifest.icon}
          iconSvg={tappInstance.manifest.iconSvg}
          name={stoppedName}
          id={tappInstance.manifest.id || tappInstance.id}
          themeColor={tappInstance.manifest.themeColor}
          category={tappInstance.manifest.category}
          permissions={tappInstance.manifest.permissions}
          iconStyle={stoppedIconStyle}
          shellClassName="tapp-page-icon w-12 h-12 mb-3"
          glyphSizeClass="w-7 h-7"
          glyphTextClass="text-2xl"
        />

        {/* 名称 */}
        <div className="text-sm font-bold text-gray-800 dark:text-gray-100 mb-1 text-center">
          {widget.config.name}
        </div>

        {/* 提示 */}
        <div className="text-xs text-gray-500 dark:text-gray-400 mb-4 text-center">
          {canControlLifecycle ? t.tapp.needStartToShow : t.tapp.stopped}
        </div>

        {/* 操作：仅所有者可启动；访客只能看详情，不能把站长已停的 Tapp 拉起来 */}
        {!isEditMode && (
          <div className="flex gap-2 justify-center">
            {canControlLifecycle && (
              <button
                onClick={handleStartTapp}
                className="px-3 py-1.5 text-xs font-medium text-white rounded-lg transition-all shadow-sm hover:shadow-md"
                style={{
                  background: `linear-gradient(135deg, ${themeColor}, color-mix(in srgb, ${themeColor} 80%, black))`,
                }}
              >
                启动
              </button>
            )}
            <button
              onClick={handleGoToTapp}
              className="px-3 py-1.5 text-xs font-medium bg-black/5 dark:bg-white/10 hover:bg-black/10 dark:hover:bg-white/15 text-gray-700 dark:text-gray-200 rounded-lg transition-colors"
            >
              详情
            </button>
          </div>
        )}
      </WidgetShell>
    )
  }

  // Prefer CSS var over getComputedStyle (avoids forced style recalc on render).
  const runningThemeColor =
    tappInstance.manifest.themeColor?.trim() || 'var(--color-primary, #6366f1)'

  return (
    <div
      ref={containerRef}
      className="w-full h-full rounded-xl overflow-hidden"
      style={pointerEventsStyle}
      data-no-ripple
    >
      {/* sandboxHostRef 常驻挂载作为视口观察目标；沙箱本身按 inViewport 挂/卸 */}
      <div ref={sandboxHostRef} className="w-full h-full">
        {inViewport ? (
          <TappWidgetSandbox
            key={refreshGeneration}
            tappInstance={tappInstance}
            code={code}
            widgetId={widget.config.id || widget.id.split('.').pop() || ''}
            widgetProps={widgetProps}
            onError={(err: Error) => {
              setError(userFacingError(err, t.tapp.loadAppFailed))
            }}
            onInstanceSettingsChange={handleInstanceSettingsChange}
            onInvalidate={requestRefresh}
            className="w-full h-full"
          />
        ) : (
          // 屏外 hold：无 bone DOM、无动画、content-visibility（多卡零 shimmer 成本）
          <WidgetSkeleton
            hold
            accent={runningThemeColor}
            label={t.common.loading}
          />
        )}
      </div>
    </div>
  )
}

export const TappWidgetComponent = memo(
  (props: TappWidgetProps) => {
    const anim = useAnimationLevel()

    if (props.isPreview) {
      return (
        <TappWidgetPreview
          tappWidgetId={props.tappWidgetId}
          config={props.config}
          animLevel={anim.level}
        />
      )
    }

    return <TappWidgetRuntime {...props} anim={anim} />
  },
  (prevProps, nextProps) => {
    // 自定义比较函数，优化重渲染
    // 预览也必须比较具体 Tapp、尺寸和实例配置，不能复用另一应用的画面。
    if (prevProps.isPreview && nextProps.isPreview) {
      return (
        prevProps.tappWidgetId === nextProps.tappWidgetId &&
        prevProps.config.type === nextProps.config.type &&
        prevProps.config.size === nextProps.config.size &&
        JSON.stringify(prevProps.config.config) ===
          JSON.stringify(nextProps.config.config)
      )
    }
    // 非预览模式下进行更详细的比较
    return (
      prevProps.tappWidgetId === nextProps.tappWidgetId &&
      prevProps.isEditMode === nextProps.isEditMode &&
      prevProps.isPreview === nextProps.isPreview &&
      prevProps.config.size === nextProps.config.size &&
      prevProps.config.type === nextProps.config.type &&
      JSON.stringify(prevProps.config.config) ===
        JSON.stringify(nextProps.config.config)
    )
  },
)

TappWidgetComponent.displayName = 'TappWidgetComponent'

export default TappWidgetComponent
