/**
 * Tapp 多窗口管理器
 *
 * 支持在页面中同时运行多个应用窗口
 * 特性：
 * - 最多支持3个应用窗口同时运行
 * - 可自由拖拽窗口位置
 * - 可调整窗口大小
 * - 窗口层级管理（点击置顶）
 */

import type { TappCodeStructure } from '../examples/tapps/types'
import type { TappNotificationOptions } from '../runtime/sandbox/types'
import type { TappInstance } from '../types'
import {
  FaExclamationTriangle,
  FaGripVertical,
  FaPlus,
  FaSave,
  FaSpinner,
  FaTh,
  FaTimes,
  FaTrash,
} from '@lib/icons'
import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
// 统一动画调度器
import { isPageVisible, scheduleIdle, startPage } from '../../hooks/animation/core'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
// CSRF 防护
import { getCSRFToken } from '../../utils/csrf'
// API 配置
import { getUIConfigDeduped } from '../../utils/requestDedup'
import { getTappRuntime } from '../runtime'
import { loadPageResources } from '../runtime/sandbox/resourceLoader'
import { TappPageSandbox } from '../runtime/TappPageSandbox'
import { getTappIconStyle } from '../utils/tappColors'
import { TappIcon } from './TappIcon'

const API_URL = import.meta.env.PUBLIC_API_URL || ''

/** 窗口状态 */
export interface TappWindow {
  /** 唯一窗口ID */
  windowId: string
  /** Tapp ID */
  tappId: string
  /** Tapp 实例 */
  tapp: TappInstance | null
  /** Tapp 代码 */
  code: TappCodeStructure | null
  /** 加载状态 */
  loading: boolean
  /** 错误信息 */
  error: string | null
  /** 窗口位置 */
  position: { x: number, y: number }
  /** 窗口尺寸 */
  size: { width: number, height: number }
  /** 是否最大化 */
  isMaximized: boolean
  /** 层级 */
  zIndex: number
}

/** 窗口管理器 Props */
export interface TappWindowManagerProps {
  /** 初始 Tapp ID */
  initialTappId?: string
  /** 返回回调 */
  onBack?: () => void
  /** 通知回调 */
  onNotification?: (options: TappNotificationOptions) => void
}

/** 最大窗口数量 */
const MAX_WINDOWS = 3

/** 默认窗口尺寸 */
const DEFAULT_WINDOW_SIZE = { width: 600, height: 500 }

/** 窗口方案中的窗口配置 */
interface WindowSchemeItem {
  tappId: string
  position: { x: number, y: number }
  size: { width: number, height: number }
}

/** 保存的窗口方案 */
interface WindowScheme {
  id: string
  name: string
  windows: WindowSchemeItem[]
  createdAt: number
}

/** 最小窗口尺寸 */
const MIN_WINDOW_SIZE = { width: 320, height: 240 }

/** 窗口头部高度 */
const WINDOW_HEADER_HEIGHT = 40

/**
 * 生成唯一窗口ID
 */
function generateWindowId(): string {
  return `window-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
}

/**
 * 计算新窗口的初始位置（级联效果）
 */
function getInitialPosition(windowCount: number): { x: number, y: number } {
  const offset = windowCount * 30
  return {
    x: 100 + offset,
    y: 100 + offset,
  }
}

/**
 * 单个 Tapp 窗口组件
 * 使用 React.memo 优化，避免其他窗口变化时重新渲染
 */
interface TappWindowComponentProps {
  window: TappWindow
  isActive: boolean
  onClose: (windowId: string) => void
  onFocus: (windowId: string) => void
  onMove: (windowId: string, position: { x: number, y: number }) => void
  onResize: (windowId: string, size: { width: number, height: number }) => void
  onNotification?: (options: TappNotificationOptions) => void
  containerBounds: { width: number, height: number }
}

const TappWindowComponent: React.FC<TappWindowComponentProps> = React.memo(({
  window,
  isActive,
  onClose,
  onFocus,
  onMove,
  onResize,
  onNotification,
  containerBounds,
}) => {
  const { t } = useI18n()
  const animConfig = useAnimationLevel()
  const noAnimation = animConfig.level === 'none'

  const windowRef = useRef<HTMLDivElement>(null)
  const [isDragging, setIsDragging] = useState(false)
  const [isResizing, setIsResizing] = useState(false)
  const [resizeDirection, setResizeDirection] = useState<string | null>(null)

  const dragStartRef = useRef({ x: 0, y: 0 })
  const positionStartRef = useRef({ x: 0, y: 0 })
  const sizeStartRef = useRef({ width: 0, height: 0 })

  // 用于追踪交互过程中的实时位置和大小（直接操作 DOM 时使用）
  const currentPositionRef = useRef({ x: window.position.x, y: window.position.y })
  const currentSizeRef = useRef({ width: window.size.width, height: window.size.height })

  // 始终同步 props 到 ref，确保方案保存时能获取最新值
  // 注意：交互过程中 ref 会被直接修改，但交互结束后会同步回 state
  useEffect(() => {
    currentPositionRef.current = { x: window.position.x, y: window.position.y }
    currentSizeRef.current = { width: window.size.width, height: window.size.height }
  }, [window.position.x, window.position.y, window.size.width, window.size.height])

  // 缓存图标样式计算
  const iconStyle = useMemo(() =>
    window.tapp ? getTappIconStyle(window.tapp.manifest) : null, [window.tapp])

  // 拖拽处理 - 支持鼠标和触摸
  const handleDragStart = useCallback((e: React.MouseEvent | React.TouchEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setIsDragging(true)
    // 获取坐标（支持鼠标和触摸）
    const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
    const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY
    dragStartRef.current = { x: clientX, y: clientY }
    // 使用 ref 中的当前值，确保从正确位置开始
    positionStartRef.current = { ...currentPositionRef.current }
    onFocus(window.windowId)
  }, [window.windowId, onFocus])

  // 调整大小处理 - 支持鼠标和触摸
  const handleResizeStart = useCallback((e: React.MouseEvent | React.TouchEvent, direction: string) => {
    e.preventDefault()
    e.stopPropagation()
    setIsResizing(true)
    setResizeDirection(direction)
    // 获取坐标（支持鼠标和触摸）
    const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
    const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY
    dragStartRef.current = { x: clientX, y: clientY }
    // 使用 ref 中的当前值，确保从正确位置和尺寸开始
    positionStartRef.current = { ...currentPositionRef.current }
    sizeStartRef.current = { ...currentSizeRef.current }
    onFocus(window.windowId)
  }, [window.windowId, onFocus])

  // 移动处理 - 使用 requestAnimationFrame 节流优化性能，支持鼠标和触摸
  useEffect(() => {
    if (!isDragging && !isResizing)
      return

    // 页面不可见时不处理拖拽（由调度器可见性状态控制）
    if (!isPageVisible())
      return

    let rafId: number | null = null
    let lastX = dragStartRef.current.x
    let lastY = dragStartRef.current.y

    const handleMove = (e: MouseEvent | TouchEvent) => {
      // 获取坐标（支持鼠标和触摸）
      const clientX = 'touches' in e ? e.touches[0]?.clientX ?? lastX : e.clientX
      const clientY = 'touches' in e ? e.touches[0]?.clientY ?? lastY : e.clientY

      // 避免重复计算相同位置
      if (clientX === lastX && clientY === lastY)
        return
      lastX = clientX
      lastY = clientY

      // 取消上一次未执行的 RAF
      if (rafId)
        cancelAnimationFrame(rafId)

      rafId = requestAnimationFrame(() => {
        if (!windowRef.current)
          return

        const deltaX = clientX - dragStartRef.current.x
        const deltaY = clientY - dragStartRef.current.y

        if (isDragging) {
          // 拖拽移动 - 直接操作 DOM
          let newX = positionStartRef.current.x + deltaX
          let newY = positionStartRef.current.y + deltaY

          // 边界限制
          newX = Math.max(0, Math.min(newX, containerBounds.width - currentSizeRef.current.width))
          newY = Math.max(0, Math.min(newY, containerBounds.height - currentSizeRef.current.height))

          // 使用 transform 进行 GPU 加速定位
          windowRef.current.style.transform = `translate3d(${newX}px, ${newY}px, 0)`
          currentPositionRef.current = { x: newX, y: newY }
        }
        else if (isResizing && resizeDirection) {
          // 调整大小 - 直接操作 DOM
          let newWidth = sizeStartRef.current.width
          let newHeight = sizeStartRef.current.height
          let newX = positionStartRef.current.x
          let newY = positionStartRef.current.y

          if (resizeDirection.includes('e')) {
            newWidth = Math.max(MIN_WINDOW_SIZE.width, sizeStartRef.current.width + deltaX)
          }
          if (resizeDirection.includes('w')) {
            const widthDelta = Math.min(deltaX, sizeStartRef.current.width - MIN_WINDOW_SIZE.width)
            newWidth = sizeStartRef.current.width - widthDelta
            newX = positionStartRef.current.x + widthDelta
          }
          if (resizeDirection.includes('s')) {
            newHeight = Math.max(MIN_WINDOW_SIZE.height, sizeStartRef.current.height + deltaY)
          }
          if (resizeDirection.includes('n')) {
            const heightDelta = Math.min(deltaY, sizeStartRef.current.height - MIN_WINDOW_SIZE.height)
            newHeight = sizeStartRef.current.height - heightDelta
            newY = positionStartRef.current.y + heightDelta
          }

          // 边界限制
          newWidth = Math.min(newWidth, containerBounds.width - newX)
          newHeight = Math.min(newHeight, containerBounds.height - newY)

          // 使用 transform + width/height，transform 用于 GPU 加速位置变换
          windowRef.current.style.transform = `translate3d(${newX}px, ${newY}px, 0)`
          windowRef.current.style.width = `${newWidth}px`
          windowRef.current.style.height = `${newHeight}px`

          currentSizeRef.current = { width: newWidth, height: newHeight }
          currentPositionRef.current = { x: newX, y: newY }
        }
      })
    }

    const handleEnd = () => {
      if (rafId)
        cancelAnimationFrame(rafId)

      // 交互结束时一次性同步状态到 React
      if (isDragging) {
        onMove(window.windowId, currentPositionRef.current)
      }
      else if (isResizing) {
        onResize(window.windowId, currentSizeRef.current)
        if (resizeDirection?.includes('w') || resizeDirection?.includes('n')) {
          onMove(window.windowId, currentPositionRef.current)
        }
      }

      setIsDragging(false)
      setIsResizing(false)
      setResizeDirection(null)
    }

    // 鼠标事件
    document.addEventListener('mousemove', handleMove, { passive: true })
    document.addEventListener('mouseup', handleEnd)
    // 触摸事件 - 使用 passive: true 优化滚动性能
    document.addEventListener('touchmove', handleMove, { passive: true })
    document.addEventListener('touchend', handleEnd)
    document.addEventListener('touchcancel', handleEnd)

    return () => {
      if (rafId)
        cancelAnimationFrame(rafId)
      document.removeEventListener('mousemove', handleMove)
      document.removeEventListener('mouseup', handleEnd)
      document.removeEventListener('touchmove', handleMove)
      document.removeEventListener('touchend', handleEnd)
      document.removeEventListener('touchcancel', handleEnd)
    }
  // 注意：onMove, onResize, window.windowId 通过闭包捕获，不加入依赖以避免不必要的重新绑定
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isDragging, isResizing, resizeDirection, containerBounds])

  // 计算窗口样式 - 使用 transform 进行 GPU 加速
  const windowStyle = useMemo(() => ({
    width: window.size.width,
    height: window.size.height,
    zIndex: window.zIndex,
    // 使用 transform 替代 top/left，启用 GPU 加速
    transform: `translate3d(${window.position.x}px, ${window.position.y}px, 0)`,
    // 只在非交互时启用过渡
    transition: (isDragging || isResizing) ? 'none' : 'box-shadow 0.15s',
  }), [window.position, window.size, window.zIndex, isDragging, isResizing])

  // 交互状态 - 用于显示遮罩层
  const isInteracting = isDragging || isResizing

  // 缓存 boxShadow 样式 - 使用更简单的阴影以提升性能
  const boxShadowStyle = useMemo(() => ({
    boxShadow: isActive
      ? '0 8px 24px rgba(0, 0, 0, 0.2)'
      : '0 4px 12px rgba(0, 0, 0, 0.1)',
    border: '1px solid var(--border-color)',
  }), [isActive])

  // 缓存标题栏样式
  const headerStyle = useMemo(() => ({
    backgroundColor: 'color-mix(in srgb, var(--bg-secondary) 85%, transparent)',
    borderBottom: '1px solid var(--border-color)',
    opacity: isActive ? 1 : 0.7,
    transition: 'opacity 0.2s ease',
  }), [isActive])

  // 缓存窗口点击处理函数
  const handleWindowClick = useCallback(() => {
    onFocus(window.windowId)
  }, [onFocus, window.windowId])

  // 缓存关闭按钮处理函数
  const handleCloseClick = useCallback((e: React.MouseEvent) => {
    e.stopPropagation()
    onClose(window.windowId)
  }, [onClose, window.windowId])

  // 调整大小的手柄 - 使用 useMemo 缓存避免每次渲染创建新数组
  const resizeHandles = useMemo(() => [
    { direction: 'n', className: 'top-0 left-2 right-2 h-1 cursor-n-resize' },
    { direction: 's', className: 'bottom-0 left-2 right-2 h-1 cursor-s-resize' },
    { direction: 'e', className: 'right-0 top-2 bottom-2 w-1 cursor-e-resize' },
    { direction: 'w', className: 'left-0 top-2 bottom-2 w-1 cursor-w-resize' },
    { direction: 'ne', className: 'top-0 right-0 w-3 h-3 cursor-ne-resize' },
    { direction: 'nw', className: 'top-0 left-0 w-3 h-3 cursor-nw-resize' },
    { direction: 'se', className: 'bottom-0 right-0 w-3 h-3 cursor-se-resize' },
    { direction: 'sw', className: 'bottom-0 left-0 w-3 h-3 cursor-sw-resize' },
  ], [])

  return (
    <div
      ref={windowRef}
      className="absolute flex flex-col overflow-hidden rounded-xl"
      style={{
        top: 0,
        left: 0,
        ...windowStyle,
        ...boxShadowStyle,
      }}
      onClick={handleWindowClick}
    >
      {/* 窗口标题栏 - 可拖拽（支持鼠标和触摸） */}
      <div
        className={`flex items-center justify-between px-3 h-10 flex-shrink-0 select-none backdrop-blur-sm ${isDragging ? 'cursor-grabbing' : 'cursor-grab'}`}
        style={headerStyle}
        onMouseDown={handleDragStart}
        onTouchStart={handleDragStart}
      >
        {/* 左侧：拖拽手柄 + 图标 + 名称 */}
        <div className="flex items-center gap-2 min-w-0">
          <FaGripVertical
            className="w-3 h-3 flex-shrink-0"
            style={{ color: 'var(--text-muted)' }}
          />

          {window.loading
            ? (
                <div className="flex items-center gap-2">
                  <div
                    className="w-6 h-6 rounded-lg flex items-center justify-center"
                    style={{ backgroundColor: 'var(--bg-hover)' }}
                  >
                    <FaSpinner className="w-3 h-3 animate-spin" style={{ color: 'var(--text-muted)' }} />
                  </div>
                  <span className="text-xs" style={{ color: 'var(--text-secondary)' }}>{t.tapp.loadingApp}</span>
                </div>
              )
            : window.error
              ? (
                  <div className="flex items-center gap-2">
                    <div className="w-6 h-6 rounded-lg bg-red-100 dark:bg-red-900/30 flex items-center justify-center">
                      <FaExclamationTriangle className="w-3 h-3 text-red-500" />
                    </div>
                    <span className="text-xs text-red-500 truncate">{t.tapp.loadAppFailed}</span>
                  </div>
                )
              : window.tapp && iconStyle
                ? (
                    <div className="flex items-center gap-2 min-w-0">
                      <div
                        className={`w-6 h-6 rounded-lg ${iconStyle.className} flex items-center justify-center text-white text-xs font-bold flex-shrink-0`}
                        style={iconStyle.style}
                      >
                        <TappIcon
                          icon={window.tapp.manifest.icon}
                          iconSvg={window.tapp.manifest.iconSvg}
                          name={window.tapp.manifest.name}
                          sizeClass="w-3 h-3"
                          textSizeClass="text-xs"
                        />
                      </div>
                      <span
                        className="text-xs font-medium truncate"
                        style={{ color: 'var(--text-primary)' }}
                      >
                        {window.tapp.manifest.name}
                      </span>
                      <span
                        className="text-[10px] flex-shrink-0"
                        style={{ color: 'var(--text-muted)' }}
                      >
                        v
                        {window.tapp.manifest.version}
                      </span>
                    </div>
                  )
                : null}
        </div>

        {/* 右侧：关闭按钮 */}
        <div className="flex items-center gap-1 flex-shrink-0">
          <motion.button
            onClick={handleCloseClick}
            className="p-1 text-gray-500 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded transition-colors"
            title={t.common.close}
            whileHover={noAnimation ? undefined : { scale: 1.1 }}
            whileTap={noAnimation ? undefined : { scale: 0.9 }}
          >
            <FaTimes className="w-3 h-3" />
          </motion.button>
        </div>
      </div>

      {/* 窗口内容 */}
      <div
        className="flex-1 overflow-hidden relative"
        style={{ backgroundColor: 'var(--bg-primary)' }}
      >
        {/* 交互时显示遮罩层，防止 iframe 捕获事件并避免重绘 */}
        {isInteracting && (
          <div
            className="absolute inset-0 z-50"
            style={{ backgroundColor: 'transparent' }}
          />
        )}
        {window.loading
          ? (
              <div className="w-full h-full flex items-center justify-center">
                <FaSpinner className="w-8 h-8 text-gray-400 animate-spin" />
              </div>
            )
          : window.error
            ? (
                <div className="w-full h-full flex items-center justify-center">
                  <div className="text-center max-w-xs mx-4">
                    <FaExclamationTriangle className="w-10 h-10 mx-auto text-red-500 mb-3" />
                    <p className="text-sm text-gray-500 dark:text-gray-400">{window.error}</p>
                  </div>
                </div>
              )
            : window.tapp && window.code
              ? (
                  <TappPageSandbox
                    tappInstance={window.tapp}
                    code={window.code}
                    onError={err => console.error('[TappWindow] Error:', err)}
                    onNotification={onNotification}
                  />
                )
              : null}
      </div>

      {/* 调整大小的手柄（支持鼠标和触摸） */}
      {resizeHandles.map(({ direction, className }) => (
        <div
          key={direction}
          className={`absolute ${className} z-10`}
          onMouseDown={e => handleResizeStart(e, direction)}
          onTouchStart={e => handleResizeStart(e, direction)}
          onMouseEnter={(e) => {
            (e.target as HTMLElement).style.backgroundColor = 'color-mix(in srgb, var(--color-primary) 30%, transparent)'
          }}
          onMouseLeave={(e) => {
            (e.target as HTMLElement).style.backgroundColor = 'transparent'
          }}
        />
      ))}
    </div>
  )
}, (prevProps, nextProps) => {
  // 自定义比较函数，只在关键属性变化时重新渲染
  return (
    prevProps.window.windowId === nextProps.window.windowId
    && prevProps.window.position.x === nextProps.window.position.x
    && prevProps.window.position.y === nextProps.window.position.y
    && prevProps.window.size.width === nextProps.window.size.width
    && prevProps.window.size.height === nextProps.window.size.height
    && prevProps.window.zIndex === nextProps.window.zIndex
    && prevProps.window.loading === nextProps.window.loading
    && prevProps.window.error === nextProps.window.error
    && prevProps.window.tapp === nextProps.window.tapp
    && prevProps.window.code === nextProps.window.code
    && prevProps.isActive === nextProps.isActive
    && prevProps.containerBounds.width === nextProps.containerBounds.width
    && prevProps.containerBounds.height === nextProps.containerBounds.height
  )
})

// 设置 displayName 便于调试
TappWindowComponent.displayName = 'TappWindowComponent'

/**
 * Tapp 多窗口管理器
 */
export const TappWindowManager: React.FC<TappWindowManagerProps> = ({
  initialTappId,
  onBack,
  onNotification,
}) => {
  const { t } = useI18n()
  const { isAuthenticated } = useAuth()
  const animConfig = useAnimationLevel()
  const noAnimation = animConfig.level === 'none'
  const runtime = getTappRuntime()

  const containerRef = useRef<HTMLDivElement>(null)
  const schemeMenuRef = useRef<HTMLDivElement>(null)
  const [containerBounds, setContainerBounds] = useState({ width: 0, height: 0 })
  const [windows, setWindows] = useState<TappWindow[]>([])
  const [activeWindowId, setActiveWindowId] = useState<string | null>(null)
  const [nextZIndex, setNextZIndex] = useState(100)
  const [showTappSelector, setShowTappSelector] = useState(false)
  const [availableTapps, setAvailableTapps] = useState<TappInstance[]>([])
  const [showSchemeMenu, setShowSchemeMenu] = useState(false)
  const [savedSchemes, setSavedSchemes] = useState<WindowScheme[]>([])

  // 用于防抖的 ref
  const resizeTimeoutRef = useRef<number | null>(null)
  const [isSaving, setIsSaving] = useState(false)

  // 注册页面到统一调度器（页面级生命周期管理）
  useEffect(() => {
    startPage('tapp-multi')
  }, [])

  // 点击外部关闭方案菜单
  useEffect(() => {
    if (!showSchemeMenu)
      return

    const handleClickOutside = (e: MouseEvent) => {
      if (schemeMenuRef.current && !schemeMenuRef.current.contains(e.target as Node)) {
        setShowSchemeMenu(false)
      }
    }

    // 延迟添加监听器，避免立即触发
    const timer = setTimeout(() => {
      document.addEventListener('mousedown', handleClickOutside)
    }, 0)

    return () => {
      clearTimeout(timer)
      document.removeEventListener('mousedown', handleClickOutside)
    }
  }, [showSchemeMenu])

  // 从云端加载已保存的方案
  useEffect(() => {
    const loadSchemes = async () => {
      try {
        const data = await getUIConfigDeduped()
        if (data.tapp_window_schemes) {
          const schemes = JSON.parse(data.tapp_window_schemes)
          if (Array.isArray(schemes)) {
            setSavedSchemes(schemes)
          }
        }
      }
      catch (e) {
        console.warn('Failed to load window schemes from cloud:', e)
      }
    }
    loadSchemes()
  }, [])

  // 更新容器尺寸 - 使用防抖优化
  useEffect(() => {
    const updateBounds = () => {
      if (containerRef.current) {
        const rect = containerRef.current.getBoundingClientRect()
        setContainerBounds({ width: rect.width, height: rect.height })
      }
    }

    const debouncedUpdateBounds = () => {
      if (resizeTimeoutRef.current) {
        cancelAnimationFrame(resizeTimeoutRef.current)
      }
      resizeTimeoutRef.current = requestAnimationFrame(updateBounds)
    }

    updateBounds() // 初始化时立即执行
    window.addEventListener('resize', debouncedUpdateBounds, { passive: true })
    return () => {
      window.removeEventListener('resize', debouncedUpdateBounds)
      if (resizeTimeoutRef.current) {
        cancelAnimationFrame(resizeTimeoutRef.current)
      }
    }
  }, [])

  // 加载可用的 Tapps - 使用空闲调度
  useEffect(() => {
    // 使用空闲任务调度预加载 Tapp 列表，避免阻塞主线程
    const cancelIdle = scheduleIdle('tapp-multi-load-tapps', async () => {
      await runtime.waitForSync()
      const tapps = runtime.getAllTapps()
      setAvailableTapps(tapps.filter(t => t.manifest.hasPage))
    }, 'normal')

    return cancelIdle
  }, [runtime])

  // 初始化第一个窗口
  useEffect(() => {
    if (initialTappId && windows.length === 0) {
      openTappWindow(initialTappId)
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialTappId])

  // 打开新的 Tapp 窗口
  const openTappWindow = useCallback(async (tappId: string) => {
    if (windows.length >= MAX_WINDOWS) {
      console.warn('Maximum window limit reached')
      return
    }

    const windowId = generateWindowId()
    const position = getInitialPosition(windows.length)

    // 创建初始窗口状态
    const newWindow: TappWindow = {
      windowId,
      tappId,
      tapp: null,
      code: null,
      loading: true,
      error: null,
      position,
      size: { ...DEFAULT_WINDOW_SIZE },
      isMaximized: false,
      zIndex: nextZIndex,
    }

    setWindows(prev => [...prev, newWindow])
    setActiveWindowId(windowId)
    setNextZIndex(prev => prev + 1)
    setShowTappSelector(false)

    // 异步加载 Tapp
    try {
      await runtime.waitForSync()

      const instance = runtime.getTapp(tappId)
      if (!instance) {
        setWindows(prev => prev.map(w =>
          w.windowId === windowId
            ? { ...w, loading: false, error: t.tapp.appNotExist }
            : w,
        ))
        return
      }

      const resources = await loadPageResources(instance)
      const tappCode: TappCodeStructure = {
        core: resources.core,
        page: resources.page,
        pageHtml: resources.html,
        styles: resources.styles,
        pageCSS: resources.css,
      }

      if (!runtime.isRunning(tappId)) {
        await runtime.startTapp(tappId)
      }

      setWindows(prev => prev.map(w =>
        w.windowId === windowId
          ? { ...w, tapp: instance, code: tappCode, loading: false }
          : w,
      ))
    }
    catch (err) {
      setWindows(prev => prev.map(w =>
        w.windowId === windowId
          ? { ...w, loading: false, error: err instanceof Error ? err.message : t.tapp.loadAppFailed }
          : w,
      ))
    }
  }, [windows.length, nextZIndex, runtime, t.tapp.appNotExist, t.tapp.loadAppFailed])

  // 关闭窗口（不触发暂停应用逻辑，应用继续在后台运行）
  const closeWindow = useCallback((windowId: string) => {
    setWindows((prev) => {
      const remaining = prev.filter(w => w.windowId !== windowId)
      // 如果关闭的是活动窗口，激活下一个
      if (activeWindowId === windowId && remaining.length > 0) {
        const topWindow = remaining.reduce((a, b) => a.zIndex > b.zIndex ? a : b)
        setActiveWindowId(topWindow.windowId)
      }
      else if (remaining.length === 0) {
        setActiveWindowId(null)
      }
      return remaining
    })
  }, [activeWindowId])

  // 聚焦窗口
  const focusWindow = useCallback((windowId: string) => {
    setActiveWindowId(windowId)
    setWindows(prev => prev.map(w =>
      w.windowId === windowId
        ? { ...w, zIndex: nextZIndex }
        : w,
    ))
    setNextZIndex(prev => prev + 1)
  }, [nextZIndex])

  // 移动窗口
  const moveWindow = useCallback((windowId: string, position: { x: number, y: number }) => {
    setWindows(prev => prev.map(w =>
      w.windowId === windowId
        ? { ...w, position }
        : w,
    ))
  }, [])

  // 调整窗口大小
  const resizeWindow = useCallback((windowId: string, size: { width: number, height: number }) => {
    setWindows(prev => prev.map(w =>
      w.windowId === windowId
        ? { ...w, size }
        : w,
    ))
  }, [])

  // 保存方案到云端
  const saveToCloud = useCallback(async (schemes: WindowScheme[]) => {
    try {
      // 获取 CSRF Token
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) {
        console.warn('Failed to get CSRF token, skipping cloud save')
        return
      }

      const response = await fetch(`${API_URL}/api/config/tapp-window-schemes`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({
          schemes: JSON.stringify(schemes),
        }),
      })

      if (!response.ok) {
        throw new Error('Failed to save to cloud')
      }
    }
    catch (e) {
      console.warn('Failed to save window schemes to cloud:', e)
    }
  }, [])

  // 保存当前窗口方案
  const saveCurrentScheme = useCallback(async () => {
    if (windows.length === 0 || isSaving)
      return

    setIsSaving(true)

    const schemeWindows: WindowSchemeItem[] = windows
      .filter(w => w.tapp) // 只保存已加载的窗口
      .map(w => ({
        tappId: w.tappId,
        position: { ...w.position },
        size: { ...w.size },
      }))

    if (schemeWindows.length === 0) {
      setIsSaving(false)
      return
    }

    const newScheme: WindowScheme = {
      id: `scheme-${Date.now()}`,
      name: `${t.tapp.schemeNamePrefix} ${savedSchemes.length + 1}`,
      windows: schemeWindows,
      createdAt: Date.now(),
    }

    const updatedSchemes = [...savedSchemes, newScheme]
    setSavedSchemes(updatedSchemes)

    await saveToCloud(updatedSchemes)

    setIsSaving(false)
    setShowSchemeMenu(false)
  }, [windows, savedSchemes, isSaving, saveToCloud, t.tapp.schemeNamePrefix])

  // 加载窗口方案
  const loadScheme = useCallback(async (scheme: WindowScheme) => {
    // 1. 先关闭菜单
    setShowSchemeMenu(false)

    // 2. 清空所有当前窗口并等待状态更新完成
    await new Promise<void>((resolve) => {
      setWindows([])
      setActiveWindowId(null)
      // 使用 requestAnimationFrame 确保 React 状态更新完成
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          resolve()
        })
      })
    })

    // 3. 准备所有新窗口的初始状态
    const newWindows: TappWindow[] = []
    const baseZIndex = nextZIndex

    for (let i = 0; i < scheme.windows.length && i < MAX_WINDOWS; i++) {
      const schemeWindow = scheme.windows[i]
      const windowId = generateWindowId()

      newWindows.push({
        windowId,
        tappId: schemeWindow.tappId,
        tapp: null,
        code: null,
        loading: true,
        error: null,
        position: { ...schemeWindow.position },
        size: { ...schemeWindow.size },
        isMaximized: false,
        zIndex: baseZIndex + i,
      })
    }

    // 4. 一次性设置所有窗口（批量更新，减少重渲染）
    if (newWindows.length > 0) {
      setWindows(newWindows)
      setActiveWindowId(newWindows[newWindows.length - 1].windowId)
      setNextZIndex(baseZIndex + newWindows.length)
    }

    // 5. 异步加载所有 Tapp 的资源
    await runtime.waitForSync()

    for (const newWindow of newWindows) {
      const { windowId, tappId } = newWindow

      try {
        const instance = runtime.getTapp(tappId)
        if (!instance) {
          setWindows(prev => prev.map(w =>
            w.windowId === windowId
              ? { ...w, loading: false, error: t.tapp.appNotExist }
              : w,
          ))
          continue
        }

        const resources = await loadPageResources(instance)
        const tappCode: TappCodeStructure = {
          core: resources.core,
          page: resources.page,
          pageHtml: resources.html,
          styles: resources.styles,
          pageCSS: resources.css,
        }

        if (!runtime.isRunning(tappId)) {
          await runtime.startTapp(tappId)
        }

        setWindows(prev => prev.map(w =>
          w.windowId === windowId
            ? { ...w, tapp: instance, code: tappCode, loading: false }
            : w,
        ))
      }
      catch (err) {
        setWindows(prev => prev.map(w =>
          w.windowId === windowId
            ? { ...w, loading: false, error: err instanceof Error ? err.message : t.tapp.loadAppFailed }
            : w,
        ))
      }
    }
  }, [nextZIndex, runtime, t.tapp.appNotExist, t.tapp.loadAppFailed])

  // 删除方案
  const deleteScheme = useCallback(async (schemeId: string) => {
    const updatedSchemes = savedSchemes.filter(s => s.id !== schemeId)
    setSavedSchemes(updatedSchemes)

    await saveToCloud(updatedSchemes)
  }, [savedSchemes, saveToCloud])

  // 可用于添加的 Tapps（允许打开同一应用的多个实例）
  const selectableTapps = useMemo(() => {
    // 不再排除已打开的应用，允许多实例
    return availableTapps
  }, [availableTapps])

  return (
    <div className="fixed inset-0 overflow-hidden" data-no-ripple>
      {/* 顶部工具栏 - 简化合并 */}
      <div className="absolute top-4 left-4 z-[1000]">
        <div
          className="flex items-center gap-2 rounded-xl px-2 py-1.5 backdrop-blur-md"
          style={{
            backgroundColor: 'color-mix(in srgb, var(--bg-card) 80%, transparent)',
            border: '1px solid var(--border-color)',
            boxShadow: '0 4px 12px rgba(0, 0, 0, 0.1)',
          }}
        >
          {/* 返回按钮 */}
          {onBack && (
            <motion.button
              onClick={onBack}
              className="flex items-center gap-1.5 px-3 py-2 rounded-lg transition-colors"
              style={{ color: 'var(--text-secondary)' }}
              whileHover={{ backgroundColor: 'var(--bg-hover)' }}
              whileTap={noAnimation ? undefined : { scale: 0.95 }}
              title={t.tapp.back}
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
              </svg>
            </motion.button>
          )}

          {/* 方案管理按钮 - 仅登录用户可见，放在返回按钮右边 */}
          {isAuthenticated && (
            <>
              {onBack && (
                <div
                  className="w-px h-6"
                  style={{ backgroundColor: 'var(--border-color)' }}
                />
              )}

              <div className="relative" ref={schemeMenuRef}>
                <motion.button
                  onClick={() => setShowSchemeMenu(!showSchemeMenu)}
                  className="flex items-center gap-2 px-3 py-2 rounded-lg transition-colors"
                  style={{ color: 'var(--text-secondary)' }}
                  whileHover={{ backgroundColor: 'var(--bg-hover)' }}
                  whileTap={noAnimation ? undefined : { scale: 0.9 }}
                  title={t.tapp.windowScheme}
                >
                  <FaTh className="w-4 h-4" />
                  <span className="text-xs font-medium">{t.tapp.scheme}</span>
                </motion.button>

                {/* 方案下拉菜单 */}
                <AnimatePresence>
                  {showSchemeMenu && (
                    <motion.div
                      className="absolute top-full left-0 mt-2 w-56 rounded-xl overflow-hidden z-[1001]"
                      style={{
                        backgroundColor: 'var(--bg-card)',
                        border: '1px solid var(--border-color)',
                        boxShadow: '0 10px 25px -5px rgba(0, 0, 0, 0.15)',
                      }}
                      initial={{ opacity: 0, y: -8, scale: 0.95 }}
                      animate={{ opacity: 1, y: 0, scale: 1 }}
                      exit={{ opacity: 0, y: -8, scale: 0.95 }}
                      transition={{ duration: 0.15 }}
                    >
                      {/* 保存当前方案按钮 */}
                      {windows.length > 0 && (
                        <motion.button
                          onClick={saveCurrentScheme}
                          disabled={isSaving}
                          className="w-full flex items-center gap-3 px-4 py-3 text-sm transition-colors disabled:opacity-50"
                          style={{ color: 'var(--text-primary)' }}
                          whileHover={isSaving ? undefined : { backgroundColor: 'var(--bg-hover)' }}
                        >
                          {isSaving
                            ? (
                                <FaSpinner className="w-4 h-4 animate-spin" style={{ color: 'var(--color-primary)' }} />
                              )
                            : (
                                <FaSave className="w-4 h-4" style={{ color: 'var(--color-primary)' }} />
                              )}
                          <span>{isSaving ? t.tapp.saving : t.tapp.saveCurrentScheme}</span>
                        </motion.button>
                      )}

                      {/* 分隔线 */}
                      {windows.length > 0 && savedSchemes.length > 0 && (
                        <div
                          className="mx-3 my-1 h-px"
                          style={{ backgroundColor: 'var(--border-color)' }}
                        />
                      )}

                      {/* 已保存的方案列表 */}
                      {savedSchemes.length > 0
                        ? (
                            <div className="max-h-48 overflow-y-auto py-1">
                              {savedSchemes.map(scheme => (
                                <div
                                  key={scheme.id}
                                  className="flex items-center justify-between px-4 py-2.5 group transition-colors hover:bg-[var(--bg-hover)]"
                                >
                                  <motion.button
                                    onClick={() => loadScheme(scheme)}
                                    className="flex-1 text-left text-sm truncate"
                                    style={{ color: 'var(--text-primary)' }}
                                    whileTap={{ scale: 0.98 }}
                                  >
                                    <span className="block truncate">{scheme.name}</span>
                                    <span
                                      className="text-xs"
                                      style={{ color: 'var(--text-muted)' }}
                                    >
                                      {t.tapp.windowCount.replace('{count}', String(scheme.windows.length))}
                                    </span>
                                  </motion.button>
                                  <motion.button
                                    onClick={(e: React.MouseEvent) => {
                                      e.stopPropagation()
                                      deleteScheme(scheme.id)
                                    }}
                                    className="p-1.5 opacity-0 group-hover:opacity-100 transition-opacity rounded"
                                    style={{ color: 'var(--text-muted)' }}
                                    whileHover={{ color: 'var(--color-error, #ef4444)' }}
                                    whileTap={{ scale: 0.9 }}
                                    title={t.tapp.deleteScheme}
                                  >
                                    <FaTrash className="w-3.5 h-3.5" />
                                  </motion.button>
                                </div>
                              ))}
                            </div>
                          )
                        : windows.length === 0
                          ? (
                              <div
                                className="px-4 py-5 text-center text-sm"
                                style={{ color: 'var(--text-muted)' }}
                              >
                                {t.tapp.noSavedSchemes}
                              </div>
                            )
                          : null}
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </>
          )}

          {/* 分隔线 */}
          <div
            className="w-px h-6"
            style={{ backgroundColor: 'var(--border-color)' }}
          />

          {/* 窗口计数 + 添加按钮 */}
          <div className="flex items-center gap-2">
            <span
              className="px-2 py-1 text-sm font-medium"
              style={{ color: 'var(--text-muted)' }}
            >
              {windows.length}
              /
              {MAX_WINDOWS}
            </span>

            {windows.length < MAX_WINDOWS && (
              <motion.button
                onClick={() => setShowTappSelector(true)}
                className="flex items-center justify-center w-8 h-8 rounded-lg transition-colors"
                style={{ color: 'var(--color-primary)' }}
                whileHover={{ backgroundColor: 'color-mix(in srgb, var(--color-primary) 15%, transparent)' }}
                whileTap={noAnimation ? undefined : { scale: 0.9 }}
                title={t.tapp.addWindow}
              >
                <FaPlus className="w-4 h-4" />
              </motion.button>
            )}
          </div>
        </div>
      </div>

      {/* 窗口容器 */}
      <div
        ref={containerRef}
        className="absolute inset-0 overflow-hidden"
      >
        <AnimatePresence>
          {windows.map(window => (
            <TappWindowComponent
              key={window.windowId}
              window={window}
              isActive={activeWindowId === window.windowId}
              onClose={closeWindow}
              onFocus={focusWindow}
              onMove={moveWindow}
              onResize={resizeWindow}
              onNotification={onNotification}
              containerBounds={containerBounds}
            />
          ))}
        </AnimatePresence>

        {/* 空状态提示 */}
        {windows.length === 0 && (
          <motion.div
            className="absolute inset-0 flex items-center justify-center"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
          >
            <div className="text-center">
              <div className="w-20 h-20 mx-auto mb-4 rounded-2xl bg-gray-200 dark:bg-neutral-700 flex items-center justify-center">
                <FaPlus className="w-8 h-8 text-gray-400 dark:text-gray-500" />
              </div>
              <h3 className="text-lg font-medium text-gray-700 dark:text-gray-200 mb-2">
                {t.tapp.noOpenWindows}
              </h3>
              <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
                {t.tapp.clickToAddWindow}
              </p>
              <motion.button
                onClick={() => setShowTappSelector(true)}
                className="px-6 py-3 bg-indigo-600 hover:bg-indigo-700 text-white rounded-xl font-medium transition-colors"
                whileHover={noAnimation ? undefined : { scale: 1.02 }}
                whileTap={noAnimation ? undefined : { scale: 0.98 }}
              >
                {t.tapp.openFirstApp}
              </motion.button>
            </div>
          </motion.div>
        )}
      </div>

      {/* Tapp 选择器弹窗 */}
      <AnimatePresence>
        {showTappSelector && (
          <>
            {/* 背景遮罩 */}
            <motion.div
              className="fixed inset-0 z-[2000]"
              style={{ backgroundColor: 'color-mix(in srgb, var(--bg-primary) 60%, transparent)' }}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              onClick={() => setShowTappSelector(false)}
            />

            {/* 选择器面板 - 使用 flex 居中 */}
            <div className="fixed inset-0 z-[2001] flex items-center justify-center pointer-events-none">
              <motion.div
                className="w-full max-w-md max-h-[70vh] overflow-hidden rounded-2xl pointer-events-auto mx-4"
                style={{
                  backgroundColor: 'var(--bg-card)',
                  border: '1px solid var(--border-color)',
                  boxShadow: '0 25px 50px -12px rgba(0, 0, 0, 0.25)',
                }}
                initial={{ opacity: 0, scale: 0.9, y: 20 }}
                animate={{ opacity: 1, scale: 1, y: 0 }}
                exit={{ opacity: 0, scale: 0.9, y: 20 }}
                transition={{ type: 'spring', stiffness: 300, damping: 25 }}
              >
                {/* 头部 */}
                <div
                  className="px-5 py-4 flex items-center justify-between"
                  style={{ borderBottom: '1px solid var(--border-color)' }}
                >
                  <h3
                    className="text-lg font-semibold"
                    style={{ color: 'var(--text-primary)' }}
                  >
                    {t.tapp.selectApp}
                  </h3>
                  <motion.button
                    onClick={() => setShowTappSelector(false)}
                    className="p-1.5 rounded-lg transition-colors"
                    style={{ color: 'var(--text-muted)' }}
                    whileHover={{ backgroundColor: 'var(--bg-hover)', color: 'var(--text-primary)' }}
                    whileTap={{ scale: 0.9 }}
                  >
                    <FaTimes className="w-4 h-4" />
                  </motion.button>
                </div>

                {/* 应用列表 - 网格布局 */}
                <div className="p-4 overflow-y-auto max-h-[calc(70vh-80px)]">
                  {selectableTapps.length === 0
                    ? (
                        <div className="text-center py-8">
                          <p style={{ color: 'var(--text-muted)' }}>
                            {t.tapp.noAvailableApps}
                          </p>
                        </div>
                      )
                    : (
                        <div className="grid grid-cols-4 gap-3">
                          {selectableTapps.map((tapp) => {
                            const style = getTappIconStyle(tapp.manifest)
                            return (
                              <motion.button
                                key={tapp.id}
                                onClick={() => openTappWindow(tapp.id)}
                                className="flex flex-col items-center gap-2 p-3 rounded-xl transition-colors"
                                whileHover={{ backgroundColor: 'var(--bg-hover)' }}
                                whileTap={{ scale: 0.95 }}
                                title={tapp.manifest.description}
                              >
                                {style && (
                                  <div
                                    className={`w-12 h-12 rounded-xl ${style.className} flex items-center justify-center text-white font-bold`}
                                    style={style.style}
                                  >
                                    <TappIcon
                                      icon={tapp.manifest.icon}
                                      iconSvg={tapp.manifest.iconSvg}
                                      name={tapp.manifest.name}
                                      sizeClass="w-6 h-6"
                                      textSizeClass="text-lg"
                                    />
                                  </div>
                                )}
                                <span
                                  className="text-xs text-center w-full truncate"
                                  style={{ color: 'var(--text-primary)' }}
                                >
                                  {tapp.manifest.name}
                                </span>
                              </motion.button>
                            )
                          })}
                        </div>
                      )}
                </div>
              </motion.div>
            </div>
          </>
        )}
      </AnimatePresence>
    </div>
  )
}

export default TappWindowManager
