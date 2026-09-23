import type { ReactNode } from 'react'
import type { TappWidgetSandboxProps } from '../../tapp/runtime/TappWidgetSandbox'
import type {
  RegisteredWidget,
  TappCodeStructure,
  TappInstance,
} from '../../tapp/types'
import type { WidgetComponentProps } from '../widgetGridTypes'
import {
  lazy,
  memo,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useNavigate } from 'react-router-dom'

import { useI18n, withI18nNamespace } from '../../contexts/I18nContext'
import { isPageVisible, onVisibility } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { TappIconBadge } from '../../tapp/components/TappIconBadge'
import { hiddenWidgetPool } from '../../tapp/runtime/resourceBounds'
import { loadWidgetResources } from '../../tapp/runtime/sandbox/resourceLoader'
import { getTappRuntime } from '../../tapp/runtime/TappRuntime'
import { widgetPerfMark } from '../../tapp/runtime/WidgetLoadPerf'
import { onTappWidgetInvalidate } from '../../tapp/runtime/WidgetRuntimeSignals'
import { resolveManifestText } from '../../tapp/utils/manifestLocale'
import { getTappIconStyle } from '../../tapp/utils/tappColors'
import { tappDetailPath, tappRunPath } from '../../tapp/utils/tappPaths'
import { useTappSubject } from '../../utils/tappSubject'
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

const TappWidgetSandbox = lazy(() =>
  import('../../tapp/runtime/TappWidgetSandbox').then((module) => ({
    default: module.TappWidgetSandbox,
  })),
)

function LazyTappWidgetSandbox({
  fallback,
  ...props
}: TappWidgetSandboxProps & { fallback: ReactNode }) {
  return (
    <Suspense fallback={fallback}>
      <TappWidgetSandbox {...props} />
    </Suspense>
  )
}

export interface TappWidgetProps extends WidgetComponentProps {
  tappWidgetId: string
}

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
      const parts = tappWidgetId.split('.')
      const widgetName = parts.pop() || 'Widget'
      return {
        name: widgetName,
        icon: undefined,
        iconSvg: undefined,
      }
    }

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
    const parts = tappWidgetId.split('.')
    const widgetName = parts.pop() || 'Widget'
    return {
      name: widgetName,
      icon: undefined,
      iconSvg: undefined,
    }
  }
}

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

    const loadedRef = useRef(false)
    const prevSizeRef = useRef(config?.size)
    const prevTappWidgetIdRef = useRef(tappWidgetId)
    const [previewData, setPreviewData] = useState<{
      tappInstance: TappInstance
      code: TappCodeStructure
      widget: RegisteredWidget
    } | null>(null)
    const [previewPhase, setPreviewPhase] = useState<'loading' | 'static'>(
      'loading',
    )

    const previewInfo = useMemo(
      () => getTappWidgetPreviewInfo(tappWidgetId, locale),
      [tappWidgetId, locale],
    )

    useEffect(() => {
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

    useEffect(() => {
      if (loadedRef.current) return
      loadedRef.current = true
      let cancelled = false

      const loadPreviewData = async () => {
        try {
          const runtime = getTappRuntime()

          await runtime.waitForSync()

          const widgets = runtime.getRegisteredWidgets()
          const widget = widgets.find((w) => w.id === tappWidgetId)
          if (!widget) {
            if (!cancelled) setPreviewPhase('static')
            return
          }

          const tapp = runtime.getTapp(widget.tappId)
          if (!tapp) {
            if (!cancelled) setPreviewPhase('static')
            return
          }

          const running = runtime.isRunning(widget.tappId)
          if (!running) {
            if (!cancelled) setPreviewPhase('static')
            return
          }

          const widgetSize = config?.size || widget.config.defaultSize || '4x2'

          try {
            const resources = await loadWidgetResources(
              tapp,
              widgetSize,
              widget.config.id,
            )

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

    if (previewData) {
      return (
        <div
          className="relative w-full h-full rounded-xl overflow-hidden"
          style={{ pointerEvents: 'none' }}
        >
          <LazyTappWidgetSandbox
            tappInstance={previewData.tappInstance}
            code={previewData.code}
            widgetId={
              previewData.widget.config.id ||
              previewData.widget.id.split('.').pop() ||
              ''
            }
            widgetProps={widgetProps}
            className="w-full h-full"
            fallback={null}
          />
          <div className="absolute inset-0 z-50" />
        </div>
      )
    }

    // 用 CSS 变量，渲染期不要 getComputedStyle。
    const themeColor =
      previewInfo?.themeColor?.trim() || 'var(--color-primary, #6366f1)'

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
              <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />
            </>
          }
        >
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

          {!isCompact && (
            <div className="text-center w-full px-2">
              <div
                className={`font-bold text-gray-800 dark:text-gray-100 truncate ${isLarge ? 'text-base mb-1' : 'text-sm'}`}
              >
                {previewInfo?.name || 'Widget'}
              </div>

              {isLarge && previewInfo?.description && (
                <div className="text-xs text-gray-500 dark:text-gray-400 line-clamp-2 leading-relaxed">
                  {previewInfo.description}
                </div>
              )}

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

  const prevSizeRef = useRef(config?.size)

  const [pageVisible, setPageVisible] = useState(isPageVisible())
  useEffect(() => {
    return onVisibility(setPageVisible)
  }, [])

  // 离屏 iframe 原地隐藏并进入有界暂存池。0×0/未撑开不能当离屏；400ms×3 复查。
  const [inViewport, setInViewport] = useState(true)
  const [retained, setRetained] = useState(true)
  const poolKey = useRef({})
  useEffect(() => {
    const key = poolKey.current
    if (inViewport && pageVisible) setRetained(true)
    else if (retained) hiddenWidgetPool.add(key, () => setRetained(false))
    return () => hiddenWidgetPool.remove(key)
  }, [inViewport, pageVisible, retained])
  const viewportObserverRef = useRef<IntersectionObserver | null>(null)
  const viewportNodeRef = useRef<HTMLDivElement | null>(null)
  const viewportRecheckRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const viewportRecheckCountRef = useRef(0)
  // 复查等待中忽略非交叉回调，只有超时后 re-observe 的结果才能判离屏。
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
        if (viewportRecheckPendingRef.current) return
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

  // 宿主刷新去抖，避免一批 storage/invalidate 连续毁建 iframe。只在页与 Widget 都可见时刷新。
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

  useEffect(() => {
    // 尺寸变化走独立缓存键，不必清空整个 Tapp 缓存。
    if (prevSizeRef.current !== config?.size) {
      prevSizeRef.current = config?.size
      if (widget) {
        setError(null)
        setLoading(true)
        setCode(null)
      }
    }
  }, [config?.size, widget])

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

  // interval 可选，只在可见且运行中的实例上计时。
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
      widget?.config.refreshPolicy?.refreshOnVisible === true
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

  const handleStartTapp = useCallback(async () => {
    if (!widget || !canControlLifecycle) return
    try {
      await runtime.startTapp(widget.tappId)
    } catch (error) {
      setError(userFacingError(error, t.tapp.startAppFailed))
    }
  }, [widget, runtime, canControlLifecycle, t.tapp.startAppFailed])

  // 所有者挂载时若自己的装是 stopped 则拉起；不帮访客启动站主已停的 Tapp。
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

  const handleGoToTapp = useCallback(() => {
    if (!widget) return
    navigate(tappDetailPath(widget.tappId))
  }, [widget, navigate])

  const pointerEventsStyle =
    isEditMode || isPreview ? { pointerEvents: 'none' as const } : {}

  // 用 CSS 变量，加载路径不要 getComputedStyle。
  const shellThemeColor =
    tappInstance?.manifest?.themeColor?.trim() ||
    'var(--color-primary, #6366f1)'

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
            <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />
          </>
        }
      >
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

        <div className="text-sm font-bold text-gray-800 dark:text-gray-100 mb-1 text-center">
          {widget.config.name}
        </div>

        <div className="text-xs text-gray-500 dark:text-gray-400 mb-4 text-center">
          {canControlLifecycle ? t.tapp.needStartToShow : t.tapp.stopped}
        </div>

        {/* 访客只能看详情，不能把站长已停的 Tapp 拉起来。 */}
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
                {t.tapp.start}
              </button>
            )}
            <button
              onClick={handleGoToTapp}
              className="px-3 py-1.5 text-xs font-medium bg-black/5 dark:bg-white/10 hover:bg-black/10 dark:hover:bg-white/15 text-gray-700 dark:text-gray-200 rounded-lg transition-colors"
            >
              {t.tapp.details}
            </button>
          </div>
        )}
      </WidgetShell>
    )
  }

  // 用 CSS 变量，避免渲染期强制样式重算。
  const runningThemeColor =
    tappInstance.manifest.themeColor?.trim() || 'var(--color-primary, #6366f1)'

  return (
    <div
      ref={containerRef}
      className="relative group w-full h-full rounded-xl overflow-hidden"
      style={pointerEventsStyle}
      data-no-ripple
    >
      {tappInstance.manifest.page && !isPreview && !isEditMode && (
        <button
          type="button"
          onClick={() => navigate(tappRunPath(tappInstance.manifest.id))}
          aria-label={`${t.tapp.start}: ${resolveManifestText(tappInstance.manifest, locale).name}`}
          title={resolveManifestText(tappInstance.manifest, locale).name}
          className="absolute top-2 right-2 z-10 rounded-lg bg-white/90 dark:bg-black/80 px-2 py-1 text-xs shadow opacity-0 group-hover:opacity-100 focus-visible:opacity-100 [@media(hover:none)]:opacity-100"
        >
          ↗ {t.tapp.start}
        </button>
      )}
      {/* 隐藏时 iframe 原地保留，只有暂存池淘汰才销毁。 */}
      <div ref={sandboxHostRef} className="w-full h-full">
        {((inViewport && pageVisible) || retained) ? (
          <LazyTappWidgetSandbox
            key={refreshGeneration}
            paused={!inViewport}
            style={{ visibility: inViewport ? undefined : 'hidden' }}
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
            fallback={
              <WidgetSkeleton
                hold
                accent={runningThemeColor}
                label={t.common.loading}
              />
            }
          />
        ) : (
          // 屏外 hold：无 bone DOM、无动画、content-visibility。
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
    const subject = useTappSubject()
    const anim = useAnimationLevel()
    if (!subject.ready) return null

    if (props.isPreview) {
      return (
        <TappWidgetPreview
          key={subject.epoch}
          tappWidgetId={props.tappWidgetId}
          config={props.config}
          animLevel={anim.level}
        />
      )
    }

    return <TappWidgetRuntime key={subject.epoch} {...props} anim={anim} />
  },
  (prevProps, nextProps) => {
    // 预览也必须比较具体 Tapp/尺寸/实例配置，不能复用另一应用的画面。
    if (prevProps.isPreview && nextProps.isPreview) {
      return (
        prevProps.tappWidgetId === nextProps.tappWidgetId &&
        prevProps.config.type === nextProps.config.type &&
        prevProps.config.size === nextProps.config.size &&
        JSON.stringify(prevProps.config.config) ===
          JSON.stringify(nextProps.config.config)
      )
    }
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

export default withI18nNamespace(['tapp'], TappWidgetComponent)
