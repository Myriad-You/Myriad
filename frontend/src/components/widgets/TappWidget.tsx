/**
 * Tapp Widget 组件
 * 用于在 Dashboard 中渲染 Tapp 提供的小组件
 *
 * 使用 TappSandbox 实现，支持完整的 Tapp SDK API（storage, AI, notifications 等）
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

import type { TappCodeStructure } from '../../tapp/examples/tapps/types'
import type { RegisteredWidget, TappInstance } from '../../tapp/types'
import type { WidgetComponentProps } from '../WidgetGrid'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { isPageVisible, onVisibility } from '../../hooks/animation/core'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { TappIcon } from '../../tapp/components/TappIcon'
import { getTappRuntime, TappWidgetSandbox } from '../../tapp/runtime'
import { getResourceLoader, loadWidgetResources } from '../../tapp/runtime/sandbox/resourceLoader'
import { GlowBackground } from './shared/GlowBackground'

export interface TappWidgetProps extends WidgetComponentProps {
  /** Tapp Widget 完整 ID (tapp.{tappId}.{widgetId}) */
  tappWidgetId: string
}

/**
 * 获取 Tapp Widget 的预览信息
 * 同步方法，从 runtime 缓存中获取信息
 */
function getTappWidgetPreviewInfo(tappWidgetId: string): {
  name: string
  icon?: string
  iconSvg?: string
  themeColor?: string
  description?: string
  tappName?: string
} | null {
  try {
    const runtime = getTappRuntime()
    const widgets = runtime.getRegisteredWidgets()
    const widget = widgets.find(w => w.id === tappWidgetId)

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

    // 获取 Tapp 实例以获取主题色
    const tapp = runtime.getTapp(widget.tappId)

    return {
      name: widget.config.name || 'Widget',
      icon: widget.config.icon || tapp?.manifest.icon,
      iconSvg: tapp?.manifest.iconSvg,
      themeColor: tapp?.manifest.themeColor,
      description: widget.config.description,
      tappName: tapp?.manifest.name,
    }
  }
  catch {
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
const TappWidgetPreview = memo(({
  tappWidgetId,
  config,
  animLevel,
}: {
  tappWidgetId: string
  config: WidgetComponentProps['config']
  animLevel: 'none' | 'light' | 'standard'
}) => {
  const { locale } = useI18n()

  // 使用 ref 确保只加载一次（但尺寸变化时需要重新加载）
  const loadedRef = useRef(false)
  const prevSizeRef = useRef(config?.size)
  const [previewData, setPreviewData] = useState<{
    tappInstance: TappInstance
    code: TappCodeStructure
    widget: RegisteredWidget
  } | null>(null)
  const [fallback, setFallback] = useState(false)

  // 获取预览信息（用于回退显示）
  const previewInfo = useMemo(() => getTappWidgetPreviewInfo(tappWidgetId), [tappWidgetId])

  // ⚡ 优化：监听尺寸变化，重新加载资源
  useEffect(() => {
    // 尺寸变化时重置加载状态，触发重新加载
    if (prevSizeRef.current !== config?.size) {
      prevSizeRef.current = config?.size
      loadedRef.current = false
      setPreviewData(null)
      setFallback(false)
    }
  }, [config?.size])

  // 加载预览数据（尺寸变化时会重新触发）
  useEffect(() => {
    if (loadedRef.current)
      return
    loadedRef.current = true

    const loadPreviewData = async () => {
      try {
        const runtime = getTappRuntime()

        // 等待 runtime 同步
        await runtime.waitForSync()

        // 查找 widget
        const widgets = runtime.getRegisteredWidgets()
        const widget = widgets.find(w => w.id === tappWidgetId)
        if (!widget) {
          setFallback(true)
          return
        }

        // 获取 Tapp 实例
        const tapp = runtime.getTapp(widget.tappId)
        if (!tapp) {
          setFallback(true)
          return
        }

        // 检查 Tapp 是否运行中
        const running = runtime.isRunning(widget.tappId)
        if (!running) {
          setFallback(true)
          return
        }

        // 🎯 使用新的资源加载器获取 Widget 专用资源
        // ⚡ 优化：使用当前尺寸加载对应的资源
        const widgetSize = config?.size || widget.config.defaultSize || '4x2'

        // 清除缓存确保获取最新的尺寸资源
        getResourceLoader().clearCache(widget.tappId)

        try {
          const resources = await loadWidgetResources(tapp, widgetSize)

          // 转换为 TappCodeStructure 格式以兼容 TappWidgetSandbox
          const tappCode: TappCodeStructure = {
            core: resources.core,
            widget: resources.widget,
            widgetHtml: resources.html,
            styles: resources.styles,
            widgetCSS: resources.css,
          }

          setPreviewData({ tappInstance: tapp, code: tappCode, widget })
        }
        catch {
          setFallback(true)
        }
      }
      catch {
        setFallback(true)
      }
    }

    loadPreviewData()
  }, [tappWidgetId, config?.size])

  // 构造 widgetProps - 只计算一次
  const widgetProps = useMemo(() => {
    const isDark = document.documentElement.classList.contains('dark')
    const primaryColor = getComputedStyle(document.documentElement)
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
          widgetId={previewData.widget.config.id || previewData.widget.id.split('.').pop() || ''}
          widgetProps={widgetProps}
          className="w-full h-full"
        />
        {/* 透明覆盖层 - 确保完全阻止交互 */}
        <div className="absolute inset-0 z-50" />
      </div>
    )
  }

  // 回退：显示静态预览
  // 获取主题色
  const themeColor = previewInfo?.themeColor
    || getComputedStyle(document.documentElement).getPropertyValue('--color-primary').trim() || '#8b5cf6'

  // 根据尺寸判断布局
  const isCompact = config.size === '1x1' || config.size === '2x1'
  const isLarge = config.size === '4x2' || config.size === '4x4' || config.size === '2x4'

  // 图标样式
  const iconBgStyle = previewInfo?.themeColor
    ? { background: `linear-gradient(to bottom right, ${previewInfo.themeColor}, ${previewInfo.themeColor}99)` }
    : undefined
  const iconBgClass = previewInfo?.themeColor
    ? 'bg-gradient-to-br'
    : 'bg-gradient-to-br from-indigo-500 to-purple-600'

  return (
    <div
      className="relative w-full h-full rounded-xl overflow-hidden glass"
      style={{ pointerEvents: 'none' }}
    >
      {/* 背景渐变 */}
      <div
        className="absolute inset-0 opacity-[0.03]"
        style={{ background: `linear-gradient(135deg, ${themeColor}, transparent 60%)` }}
      />

      {/* 光晕背景 */}
      <GlowBackground
        color={themeColor}
        animLevel={animLevel}
        shouldAnimate={false}
        variant="single"
        size={isLarge ? 'lg' : 'md'}
        opacity={0.15}
      />

      {/* 主内容 */}
      <div className={`relative z-10 h-full flex ${isCompact ? 'items-center justify-center' : 'flex-col justify-center items-center'} p-3`}>
        {/* 图标 */}
        <div
          className={`${iconBgClass} flex items-center justify-center text-white shadow-lg relative overflow-hidden flex-shrink-0 ${
            isCompact ? 'w-8 h-8 rounded-lg' : isLarge ? 'w-14 h-14 rounded-xl mb-3' : 'w-10 h-10 rounded-xl mb-2'
          }`}
          style={iconBgStyle}
        >
          <div className="absolute inset-0 bg-gradient-to-br from-white/25 to-transparent" />
          <TappIcon
            icon={previewInfo?.icon}
            iconSvg={previewInfo?.iconSvg}
            name={previewInfo?.name || 'Widget'}
            sizeClass={isCompact ? 'w-5 h-5' : isLarge ? 'w-8 h-8' : 'w-6 h-6'}
            textSizeClass={isCompact ? 'text-lg' : isLarge ? 'text-2xl' : 'text-xl'}
            className="relative z-10"
          />
        </div>

        {/* 文本信息 - 紧凑模式不显示 */}
        {!isCompact && (
          <div className="text-center w-full px-2">
            <div className={`font-bold text-gray-800 dark:text-gray-100 truncate ${isLarge ? 'text-base mb-1' : 'text-sm'}`}>
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
      </div>

      {/* 边框效果 */}
      <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />
    </div>
  )
})

TappWidgetPreview.displayName = 'TappWidgetPreview'

/**
 * Tapp Widget 组件（使用 TappSandbox 实现完整 API 支持）
 */
export const TappWidgetComponent = memo(({
  config,
  isEditMode,
  isPreview,
  tappWidgetId,
}: TappWidgetProps) => {
  const anim = useAnimationLevel()

  // 🎯 预览模式优化：渲染美观的 Glass 风格预览卡片
  // 预览用于小组件库中显示，使用与普通小组件一致的视觉风格
  if (isPreview) {
    return (
      <TappWidgetPreview
        tappWidgetId={tappWidgetId}
        config={config}
        animLevel={anim.level}
      />
    )
  }

  const runtime = getTappRuntime()
  const containerRef = useRef<HTMLDivElement>(null)
  const navigate = useNavigate()
  const { locale } = useI18n()
  const [widget, setWidget] = useState<RegisteredWidget | null>(null)
  const [tappInstance, setTappInstance] = useState<TappInstance | null>(null)
  const [code, setCode] = useState<TappCodeStructure | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [isRunning, setIsRunning] = useState(false)

  // 记录初始化完成状态，避免重复初始化
  const initializedRef = useRef(false)
  const tappWidgetIdRef = useRef(tappWidgetId)
  tappWidgetIdRef.current = tappWidgetId

  // ⚡ 记录上一次的尺寸，用于检测尺寸变化
  const prevSizeRef = useRef(config?.size)

  // 🎯 集成动画调度器的页面可见性感知
  // 页面不可见时跳过非必要的状态更新，减少后台 CPU 开销
  const pageVisibleRef = useRef(isPageVisible())
  useEffect(() => {
    return onVisibility((visible) => {
      pageVisibleRef.current = visible
    })
  }, [])

  // ⚡ 监听尺寸变化，重新加载资源
  useEffect(() => {
    // 尺寸变化时重置初始化状态，清除缓存，触发重新加载
    if (prevSizeRef.current !== config?.size && widget) {
      prevSizeRef.current = config?.size
      initializedRef.current = false

      // 清除资源加载器缓存
      getResourceLoader().clearCache(widget.tappId)

      // 触发重新加载
      setLoading(true)
      setCode(null)
    }
  }, [config?.size, widget])

  // 加载 Widget 信息和代码
  useEffect(() => {
    let cancelled = false

    const loadWidget = async () => {
      // 防止重复初始化
      if (initializedRef.current && tappWidgetIdRef.current === tappWidgetId) {
        return
      }

      setLoading(true)
      setError(null)

      try {
        // 等待 runtime 同步
        await runtime.waitForSync()

        const widgets = runtime.getRegisteredWidgets()
        const found = widgets.find(w => w.id === tappWidgetId)

        if (!found) {
          if (!cancelled) {
            setError('Widget not found')
            setLoading(false)
          }
          return
        }

        // 获取 Tapp 实例
        const tapp = runtime.getTapp(found.tappId)
        if (!tapp) {
          if (!cancelled) {
            setError('Tapp not found')
            setLoading(false)
          }
          return
        }

        // 检查 Tapp 是否运行中
        const running = runtime.isRunning(found.tappId)

        if (!cancelled) {
          setWidget(found)
          setTappInstance(tapp)
          setIsRunning(running)

          // 只有运行中才获取代码
          if (running) {
            // 🎯 使用新的资源加载器获取 Widget 专用资源
            const widgetSize = config?.size || found.config.defaultSize || '4x2'
            try {
              const resources = await loadWidgetResources(tapp, widgetSize)

              // 转换为 TappCodeStructure 格式以兼容 TappWidgetSandbox
              const tappCode: TappCodeStructure = {
                core: resources.core,
                widget: resources.widget,
                widgetHtml: resources.html,
                styles: resources.styles,
                widgetCSS: resources.css,
              }

              setCode(tappCode)
            }
            catch {
              setError('Tapp code not found')
              setLoading(false)
              return
            }
          }

          setError(null)
          setLoading(false)
          initializedRef.current = true
        }
      }
      catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load widget')
          setLoading(false)
        }
      }
    }

    loadWidget()

    return () => {
      cancelled = true
    }
  }, [tappWidgetId, runtime])

  // 监听 Tapp 启动/停止事件
  useEffect(() => {
    if (!widget)
      return

    const handleStarted = (data: unknown) => {
      const eventData = data as { id: string }
      if (eventData.id === widget.tappId) {
        initializedRef.current = false
        setIsRunning(true)
        // 重新加载
        setLoading(true)
      }
    }

    const handleStopped = (data: unknown) => {
      const eventData = data as { id: string }
      if (eventData.id === widget.tappId) {
        setIsRunning(false)
        setCode(null)
      }
    }

    const unsubStart = runtime.on('tapp:started', handleStarted)
    const unsubStop = runtime.on('tapp:stopped', handleStopped)

    return () => {
      unsubStart()
      unsubStop()
    }
  }, [widget, runtime])

  // 重新加载时获取代码
  useEffect(() => {
    if (!loading || !isRunning || !widget || !tappInstance)
      return

    const loadCode = async () => {
      try {
        // 🎯 使用新的资源加载器获取 Widget 专用资源
        const widgetSize = config?.size || widget.config.defaultSize || '4x2'

        // 清除资源加载器缓存以确保获取最新资源
        getResourceLoader().clearCache(widget.tappId)

        const resources = await loadWidgetResources(tappInstance, widgetSize)

        // 转换为 TappCodeStructure 格式
        const tappCode: TappCodeStructure = {
          core: resources.core,
          widget: resources.widget,
          widgetHtml: resources.html,
          styles: resources.styles,
          widgetCSS: resources.css,
        }

        setCode(tappCode)
        setLoading(false)
        initializedRef.current = true
      }
      catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load code')
        setLoading(false)
      }
    }

    loadCode()
  }, [loading, isRunning, widget, tappInstance, config?.size])

  // 使用 useMemo 稳定 widgetProps，避免 TappWidgetSandbox 不必要的重渲染
  // scale 和 fontScale 由 TappWidgetSandbox 内部自动计算并注入到 iframe
  // 使用 JSON.stringify 稳定 config.config 的依赖比较
  const configString = JSON.stringify(config.config || {})
  const widgetProps = useMemo(() => {
    const isDark = document.documentElement.classList.contains('dark')
    // 获取主题色
    const primaryColor = getComputedStyle(document.documentElement)
      .getPropertyValue('--color-primary')
      .trim() || '#8b5cf6'
    return {
      size: config.size,
      config: config.config || {},
      isEditMode: isEditMode || false,
      isPreview: isPreview || false,
      theme: (isDark ? 'dark' : 'light') as 'light' | 'dark',
      primaryColor,
      locale,
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [config.size, configString, isEditMode, isPreview, locale])

  // 启动 Tapp
  const handleStartTapp = useCallback(async () => {
    if (!widget)
      return
    try {
      await runtime.startTapp(widget.tappId)
    }
    catch {
      // Tapp 启动失败，静默处理
    }
  }, [widget, runtime])

  // 跳转到 Tapp 详情
  const handleGoToTapp = useCallback(() => {
    if (!widget)
      return
    navigate(`/tapp/detail/${widget.tappId}`)
  }, [widget, navigate])

  // 编辑模式下禁用指针事件，允许父级处理拖拽
  const pointerEventsStyle = (isEditMode || isPreview) ? { pointerEvents: 'none' as const } : {}

  // 加载中
  if (loading) {
    return (
      <div
        ref={containerRef}
        className="w-full h-full flex items-center justify-center bg-white/50 dark:bg-neutral-900/50 rounded-xl"
        style={pointerEventsStyle}
      >
        <div className="animate-pulse text-gray-400 dark:text-gray-500">
          Loading...
        </div>
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
          {error || 'Widget not available'}
        </div>
      </div>
    )
  }

  // Tapp 未运行 - 显示启动提示（使用 Glass 风格）
  if (!isRunning || !code) {
    // 获取主题色
    const themeColor = tappInstance.manifest.themeColor
      || getComputedStyle(document.documentElement).getPropertyValue('--color-primary').trim() || '#8b5cf6'

    // 图标样式
    const iconBgStyle = tappInstance.manifest.themeColor
      ? { background: `linear-gradient(to bottom right, ${tappInstance.manifest.themeColor}, ${tappInstance.manifest.themeColor}99)` }
      : undefined
    const iconBgClass = tappInstance.manifest.themeColor
      ? 'bg-gradient-to-br'
      : 'bg-gradient-to-br from-indigo-500 to-purple-600'

    return (
      <div
        ref={containerRef}
        className="relative w-full h-full rounded-xl overflow-hidden glass"
        style={pointerEventsStyle}
      >
        {/* 背景渐变 */}
        <div
          className="absolute inset-0 opacity-[0.05]"
          style={{ background: `linear-gradient(135deg, ${themeColor}, transparent 60%)` }}
        />

        {/* 光晕背景 */}
        <GlowBackground
          color={themeColor}
          animLevel={anim.level}
          shouldAnimate={false}
          variant="single"
          size="md"
          opacity={0.12}
        />

        {/* 主内容 */}
        <div className="relative z-10 h-full flex flex-col items-center justify-center px-4">
          {/* 图标 */}
          <div
            className={`w-12 h-12 ${iconBgClass} rounded-xl flex items-center justify-center text-white shadow-lg relative overflow-hidden mb-3`}
            style={iconBgStyle}
          >
            <div className="absolute inset-0 bg-gradient-to-br from-white/25 to-transparent" />
            <TappIcon
              icon={widget.config.icon || tappInstance.manifest.icon}
              iconSvg={tappInstance.manifest.iconSvg}
              name={widget.config.name || tappInstance.manifest.name}
              sizeClass="w-7 h-7"
              textSizeClass="text-2xl"
              className="relative z-10"
            />
          </div>

          {/* 名称 */}
          <div className="text-sm font-bold text-gray-800 dark:text-gray-100 mb-1 text-center">
            {widget.config.name}
          </div>

          {/* 提示 */}
          <div className="text-xs text-gray-500 dark:text-gray-400 mb-4 text-center">
            需要启动 Tapp 以显示
          </div>

          {/* 操作按钮 */}
          {!isEditMode && (
            <div className="flex gap-2 justify-center">
              <button
                onClick={handleStartTapp}
                className="px-3 py-1.5 text-xs font-medium text-white rounded-lg transition-all shadow-sm hover:shadow-md"
                style={{ background: `linear-gradient(135deg, ${themeColor}, color-mix(in srgb, ${themeColor} 80%, black))` }}
              >
                启动
              </button>
              <button
                onClick={handleGoToTapp}
                className="px-3 py-1.5 text-xs font-medium bg-black/5 dark:bg-white/10 hover:bg-black/10 dark:hover:bg-white/15 text-gray-700 dark:text-gray-200 rounded-lg transition-colors"
              >
                详情
              </button>
            </div>
          )}
        </div>

        {/* 边框效果 */}
        <div className="absolute inset-0 rounded-xl ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none" />
      </div>
    )
  }

  return (
    <div
      ref={containerRef}
      className="w-full h-full rounded-xl overflow-hidden"
      style={pointerEventsStyle}
      data-no-ripple
    >
      <TappWidgetSandbox
        tappInstance={tappInstance}
        code={code}
        widgetId={widget.config.id || widget.id.split('.').pop() || ''}
        widgetProps={widgetProps}
        onError={(err: Error) => {
          setError(err.message)
        }}
        className="w-full h-full"
      />
    </div>
  )
}, (prevProps, nextProps) => {
  // 自定义比较函数，优化重渲染
  // 预览模式下只比较 type 和 isPreview
  if (prevProps.isPreview && nextProps.isPreview) {
    return prevProps.config.type === nextProps.config.type
  }
  // 非预览模式下进行更详细的比较
  return (
    prevProps.tappWidgetId === nextProps.tappWidgetId
    && prevProps.isEditMode === nextProps.isEditMode
    && prevProps.isPreview === nextProps.isPreview
    && prevProps.config.size === nextProps.config.size
    && prevProps.config.type === nextProps.config.type
    && JSON.stringify(prevProps.config.config) === JSON.stringify(nextProps.config.config)
  )
})

TappWidgetComponent.displayName = 'TappWidgetComponent'

export default TappWidgetComponent
