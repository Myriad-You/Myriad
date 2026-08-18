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

import type { TappCategory, TappCodeStructure, TappInstance } from '../types'
import {
  FaExclamationTriangle,
  FaGripVertical,
  FaSave,
  FaTh,
  FaTimes,
  FaTrash,
  LuMinus,
  LuSearch,
  LuX,
} from '@lib/icons'

import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Spinner } from '../../components/Spinner'
// API 配置
import { API_URL as CONFIG_API_URL } from '../../config'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
// 统一动画调度器
import { isPageVisible, startPage } from '../../hooks/animation'
import { isExlight, useAnimationLevel } from '../../hooks/useAnimationLevel'
// CSRF 防护
import { getCSRFToken } from '../../utils/csrf'
import { getUIConfigDeduped } from '../../utils/requestDedup'
import {
  HOST_PANEL_STORE_ID,
  isHostPanelId,
  isStoreHostPanel,
} from '../constants/hostPanels'
import { TAPP_ICON_TOKENS } from '../constants/icons'
import { useWindowAgentHandler } from '../hooks/useWindowAgentHandler'
import { getTappRuntime } from '../runtime'
import { loadPageResources } from '../runtime/sandbox/resourceLoader'
import { TappPageSandbox } from '../runtime/TappPageSandbox'
import { resolveManifestText } from '../utils/manifestLocale'
import {
  resolveTappCategory,
  TAPP_CATEGORIES,
  TAPP_CATEGORY_I18N_KEYS,
} from '../utils/tappCategories'
import {
  getTappIconAccentColor,
  getTappIconStyle,
} from '../utils/tappColors'
import { TappIcon } from './TappIcon'
import { TappIconBadge } from './TappIconBadge'
import { TappStore } from './TappStore'
import './TappWindowManager.css'

const API_URL = CONFIG_API_URL

/** 窗口种类：真实沙箱 Tapp 或宿主 React 面板 */
export type TappWindowKind = 'tapp' | 'host'

/** 窗口状态 */
export interface TappWindow {
  /** 唯一窗口ID */
  windowId: string
  /** Tapp ID，或宿主面板 ID（如 myriad:host.store） */
  tappId: string
  /** 窗口种类，默认 tapp */
  kind: TappWindowKind
  /** Tapp 实例（host 为 null） */
  tapp: TappInstance | null
  /** Tapp 代码（host 为 null） */
  code: TappCodeStructure | null
  /** 加载状态 */
  loading: boolean
  /** 错误信息 */
  error: string | null
  /** 窗口位置 */
  position: { x: number; y: number }
  /** 窗口尺寸 */
  size: { width: number; height: number }
  /** 是否最大化 */
  isMaximized: boolean
  /** 是否最小化（藏入 Dock，实例仍保留） */
  isMinimized: boolean
  /** 层级 */
  zIndex: number
}

/** 商店宿主面板默认尺寸（比单应用窗口更宽） */
/** 商店宿主面板默认尺寸（宽屏：侧栏 + 内容区） */
const HOST_STORE_WINDOW_SIZE = { width: 960, height: 720 }

/** 窗口管理器 Props */
export interface TappWindowManagerProps {
  /** 初始 Tapp ID */
  initialTappId?: string
  /** 返回回调 */
  onBack?: () => void
}

/** 最大窗口数量 */
const MAX_WINDOWS = 5

/** Dock 直接展示的已安装应用上限；超出收入应用面板 */
const MAX_DOCK_APPS = 18

/** Launchpad：每行 7 个，最多 3 行 → 每页 21 */
const LAUNCHPAD_COLS = 7
const LAUNCHPAD_ROWS = 3
const LAUNCHPAD_PAGE_SIZE = LAUNCHPAD_COLS * LAUNCHPAD_ROWS

/** 默认窗口尺寸（移动端竖屏比例） */
const DEFAULT_WINDOW_SIZE = { width: 400, height: 600 }

type LaunchpadEntry =
  | { kind: 'store' }
  | { kind: 'app'; tapp: TappInstance }

/** 窗口方案中的窗口配置 */
interface WindowSchemeItem {
  tappId: string
  position: { x: number; y: number }
  size: { width: number; height: number }
}

/** 保存的窗口方案 */
interface WindowScheme {
  id: string
  name: string
  windows: WindowSchemeItem[]
  createdAt: number
}

/** 最小窗口尺寸（与默认尺寸同步） */
const MIN_WINDOW_SIZE = { ...DEFAULT_WINDOW_SIZE }

const WINDOW_CONTROL_HOVER_CLASS = 'tapp-window-control'
const WINDOW_CONTROL_DANGER_HOVER_CLASS =
  'tapp-window-control tapp-window-control-danger'

const WINDOW_CONTROL_HOVER_STYLE = {
  '--tapp-window-control-hover-bg': 'var(--bg-hover)',
} as React.CSSProperties

const WINDOW_CONTROL_DANGER_HOVER_STYLE = {
  '--tapp-window-control-hover-bg':
    'color-mix(in srgb, var(--color-error, #ef4444) 12%, transparent)',
  '--tapp-window-control-danger-color': 'var(--color-error, #ef4444)',
  color: 'var(--text-muted)',
} as React.CSSProperties

/**
 * 生成唯一窗口ID
 */
function generateWindowId(): string {
  return `window-${Date.now()}-${Math.random().toString(36).slice(2, 11)}`
}

/**
 * 计算新窗口的初始位置（级联效果）
 */
function getInitialPosition(windowCount: number): { x: number; y: number } {
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
  onMinimize: (windowId: string) => void
  onFocus: (windowId: string) => void
  onMove: (windowId: string, position: { x: number; y: number }) => void
  onResize: (windowId: string, size: { width: number; height: number }) => void
  containerBounds: { width: number; height: number }
}

const TappWindowComponent: React.FC<TappWindowComponentProps> = React.memo(
  ({
    window,
    isActive,
    onClose,
    onMinimize,
    onFocus,
    onMove,
    onResize,
    containerBounds,
  }) => {
    const { t, locale } = useI18n()
    const animConfig = useAnimationLevel()
    const noAnimation = isExlight(animConfig)
    const isStorePanel = isStoreHostPanel(window.tappId)
    const windowTappName = isStorePanel
      ? t.tapp.storeTitle
      : window.tapp
        ? resolveManifestText(window.tapp.manifest, locale).name
        : ''

    const windowRef = useRef<HTMLDivElement>(null)
    const [isDragging, setIsDragging] = useState(false)
    const [isResizing, setIsResizing] = useState(false)
    const [resizeDirection, setResizeDirection] = useState<string | null>(null)

    const dragStartRef = useRef({ x: 0, y: 0 })
    const positionStartRef = useRef({ x: 0, y: 0 })
    const sizeStartRef = useRef({ width: 0, height: 0 })

    // 用于追踪交互过程中的实时位置和大小（直接操作 DOM 时使用）
    const currentPositionRef = useRef({
      x: window.position.x,
      y: window.position.y,
    })
    const currentSizeRef = useRef({
      width: window.size.width,
      height: window.size.height,
    })

    // 始终同步 props 到 ref，确保方案保存时能获取最新值
    // 注意：交互过程中 ref 会被直接修改，但交互结束后会同步回 state
    useEffect(() => {
      currentPositionRef.current = {
        x: window.position.x,
        y: window.position.y,
      }
      currentSizeRef.current = {
        width: window.size.width,
        height: window.size.height,
      }
    }, [
      window.position.x,
      window.position.y,
      window.size.width,
      window.size.height,
    ])

    // 缓存图标样式计算（宿主商店用固定 token）
    const iconStyle = useMemo(() => {
      if (isStoreHostPanel(window.tappId)) return null
      return window.tapp ? getTappIconStyle(window.tapp.manifest) : null
    }, [window.tapp, window.tappId])

    // 拖拽处理 - 支持鼠标和触摸
    const handleDragStart = useCallback(
      (e: React.MouseEvent | React.TouchEvent) => {
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
      },
      [window.windowId, onFocus],
    )

    // 调整大小处理 - 支持鼠标和触摸
    const handleResizeStart = useCallback(
      (e: React.MouseEvent | React.TouchEvent, direction: string) => {
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
      },
      [window.windowId, onFocus],
    )

    // 移动处理 - 使用 requestAnimationFrame 节流优化性能，支持鼠标和触摸
    useEffect(() => {
      if (!isDragging && !isResizing) return

      // 页面不可见时不处理拖拽（由调度器可见性状态控制）
      if (!isPageVisible()) return

      let rafId: number | null = null
      let lastX = dragStartRef.current.x
      let lastY = dragStartRef.current.y

      const handleMove = (e: MouseEvent | TouchEvent) => {
        // 获取坐标（支持鼠标和触摸）
        const clientX =
          'touches' in e ? (e.touches[0]?.clientX ?? lastX) : e.clientX
        const clientY =
          'touches' in e ? (e.touches[0]?.clientY ?? lastY) : e.clientY

        // 避免重复计算相同位置
        if (clientX === lastX && clientY === lastY) return
        lastX = clientX
        lastY = clientY

        // 取消上一次未执行的 RAF
        if (rafId) cancelAnimationFrame(rafId)

        rafId = requestAnimationFrame(() => {
          if (!windowRef.current) return

          const deltaX = clientX - dragStartRef.current.x
          const deltaY = clientY - dragStartRef.current.y

          if (isDragging) {
            // 拖拽移动 - 直接操作 DOM
            let newX = positionStartRef.current.x + deltaX
            let newY = positionStartRef.current.y + deltaY

            // 边界限制
            newX = Math.max(
              0,
              Math.min(
                newX,
                containerBounds.width - currentSizeRef.current.width,
              ),
            )
            newY = Math.max(
              0,
              Math.min(
                newY,
                containerBounds.height - currentSizeRef.current.height,
              ),
            )

            // 使用 transform 进行 GPU 加速定位
            windowRef.current.style.transform = `translate3d(${newX}px, ${newY}px, 0)`
            currentPositionRef.current = { x: newX, y: newY }
          } else if (isResizing && resizeDirection) {
            // 调整大小 - 直接操作 DOM
            let newWidth = sizeStartRef.current.width
            let newHeight = sizeStartRef.current.height
            let newX = positionStartRef.current.x
            let newY = positionStartRef.current.y

            if (resizeDirection.includes('e')) {
              newWidth = Math.max(
                MIN_WINDOW_SIZE.width,
                sizeStartRef.current.width + deltaX,
              )
            }
            if (resizeDirection.includes('w')) {
              const widthDelta = Math.min(
                deltaX,
                sizeStartRef.current.width - MIN_WINDOW_SIZE.width,
              )
              newWidth = sizeStartRef.current.width - widthDelta
              newX = positionStartRef.current.x + widthDelta
            }
            if (resizeDirection.includes('s')) {
              newHeight = Math.max(
                MIN_WINDOW_SIZE.height,
                sizeStartRef.current.height + deltaY,
              )
            }
            if (resizeDirection.includes('n')) {
              const heightDelta = Math.min(
                deltaY,
                sizeStartRef.current.height - MIN_WINDOW_SIZE.height,
              )
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
        if (rafId) cancelAnimationFrame(rafId)

        // 交互结束时一次性同步状态到 React
        if (isDragging) {
          onMove(window.windowId, currentPositionRef.current)
        } else if (isResizing) {
          onResize(window.windowId, currentSizeRef.current)
          if (
            resizeDirection?.includes('w') ||
            resizeDirection?.includes('n')
          ) {
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
        if (rafId) cancelAnimationFrame(rafId)
        document.removeEventListener('mousemove', handleMove)
        document.removeEventListener('mouseup', handleEnd)
        document.removeEventListener('touchmove', handleMove)
        document.removeEventListener('touchend', handleEnd)
        document.removeEventListener('touchcancel', handleEnd)
      }
      // 注意：onMove, onResize, window.windowId 通过闭包捕获，不加入依赖以避免不必要的重新绑定
    }, [isDragging, isResizing, resizeDirection, containerBounds])

    // 计算窗口样式 - 使用 transform 进行 GPU 加速
    const windowStyle = useMemo(
      () => ({
        width: window.size.width,
        height: window.size.height,
        zIndex: window.zIndex,
        // 使用 transform 替代 top/left，启用 GPU 加速
        transform: `translate3d(${window.position.x}px, ${window.position.y}px, 0)`,
        // 只在非交互时启用过渡
        transition: isDragging || isResizing ? 'none' : 'box-shadow 0.15s',
      }),
      [window.position, window.size, window.zIndex, isDragging, isResizing],
    )

    // 交互状态 - 用于显示遮罩层
    const isInteracting = isDragging || isResizing

    // 缓存 boxShadow 样式 - 使用更简单的阴影以提升性能
    const boxShadowStyle = useMemo(
      () => ({
        boxShadow: isActive
          ? '0 8px 24px rgba(0, 0, 0, 0.2)'
          : '0 4px 12px rgba(0, 0, 0, 0.1)',
        border: '1px solid var(--border-color)',
      }),
      [isActive],
    )

    // 缓存标题栏样式（exlight：不透明底，避免关 blur 后仍透壁纸）
    const headerStyle = useMemo(
      () => ({
        borderBottom: `1px solid ${isStorePanel ? 'var(--surface-border)' : 'var(--border-color)'}`,
        opacity: isActive ? 1 : 0.7,
        transition: 'opacity 0.2s ease',
      }),
      [isActive, isStorePanel],
    )

    // 缓存窗口点击处理函数
    const handleWindowClick = useCallback(() => {
      onFocus(window.windowId)
    }, [onFocus, window.windowId])

    // 缓存关闭 / 最小化按钮处理函数
    const handleCloseClick = useCallback(
      (e: React.MouseEvent) => {
        e.stopPropagation()
        onClose(window.windowId)
      },
      [onClose, window.windowId],
    )

    const handleMinimizeClick = useCallback(
      (e: React.MouseEvent) => {
        e.stopPropagation()
        onMinimize(window.windowId)
      },
      [onMinimize, window.windowId],
    )

    /** 标题栏控件：阻止 mousedown 冒泡触发拖拽 */
    const stopTitleControlPointer = useCallback(
      (e: React.MouseEvent | React.TouchEvent) => {
        e.stopPropagation()
      },
      [],
    )

    // 调整大小的手柄：命中区样式在 TappWindowManager.css（比 4px 边框更易抓取）
    const resizeHandles = useMemo(
      () =>
        (['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'] as const).map(
          (direction) => ({
            direction,
            className: `tapp-window-resize-handle tapp-window-resize-${direction}`,
          }),
        ),
      [],
    )

    return (
      <div
        ref={windowRef}
        className="absolute flex flex-col overflow-visible rounded-xl"
        style={{
          top: 0,
          left: 0,
          ...windowStyle,
          ...(window.isMinimized
            ? {
                // 最小化：保留挂载与沙箱状态，仅隐藏
                visibility: 'hidden' as const,
                pointerEvents: 'none' as const,
                zIndex: 0,
                boxShadow: 'none',
              }
            : boxShadowStyle),
        }}
        onClick={window.isMinimized ? undefined : handleWindowClick}
        aria-hidden={window.isMinimized || undefined}
      >
        {/* 窗口标题栏 - 可拖拽（支持鼠标和触摸） */}
        <div
          className={`flex items-center justify-between px-3 h-10 shrink-0 select-none rounded-t-xl ${isStorePanel ? 'glass glass-chrome-free' : 'glass-surface glass-80'} ${isDragging ? 'cursor-grabbing' : 'cursor-grab'}`}
          style={headerStyle}
          onMouseDown={handleDragStart}
          onTouchStart={handleDragStart}
        >
          {/* 左侧：拖拽手柄 + 图标 + 名称 */}
          <div className="flex items-center gap-2 min-w-0">
            <FaGripVertical
              className="w-3 h-3 shrink-0"
              style={{ color: 'var(--text-muted)' }}
            />

            {window.loading ? (
              <div
                className="w-6 h-6 rounded-lg flex items-center justify-center"
                style={{ backgroundColor: 'var(--bg-hover)' }}
              >
                <Spinner size="xs" color="var(--text-muted)" />
              </div>
            ) : window.error ? (
              <div className="flex items-center gap-2">
                <div className="w-6 h-6 rounded-lg bg-red-100 dark:bg-red-900/30 flex items-center justify-center">
                  <FaExclamationTriangle className="w-3 h-3 text-red-500" />
                </div>
                <span className="text-xs text-red-500 truncate">
                  {t.tapp.loadAppFailed}
                </span>
              </div>
            ) : isStorePanel ? (
              <div className="flex items-center gap-2 min-w-0">
                <div className="flex items-center justify-center shrink-0">
                  <TappIcon
                    icon={TAPP_ICON_TOKENS.store}
                    name={windowTappName}
                    sizeClass="w-4 h-4"
                    textSizeClass="text-xs"
                  />
                </div>
                <span
                  className="text-xs font-medium truncate"
                  style={{ color: 'var(--text-primary)' }}
                >
                  {windowTappName}
                </span>
              </div>
            ) : window.tapp && iconStyle ? (
              <div className="flex items-center gap-2 min-w-0">
                <TappIconBadge
                  icon={window.tapp.manifest.icon}
                  iconSvg={window.tapp.manifest.iconSvg}
                  name={windowTappName}
                  id={window.tapp.manifest.id || window.tapp.id}
                  themeColor={window.tapp.manifest.themeColor}
                  category={window.tapp.manifest.category}
                  permissions={window.tapp.manifest.permissions}
                  iconStyle={iconStyle}
                  shellClassName="tapp-page-icon w-6 h-6"
                  glyphSizeClass="w-3 h-3"
                  glyphTextClass="text-xs"
                />
                <span
                  className="text-xs font-medium truncate"
                  style={{ color: 'var(--text-primary)' }}
                >
                  {windowTappName}
                </span>
                <span
                  className="text-[10px] shrink-0"
                  style={{ color: 'var(--text-muted)' }}
                >
                  v{window.tapp.manifest.version}
                </span>
              </div>
            ) : null}
          </div>

          {/* 右侧：最小化 + 关闭 */}
          <div className="flex items-center gap-0.5 shrink-0">
            <motion.button
              type="button"
              onClick={handleMinimizeClick}
              onMouseDown={stopTitleControlPointer}
              onTouchStart={stopTitleControlPointer}
              className="p-1.5 text-gray-500 hover:text-gray-800 dark:hover:text-gray-200 hover:bg-black/5 dark:hover:bg-white/10 rounded transition-colors"
              title={t.tapp.minimize}
              aria-label={t.tapp.minimize}
              whileHover={noAnimation ? undefined : { scale: 1.1 }}
              whileTap={noAnimation ? undefined : { scale: 0.9 }}
            >
              <LuMinus className="w-3 h-3" />
            </motion.button>
            <motion.button
              type="button"
              onClick={handleCloseClick}
              onMouseDown={stopTitleControlPointer}
              onTouchStart={stopTitleControlPointer}
              className="p-1.5 text-gray-500 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded transition-colors"
              title={t.common.close}
              aria-label={t.common.close}
              whileHover={noAnimation ? undefined : { scale: 1.1 }}
              whileTap={noAnimation ? undefined : { scale: 0.9 }}
            >
              <FaTimes className="w-3 h-3" />
            </motion.button>
          </div>
        </div>

        {/* 窗口内容（圆角 + 裁剪在此层，外层 overflow-visible 以便缩放命中区伸出边框） */}
        <div
          className="flex-1 overflow-hidden relative rounded-b-xl"
          style={{
            backgroundColor: isStorePanel ? 'transparent' : 'var(--bg-primary)',
          }}
        >
          {/* 交互时显示遮罩层，防止 iframe 捕获事件并避免重绘 */}
          {isInteracting && (
            <div
              className="absolute inset-0 z-50"
              style={{ backgroundColor: 'transparent' }}
            />
          )}
          {window.loading ? (
            <div className="w-full h-full flex items-center justify-center">
              <Spinner size="lg" />
            </div>
          ) : window.error ? (
            <div className="w-full h-full flex items-center justify-center">
              <div className="text-center max-w-xs mx-4">
                <FaExclamationTriangle className="w-10 h-10 mx-auto text-red-500 mb-3" />
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  {window.error}
                </p>
              </div>
            </div>
          ) : isStorePanel ? (
            <div
              data-window-id={window.windowId}
              data-tapp-id={window.tappId}
              data-host-panel="store"
              className="tapp-store-frame--wallpaper absolute inset-0"
            >
              <TappStore className="h-full" embeddedChrome compact />
            </div>
          ) : window.tapp && window.code ? (
            <div
              data-window-id={window.windowId}
              data-tapp-id={window.tappId}
              className="absolute inset-0"
            >
              <TappPageSandbox
                tappInstance={window.tapp}
                code={window.code}
                paused={window.isMinimized}
                onError={(err) => console.error('[TappWindow] Error:', err)}
              />
            </div>
          ) : null}
        </div>

        {/* 调整大小的手柄（支持鼠标和触摸；命中区见 CSS） */}
        {resizeHandles.map(({ direction, className }) => (
          <div
            key={direction}
            className={className}
            onMouseDown={(e) => handleResizeStart(e, direction)}
            onTouchStart={(e) => handleResizeStart(e, direction)}
          />
        ))}
      </div>
    )
  },
  (prevProps, nextProps) => {
    // 自定义比较函数，只在关键属性变化时重新渲染
    return (
      prevProps.window.windowId === nextProps.window.windowId &&
      prevProps.window.kind === nextProps.window.kind &&
      prevProps.window.tappId === nextProps.window.tappId &&
      prevProps.window.position.x === nextProps.window.position.x &&
      prevProps.window.position.y === nextProps.window.position.y &&
      prevProps.window.size.width === nextProps.window.size.width &&
      prevProps.window.size.height === nextProps.window.size.height &&
      prevProps.window.zIndex === nextProps.window.zIndex &&
      prevProps.window.isMinimized === nextProps.window.isMinimized &&
      prevProps.window.loading === nextProps.window.loading &&
      prevProps.window.error === nextProps.window.error &&
      prevProps.window.tapp === nextProps.window.tapp &&
      prevProps.window.code === nextProps.window.code &&
      prevProps.isActive === nextProps.isActive &&
      prevProps.containerBounds.width === nextProps.containerBounds.width &&
      prevProps.containerBounds.height === nextProps.containerBounds.height
    )
  },
)

// 设置 displayName 便于调试
TappWindowComponent.displayName = 'TappWindowComponent'

/**
 * Tapp 多窗口管理器
 */
export const TappWindowManager: React.FC<TappWindowManagerProps> = ({
  initialTappId,
  onBack,
}) => {
  const { t, locale } = useI18n()
  const { isAuthenticated } = useAuth()
  const animConfig = useAnimationLevel()
  const noAnimation = isExlight(animConfig)
  const runtime = getTappRuntime()

  const containerRef = useRef<HTMLDivElement>(null)
  const schemeMenuRef = useRef<HTMLDivElement>(null)
  const launchpadSearchRef = useRef<HTMLInputElement>(null)
  const launchpadStageRef = useRef<HTMLDivElement>(null)
  const [containerBounds, setContainerBounds] = useState({
    width: 0,
    height: 0,
  })
  const [windows, setWindows] = useState<TappWindow[]>([])
  const [activeWindowId, setActiveWindowId] = useState<string | null>(null)
  const [nextZIndex, setNextZIndex] = useState(100)
  const [availableTapps, setAvailableTapps] = useState<TappInstance[]>([])
  const [showSchemeMenu, setShowSchemeMenu] = useState(false)
  const [showLaunchpad, setShowLaunchpad] = useState(false)
  const [launchpadQuery, setLaunchpadQuery] = useState('')
  const [launchpadCategory, setLaunchpadCategory] = useState<
    TappCategory | 'all'
  >('all')
  const [launchpadPage, setLaunchpadPage] = useState(0)
  const [savedSchemes, setSavedSchemes] = useState<WindowScheme[]>([])

  // 用于防抖的 ref
  const resizeTimeoutRef = useRef<number | null>(null)
  const [isSaving, setIsSaving] = useState(false)

  // 用 ref 跟踪 windows 和 activeWindowId，避免 agent handler 的 useEffect 因 windows 变化频繁重注册
  const windowsRef = useRef(windows)
  windowsRef.current = windows
  const activeWindowIdRef = useRef(activeWindowId)
  activeWindowIdRef.current = activeWindowId

  // 注册页面到统一调度器（页面级生命周期管理）
  useEffect(() => {
    startPage('tapp-multi')
  }, [])

  // 点击外部关闭方案菜单
  useEffect(() => {
    if (!showSchemeMenu) return

    const handleClickOutside = (e: MouseEvent) => {
      if (
        schemeMenuRef.current &&
        !schemeMenuRef.current.contains(e.target as Node)
      ) {
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
      } catch (e) {
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

  /** Dock 已安装列表：内存快照 + 事件驱动刷新（避免 idle 延迟 / 商店装完不同步） */
  const refreshDockApps = useCallback(() => {
    const next = runtime
      .getAllTapps()
      .filter((item) => item.manifest.hasPage)
    setAvailableTapps((prev) => {
      if (
        prev.length === next.length &&
        prev.every(
          (p, i) =>
            p.id === next[i]?.id &&
            p.manifest.version === next[i]?.manifest.version &&
            p.manifest.icon === next[i]?.manifest.icon &&
            p.manifest.themeColor === next[i]?.manifest.themeColor &&
            p.manifest.category === next[i]?.manifest.category,
        )
      ) {
        return prev
      }
      return next
    })
  }, [runtime])

  useEffect(() => {
    let cancelled = false

    // 立即用当前缓存填 Dock，避免 scheduleIdle 造成空坞
    refreshDockApps()

    const syncThenRefresh = async () => {
      await runtime.waitForSync()
      if (!cancelled) refreshDockApps()
    }
    void syncThenRefresh()

    const onChange = () => {
      if (!cancelled) refreshDockApps()
    }
    const unsubInstalled = runtime.on('tapp:installed', onChange)
    const unsubUninstalled = runtime.on('tapp:uninstalled', (data) => {
      const id = (data as { id?: string })?.id
      if (id) {
        // 卸载后关掉对应窗口，指示点与列表一并收敛
        setWindows((prev) => {
          const remaining = prev.filter((w) => w.tappId !== id)
          setActiveWindowId((cur) => {
            if (cur && remaining.some((w) => w.windowId === cur)) return cur
            if (remaining.length === 0) return null
            return remaining.reduce((a, b) =>
              a.zIndex >= b.zIndex ? a : b,
            ).windowId
          })
          return remaining
        })
      }
      onChange()
    })
    const unsubUpdated = runtime.on('tapp:updated', onChange)
    const unsubSync = runtime.on('sync:complete', onChange)

    const onVisible = () => {
      if (isPageVisible()) onChange()
    }
    document.addEventListener('visibilitychange', onVisible)

    return () => {
      cancelled = true
      unsubInstalled()
      unsubUninstalled()
      unsubUpdated()
      unsubSync()
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [runtime, refreshDockApps])

  // 更新已打开的多窗口实例。资源缓存代际已在 runtime 事件发出前提升，所有同 ID
  // 窗口共享一次重新加载，然后各自重建沙箱。宿主面板跳过。
  useEffect(() => {
    let cancelled = false
    const unsubscribe = runtime.on('tapp:updated', (data) => {
      const tappId = (data as { id: string }).id
      if (isHostPanelId(tappId)) return
      setWindows((prev) =>
        prev.map((item) =>
          item.kind === 'tapp' && item.tappId === tappId
            ? { ...item, loading: true, error: null, tapp: null, code: null }
            : item,
        ),
      )
      void (async () => {
        try {
          const instance = runtime.getTapp(tappId)
          if (!instance) throw new Error(t.tapp.appNotExist)
          const resources = await loadPageResources(instance)
          if (cancelled) return
          const code: TappCodeStructure = {
            core: resources.core,
            page: resources.page,
            pageHtml: resources.html,
            styles: resources.styles,
            pageCSS: resources.css,
            i18n: resources.i18n,
            pageModules: resources.pageModules,
            pageModuleOrder: resources.pageModuleOrder,
          }
          setWindows((prev) =>
            prev.map((item) =>
              item.tappId === tappId
                ? { ...item, tapp: instance, code, loading: false }
                : item,
            ),
          )
        } catch (error) {
          if (cancelled) return
          const message =
            error instanceof Error ? error.message : t.tapp.loadAppFailed
          setWindows((prev) =>
            prev.map((item) =>
              item.tappId === tappId
                ? { ...item, loading: false, error: message }
                : item,
            ),
          )
        }
      })()
    })
    return () => {
      cancelled = true
      unsubscribe()
    }
  }, [runtime, t.tapp.appNotExist, t.tapp.loadAppFailed])

  // 初始化第一个窗口
  useEffect(() => {
    if (initialTappId && windows.length === 0) {
      openTappWindow(initialTappId)
    }
  }, [initialTappId])

  // 打开新的 Tapp / 宿主面板窗口（opts.size/position from Agent open_window when provided）
  const openTappWindow = useCallback(
    async (
      tappId: string,
      opts?: {
        size?: { width?: number; height?: number }
        position?: { x?: number; y?: number }
      },
    ) => {
      if (windows.length >= MAX_WINDOWS) {
        console.warn('Maximum window limit reached')
        return
      }

      const isHost = isHostPanelId(tappId)
      // 商店宿主面板：已打开则聚焦，避免重复占用窗口位
      if (isHost && isStoreHostPanel(tappId)) {
        const existing = windows.find(
          (w) => w.kind === 'host' && isStoreHostPanel(w.tappId),
        )
        if (existing) {
          setActiveWindowId(existing.windowId)
          setWindows((prev) =>
            prev.map((w) =>
              w.windowId === existing.windowId
                ? { ...w, zIndex: nextZIndex, isMinimized: false }
                : w,
            ),
          )
          setNextZIndex((prev) => prev + 1)
          return
        }
      }

      const windowId = generateWindowId()
      const basePos = getInitialPosition(windows.length)
      const position = {
        x:
          typeof opts?.position?.x === 'number' &&
          Number.isFinite(opts.position.x)
            ? opts.position.x
            : basePos.x,
        y:
          typeof opts?.position?.y === 'number' &&
          Number.isFinite(opts.position.y)
            ? opts.position.y
            : basePos.y,
      }
      const defaultSize = isStoreHostPanel(tappId)
        ? HOST_STORE_WINDOW_SIZE
        : DEFAULT_WINDOW_SIZE
      const size = {
        width:
          typeof opts?.size?.width === 'number' &&
          Number.isFinite(opts.size.width) &&
          opts.size.width > 0
            ? opts.size.width
            : defaultSize.width,
        height:
          typeof opts?.size?.height === 'number' &&
          Number.isFinite(opts.size.height) &&
          opts.size.height > 0
            ? opts.size.height
            : defaultSize.height,
      }

      // 宿主面板：无需沙箱加载，直接就绪
      if (isHost) {
        if (!isStoreHostPanel(tappId)) {
          console.warn('[TappWindowManager] Unknown host panel:', tappId)
          return
        }
        const hostWindow: TappWindow = {
          windowId,
          tappId,
          kind: 'host',
          tapp: null,
          code: null,
          loading: false,
          error: null,
          position,
          size,
          isMaximized: false,
          isMinimized: false,
          zIndex: nextZIndex,
        }
        setWindows((prev) => [...prev, hostWindow])
        setActiveWindowId(windowId)
        setNextZIndex((prev) => prev + 1)
        return
      }

      // 创建初始窗口状态
      const newWindow: TappWindow = {
        windowId,
        tappId,
        kind: 'tapp',
        tapp: null,
        code: null,
        loading: true,
        error: null,
        position,
        size,
        isMaximized: false,
        isMinimized: false,
        zIndex: nextZIndex,
      }

      setWindows((prev) => [...prev, newWindow])
      setActiveWindowId(windowId)
      setNextZIndex((prev) => prev + 1)

      // 异步加载 Tapp
      try {
        await runtime.waitForSync()

        const instance = runtime.getTapp(tappId)
        if (!instance) {
          setWindows((prev) =>
            prev.map((w) =>
              w.windowId === windowId
                ? { ...w, loading: false, error: t.tapp.appNotExist }
                : w,
            ),
          )
          return
        }

        const resources = await loadPageResources(instance)
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

        setWindows((prev) =>
          prev.map((w) =>
            w.windowId === windowId
              ? { ...w, tapp: instance, code: tappCode, loading: false }
              : w,
          ),
        )
      } catch (err) {
        setWindows((prev) =>
          prev.map((w) =>
            w.windowId === windowId
              ? {
                  ...w,
                  loading: false,
                  error:
                    err instanceof Error ? err.message : t.tapp.loadAppFailed,
                }
              : w,
          ),
        )
      }
    },
    [windows, nextZIndex, runtime, t.tapp.appNotExist, t.tapp.loadAppFailed],
  )

  // 关闭窗口（不触发暂停应用逻辑，应用继续在后台运行）
  const closeWindow = useCallback(
    (windowId: string) => {
      setWindows((prev) => {
        const remaining = prev.filter((w) => w.windowId !== windowId)
        // 如果关闭的是活动窗口，激活下一个可见窗口
        if (activeWindowId === windowId && remaining.length > 0) {
          const candidates = remaining.filter((w) => !w.isMinimized)
          const pool = candidates.length > 0 ? candidates : remaining
          const topWindow = pool.reduce((a, b) =>
            a.zIndex > b.zIndex ? a : b,
          )
          setActiveWindowId(topWindow.windowId)
        } else if (remaining.length === 0) {
          setActiveWindowId(null)
        }
        return remaining
      })
    },
    [activeWindowId],
  )

  // Minimize: hide into Dock but keep the sandbox/iframe alive so restore is
  // instant. Work freezes via `paused` → lifecycle:pause (no teardown).
  const minimizeWindow = useCallback(
    (windowId: string) => {
      setWindows((prev) => {
        const next = prev.map((w) =>
          w.windowId === windowId ? { ...w, isMinimized: true } : w,
        )
        if (activeWindowId === windowId) {
          const visible = next.filter((w) => !w.isMinimized)
          if (visible.length > 0) {
            const top = visible.reduce((a, b) =>
              a.zIndex > b.zIndex ? a : b,
            )
            setActiveWindowId(top.windowId)
          } else {
            setActiveWindowId(null)
          }
        }
        return next
      })
    },
    [activeWindowId],
  )

  // 聚焦窗口（同时从最小化恢复）
  const focusWindow = useCallback(
    (windowId: string) => {
      setActiveWindowId(windowId)
      setWindows((prev) =>
        prev.map((w) =>
          w.windowId === windowId
            ? { ...w, zIndex: nextZIndex, isMinimized: false }
            : w,
        ),
      )
      setNextZIndex((prev) => prev + 1)
    },
    [nextZIndex],
  )

  // 移动窗口
  const moveWindow = useCallback(
    (windowId: string, position: { x: number; y: number }) => {
      setWindows((prev) =>
        prev.map((w) => (w.windowId === windowId ? { ...w, position } : w)),
      )
    },
    [],
  )

  // 调整窗口大小
  const resizeWindow = useCallback(
    (windowId: string, size: { width: number; height: number }) => {
      setWindows((prev) =>
        prev.map((w) => (w.windowId === windowId ? { ...w, size } : w)),
      )
    },
    [],
  )

  // Agent 操作处理器（已解耦为 Hook）
  useWindowAgentHandler({
    windowsRef,
    activeWindowIdRef,
    openTappWindow,
    closeWindow,
    focusWindow,
  })

  // 保存方案到云端
  const saveToCloud = useCallback(async (schemes: WindowScheme[]) => {
    try {
      // 获取 CSRF Token
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) {
        console.warn('Failed to get CSRF token, skipping cloud save')
        return
      }

      const response = await fetch(
        `${API_URL}/api/config/tapp-window-schemes`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': csrfToken,
          },
          credentials: 'include',
          body: JSON.stringify({
            schemes: JSON.stringify(schemes),
          }),
        },
      )

      if (!response.ok) {
        throw new Error('Failed to save to cloud')
      }
    } catch (e) {
      console.warn('Failed to save window schemes to cloud:', e)
    }
  }, [])

  // 保存当前窗口方案
  const saveCurrentScheme = useCallback(async () => {
    if (windows.length === 0 || isSaving) return

    setIsSaving(true)

    const schemeWindows: WindowSchemeItem[] = windows
      // 已加载的 Tapp，或就绪的宿主面板
      .filter((w) => w.kind === 'host' || !!w.tapp)
      .map((w) => ({
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
  const loadScheme = useCallback(
    async (scheme: WindowScheme) => {
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
        const isHost = isHostPanelId(schemeWindow.tappId)

        newWindows.push({
          windowId,
          tappId: schemeWindow.tappId,
          kind: isHost ? 'host' : 'tapp',
          tapp: null,
          code: null,
          // 宿主面板无需异步加载
          loading: !isHost,
          error:
            isHost && !isStoreHostPanel(schemeWindow.tappId)
              ? t.tapp.appNotExist
              : null,
          position: { ...schemeWindow.position },
          size: { ...schemeWindow.size },
          isMaximized: false,
          isMinimized: false,
          zIndex: baseZIndex + i,
        })
      }

      // 4. 一次性设置所有窗口（批量更新，减少重渲染）
      if (newWindows.length > 0) {
        setWindows(newWindows)
        setActiveWindowId(newWindows[newWindows.length - 1].windowId)
        setNextZIndex(baseZIndex + newWindows.length)
      }

      // 5. 异步加载所有真实 Tapp 的资源（跳过宿主面板）
      await runtime.waitForSync()

      for (const newWindow of newWindows) {
        if (newWindow.kind === 'host') continue

        const { windowId, tappId } = newWindow

        try {
          const instance = runtime.getTapp(tappId)
          if (!instance) {
            setWindows((prev) =>
              prev.map((w) =>
                w.windowId === windowId
                  ? { ...w, loading: false, error: t.tapp.appNotExist }
                  : w,
              ),
            )
            continue
          }

          const resources = await loadPageResources(instance)
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

          setWindows((prev) =>
            prev.map((w) =>
              w.windowId === windowId
                ? { ...w, tapp: instance, code: tappCode, loading: false }
                : w,
            ),
          )
        } catch (err) {
          setWindows((prev) =>
            prev.map((w) =>
              w.windowId === windowId
                ? {
                    ...w,
                    loading: false,
                    error:
                      err instanceof Error ? err.message : t.tapp.loadAppFailed,
                  }
                : w,
            ),
          )
        }
      }
    },
    [nextZIndex, runtime, t.tapp.appNotExist, t.tapp.loadAppFailed],
  )

  // 删除方案
  const deleteScheme = useCallback(
    async (schemeId: string) => {
      const updatedSchemes = savedSchemes.filter((s) => s.id !== schemeId)
      setSavedSchemes(updatedSchemes)

      await saveToCloud(updatedSchemes)
    },
    [savedSchemes, saveToCloud],
  )

  // Dock 快捷槽：最多 MAX_DOCK_APPS；已打开优先。应用面板入口常显，面板内始终列全部。
  const dockVisibleApps = useMemo(() => {
    const openIds = new Set(
      windows
        .map((w) => w.tappId)
        .filter((id) => !isHostPanelId(id)),
    )
    const openApps: TappInstance[] = []
    const restApps: TappInstance[] = []
    for (const app of availableTapps) {
      if (openIds.has(app.id)) openApps.push(app)
      else restApps.push(app)
    }
    return [...openApps, ...restApps].slice(0, MAX_DOCK_APPS)
  }, [availableTapps, windows])

  /** 应用面板：全部已安装（有页面）应用，与是否溢出无关 */
  const dockPanelApps = availableTapps

  /** 按 tappId 统计打开中的窗口（用于 Dock 指示点） */
  const openCountByTappId = useMemo(() => {
    const map = new Map<string, number>()
    for (const w of windows) {
      map.set(w.tappId, (map.get(w.tappId) ?? 0) + 1)
    }
    return map
  }, [windows])

  const closeLaunchpad = useCallback(() => {
    setShowLaunchpad(false)
    setLaunchpadQuery('')
    setLaunchpadCategory('all')
    setLaunchpadPage(0)
  }, [])

  const openLaunchpad = useCallback(() => {
    setShowLaunchpad(true)
    setLaunchpadQuery('')
    setLaunchpadCategory('all')
    setLaunchpadPage(0)
  }, [])

  /** 已安装应用中出现过的分类（稳定顺序） */
  const launchpadCategories = useMemo(() => {
    const present = new Set<TappCategory>()
    for (const app of dockPanelApps) {
      present.add(resolveTappCategory(app.manifest))
    }
    return TAPP_CATEGORIES.filter((c) => present.has(c))
  }, [dockPanelApps])

  const launchpadCategoryLabel = useCallback(
    (cat: TappCategory) => {
      const key = TAPP_CATEGORY_I18N_KEYS[cat]
      return (t.tapp as Record<string, string>)[key] ?? cat
    },
    [t.tapp],
  )

  /** 启动台：商店 + 已安装（搜索 + 分类），扁平条目供 7×3 分页 */
  const launchpadEntries = useMemo((): LaunchpadEntry[] => {
    const q = launchpadQuery.trim().toLowerCase()
    const storeTitle = t.tapp.storeTitle
    const entries: LaunchpadEntry[] = []
    // 商店仅在「全部」分类下展示
    if (
      launchpadCategory === 'all' &&
      (!q || storeTitle.toLowerCase().includes(q))
    ) {
      entries.push({ kind: 'store' })
    }
    for (const tapp of dockPanelApps) {
      const cat = resolveTappCategory(tapp.manifest)
      if (launchpadCategory !== 'all' && cat !== launchpadCategory) continue
      if (q) {
        const name = resolveManifestText(tapp.manifest, locale).name
        if (!name.toLowerCase().includes(q)) continue
      }
      entries.push({ kind: 'app', tapp })
    }
    return entries
  }, [
    dockPanelApps,
    launchpadCategory,
    launchpadQuery,
    locale,
    t.tapp.storeTitle,
  ])

  const launchpadPageCount = Math.max(
    1,
    Math.ceil(launchpadEntries.length / LAUNCHPAD_PAGE_SIZE) || 1,
  )

  const launchpadPageItems = useMemo(() => {
    const page = Math.min(launchpadPage, launchpadPageCount - 1)
    const start = page * LAUNCHPAD_PAGE_SIZE
    return launchpadEntries.slice(start, start + LAUNCHPAD_PAGE_SIZE)
  }, [launchpadEntries, launchpadPage, launchpadPageCount])

  // 搜索 / 分类变化时回到第一页；页码钳制
  useEffect(() => {
    setLaunchpadPage(0)
  }, [launchpadQuery, launchpadCategory])

  useEffect(() => {
    setLaunchpadPage((p) => Math.min(p, launchpadPageCount - 1))
  }, [launchpadPageCount])

  // 启动台：聚焦搜索；Esc 关闭；点窗外关闭；←/→ 翻页（非输入中）
  useEffect(() => {
    if (!showLaunchpad) return
    const tId = window.setTimeout(() => {
      launchpadSearchRef.current?.focus()
    }, 40)
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        closeLaunchpad()
        return
      }
      const typing =
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement
      if (typing) return
      if (e.key === 'ArrowRight' || e.key === 'PageDown') {
        e.preventDefault()
        setLaunchpadPage((p) => Math.min(p + 1, launchpadPageCount - 1))
      } else if (e.key === 'ArrowLeft' || e.key === 'PageUp') {
        e.preventDefault()
        setLaunchpadPage((p) => Math.max(p - 1, 0))
      }
    }
    const onPointerDown = (e: MouseEvent) => {
      const stage = launchpadStageRef.current
      const target = e.target as Node
      if (stage && !stage.contains(target)) {
        // Dock「应用」入口自行 toggle，勿抢先关掉再被打开
        if (
          target instanceof Element &&
          target.closest('.tapp-multi-dock-more')
        ) {
          return
        }
        closeLaunchpad()
      }
    }
    document.addEventListener('keydown', onKey)
    // 延迟，避免打开时同一 click 立刻关掉
    const outId = window.setTimeout(() => {
      document.addEventListener('mousedown', onPointerDown)
    }, 0)
    return () => {
      window.clearTimeout(tId)
      window.clearTimeout(outId)
      document.removeEventListener('keydown', onKey)
      document.removeEventListener('mousedown', onPointerDown)
    }
  }, [showLaunchpad, closeLaunchpad, launchpadPageCount])

  /**
   * Dock 点击：已有窗口则聚焦（含从最小化恢复）；Alt/⌘/Ctrl 强制新开。
   * 同 app 多窗时：若顶层已是活动且可见，则恢复下一扇最小化副本，否则聚焦 z 最高。
   */
  const activateFromDock = useCallback(
    (tappId: string, forceNew: boolean) => {
      if (!forceNew) {
        const same = windows.filter((w) => w.tappId === tappId)
        if (same.length > 0) {
          const byZ = [...same].sort((a, b) => b.zIndex - a.zIndex)
          const top = byZ[0]
          const minimizedTop = byZ.find((w) => w.isMinimized)
          const target =
            !top.isMinimized &&
            activeWindowId === top.windowId &&
            minimizedTop
              ? minimizedTop
              : top
          focusWindow(target.windowId)
          closeLaunchpad()
          return
        }
      }
      if (windows.length >= MAX_WINDOWS) {
        console.warn('Maximum window limit reached')
        return
      }
      void openTappWindow(tappId)
      closeLaunchpad()
    },
    [windows, activeWindowId, focusWindow, openTappWindow, closeLaunchpad],
  )

  const dockAtMax = windows.length >= MAX_WINDOWS

  /** 渲染单个 Dock 应用图标 */
  const renderDockAppItem = useCallback(
    (tapp: TappInstance) => {
      const style = getTappIconStyle(tapp.manifest)
      const accent = getTappIconAccentColor({
        icon: tapp.manifest.icon,
        iconSvg: tapp.manifest.iconSvg,
        themeColor: tapp.manifest.themeColor,
        category: tapp.manifest.category,
        id: tapp.id,
        permissions: tapp.manifest.permissions,
      })
      const text = resolveManifestText(tapp.manifest, locale)
      const openCount = openCountByTappId.get(tapp.id) ?? 0
      const isOpen = openCount > 0
      const appLabel = `${text.name}${isOpen && openCount > 1 ? ` (${openCount})` : ''}${dockAtMax && !isOpen ? ` · ${t.tapp.dockAtMax}` : ''}`
      return (
        <motion.button
          key={tapp.id}
          type="button"
          className={`tapp-multi-dock-item${isOpen ? ' is-open' : ''}`}
          onClick={(e: React.MouseEvent) =>
            activateFromDock(tapp.id, e.altKey || e.metaKey || e.ctrlKey)
          }
          aria-label={appLabel}
          whileHover={noAnimation ? undefined : { y: -5, scale: 1.1 }}
          whileTap={noAnimation ? undefined : { scale: 0.92 }}
        >
          <span className="tapp-multi-dock-label" aria-hidden>
            {appLabel}
          </span>
          <span className="tapp-multi-dock-icon">
            <TappIconBadge
              icon={tapp.manifest.icon}
              iconSvg={tapp.manifest.iconSvg}
              name={text.name}
              id={tapp.manifest.id || tapp.id}
              themeColor={tapp.manifest.themeColor}
              category={tapp.manifest.category}
              permissions={tapp.manifest.permissions}
              iconStyle={style}
              shellClassName="tapp-multi-dock-badge-shell h-full w-full"
              glyphSizeClass="w-[55%] h-[55%]"
              glyphTextClass="text-[0.7em]"
            />
          </span>
          <span
            className="tapp-multi-dock-dot"
            style={{
              opacity: isOpen ? 1 : 0,
              background: accent,
            }}
            aria-hidden
          />
        </motion.button>
      )
    },
    [
      activateFromDock,
      dockAtMax,
      locale,
      noAnimation,
      openCountByTappId,
      t.tapp.dockAtMax,
    ],
  )

  return (
    // z-100：高于 NavigationIsland（z-50），避免底栏被挡住
    <div className="fixed inset-0 z-100 overflow-hidden" data-no-ripple>
      {/* 顶部工具栏：返回 + 方案 + 窗口计数（不再弹中间选择器） */}
      <div className="absolute top-4 left-4 z-1000">
        <div
          className="flex items-center gap-2 rounded-xl px-2 py-1.5 glass-surface glass-80"
          style={{
            border: '1px solid var(--border-color)',
            boxShadow: '0 4px 12px rgba(0, 0, 0, 0.1)',
          }}
        >
          {onBack && (
            <motion.button
              onClick={onBack}
              className={`flex items-center gap-1.5 rounded-lg px-3 py-2 transition-colors ${WINDOW_CONTROL_HOVER_CLASS}`}
              style={{
                ...WINDOW_CONTROL_HOVER_STYLE,
                color: 'var(--text-secondary)',
              }}
              whileTap={noAnimation ? undefined : { scale: 0.95 }}
              title={t.tapp.back}
            >
              <svg
                className="h-4 w-4"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M15 19l-7-7 7-7"
                />
              </svg>
            </motion.button>
          )}

          {isAuthenticated && (
            <>
              {onBack && (
                <div
                  className="h-6 w-px"
                  style={{ backgroundColor: 'var(--border-color)' }}
                />
              )}

              <div className="relative" ref={schemeMenuRef}>
                <motion.button
                  onClick={() => setShowSchemeMenu(!showSchemeMenu)}
                  className={`flex items-center gap-2 rounded-lg px-3 py-2 transition-colors ${WINDOW_CONTROL_HOVER_CLASS}`}
                  style={{
                    ...WINDOW_CONTROL_HOVER_STYLE,
                    color: 'var(--text-secondary)',
                  }}
                  whileTap={noAnimation ? undefined : { scale: 0.9 }}
                  title={t.tapp.windowScheme}
                >
                  <FaTh className="h-4 w-4" />
                  <span className="text-xs font-medium">{t.tapp.scheme}</span>
                </motion.button>

                <AnimatePresence>
                  {showSchemeMenu && (
                    <motion.div
                      className="absolute top-full left-0 z-1001 mt-2 w-56 overflow-hidden rounded-xl glass-surface glass-90"
                      style={{
                        border: '1px solid var(--border-color)',
                        boxShadow: '0 10px 25px -5px rgba(0, 0, 0, 0.15)',
                      }}
                      initial={{ opacity: 0, y: -8, scale: 0.95 }}
                      animate={{ opacity: 1, y: 0, scale: 1 }}
                      exit={{ opacity: 0, y: -8, scale: 0.95 }}
                      transition={{ duration: 0.15 }}
                    >
                      {windows.length > 0 && (
                        <motion.button
                          onClick={saveCurrentScheme}
                          disabled={isSaving}
                          className={`flex w-full items-center gap-3 px-4 py-3 text-sm transition-colors disabled:opacity-50 ${
                            isSaving
                              ? 'bg-transparent'
                              : WINDOW_CONTROL_HOVER_CLASS
                          }`}
                          style={{
                            ...WINDOW_CONTROL_HOVER_STYLE,
                            color: 'var(--text-primary)',
                          }}
                        >
                          {isSaving ? (
                            <Spinner size="sm" color="primary" />
                          ) : (
                            <FaSave
                              className="h-4 w-4"
                              style={{ color: 'var(--color-primary)' }}
                            />
                          )}
                          <span>
                            {isSaving
                              ? t.tapp.saving
                              : t.tapp.saveCurrentScheme}
                          </span>
                        </motion.button>
                      )}

                      {windows.length > 0 && savedSchemes.length > 0 && (
                        <div
                          className="mx-3 my-1 h-px"
                          style={{ backgroundColor: 'var(--border-color)' }}
                        />
                      )}

                      {savedSchemes.length > 0 ? (
                        <div className="max-h-48 overflow-y-auto py-1">
                          {savedSchemes.map((scheme) => (
                            <div
                              key={scheme.id}
                              className="group flex items-center justify-between px-4 py-2.5 transition-colors hover:bg-[var(--bg-hover)]"
                            >
                              <motion.button
                                onClick={() => loadScheme(scheme)}
                                className="min-w-0 flex-1 truncate text-left text-sm"
                                style={{ color: 'var(--text-primary)' }}
                                whileTap={{ scale: 0.98 }}
                              >
                                <span className="block truncate">
                                  {scheme.name}
                                </span>
                                <span
                                  className="text-xs"
                                  style={{ color: 'var(--text-muted)' }}
                                >
                                  {t.tapp.windowCount.replace(
                                    '{count}',
                                    String(scheme.windows.length),
                                  )}
                                </span>
                              </motion.button>
                              <motion.button
                                onClick={(e: React.MouseEvent) => {
                                  e.stopPropagation()
                                  deleteScheme(scheme.id)
                                }}
                                className={`rounded p-1.5 opacity-0 transition-all group-hover:opacity-100 ${WINDOW_CONTROL_DANGER_HOVER_CLASS}`}
                                style={WINDOW_CONTROL_DANGER_HOVER_STYLE}
                                whileTap={{ scale: 0.9 }}
                                title={t.tapp.deleteScheme}
                              >
                                <FaTrash className="h-3.5 w-3.5" />
                              </motion.button>
                            </div>
                          ))}
                        </div>
                      ) : windows.length === 0 ? (
                        <div
                          className="px-4 py-5 text-center text-sm"
                          style={{ color: 'var(--text-muted)' }}
                        >
                          {t.tapp.noSavedSchemes}
                        </div>
                      ) : null}
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </>
          )}

          <div
            className="h-6 w-px"
            style={{ backgroundColor: 'var(--border-color)' }}
          />

          <span
            className="px-2 py-1 text-sm font-medium"
            style={{ color: 'var(--text-muted)' }}
            title={t.tapp.windowCount.replace(
              '{count}',
              String(windows.length),
            )}
          >
            {windows.length}/{MAX_WINDOWS}
          </span>
        </div>
      </div>

      {/* 窗口桌面（底部为 Dock 留白） */}
      <div
        ref={containerRef}
        className="tapp-multi-desktop absolute inset-0 overflow-hidden"
      >
        <AnimatePresence>
          {windows.map((window) => (
            <TappWindowComponent
              key={window.windowId}
              window={window}
              isActive={activeWindowId === window.windowId}
              onClose={closeWindow}
              onMinimize={minimizeWindow}
              onFocus={focusWindow}
              onMove={moveWindow}
              onResize={resizeWindow}
              containerBounds={containerBounds}
            />
          ))}
        </AnimatePresence>
      </div>

      {/* 应用启动窗：居中窗口，无背景遮罩 */}
      <AnimatePresence>
        {showLaunchpad && (
          <div className="tapp-launchpad" aria-hidden={false}>
            <motion.div
              ref={launchpadStageRef}
              className="tapp-launchpad-stage glass"
              role="dialog"
              aria-modal="false"
              aria-label={t.tapp.dockAppPanel}
              initial={
                noAnimation ? false : { opacity: 0, scale: 0.96, y: 10 }
              }
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={
                noAnimation
                  ? undefined
                  : { opacity: 0, scale: 0.97, y: 8 }
              }
              transition={
                noAnimation
                  ? { duration: 0 }
                  : { duration: 0.2, ease: [0.22, 1, 0.36, 1] }
              }
            >
              <div className="tapp-launchpad-toolbar">
                <label className="tapp-launchpad-search">
                  <LuSearch
                    className="tapp-launchpad-search-icon"
                    aria-hidden
                  />
                  <input
                    ref={launchpadSearchRef}
                    type="search"
                    className="tapp-launchpad-search-input"
                    value={launchpadQuery}
                    onChange={(e) => setLaunchpadQuery(e.target.value)}
                    placeholder={t.tapp.dockAppSearch}
                    aria-label={t.tapp.dockAppSearch}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  {launchpadQuery.length > 0 && (
                    <button
                      type="button"
                      className="tapp-launchpad-search-clear"
                      onClick={() => {
                        setLaunchpadQuery('')
                        launchpadSearchRef.current?.focus()
                      }}
                      aria-label={t.common.close}
                    >
                      <LuX className="tapp-launchpad-search-clear-icon" />
                    </button>
                  )}
                </label>

                <div
                  className="tapp-launchpad-cats"
                  role="tablist"
                  aria-label={t.tapp.categoryFilter}
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={launchpadCategory === 'all'}
                    className={`tapp-launchpad-cat${launchpadCategory === 'all' ? ' is-active' : ''}`}
                    onClick={() => setLaunchpadCategory('all')}
                  >
                    {t.tapp.allApps}
                  </button>
                  {launchpadCategories.map((cat) => (
                    <button
                      key={cat}
                      type="button"
                      role="tab"
                      aria-selected={launchpadCategory === cat}
                      className={`tapp-launchpad-cat${launchpadCategory === cat ? ' is-active' : ''}`}
                      onClick={() => setLaunchpadCategory(cat)}
                    >
                      {launchpadCategoryLabel(cat)}
                    </button>
                  ))}
                </div>
              </div>

              {launchpadEntries.length === 0 ? (
                <div className="tapp-launchpad-empty">
                  {t.tapp.noAvailableApps}
                </div>
              ) : (
                <>
                  <div
                    className="tapp-launchpad-grid"
                    style={
                      {
                        '--tapp-lp-cols': LAUNCHPAD_COLS,
                        '--tapp-lp-rows': LAUNCHPAD_ROWS,
                      } as React.CSSProperties
                    }
                  >
                    {launchpadPageItems.map((entry) => {
                      if (entry.kind === 'store') {
                        const storeOpen =
                          (openCountByTappId.get(HOST_PANEL_STORE_ID) ?? 0) >
                          0
                        const storeTitle = t.tapp.storeTitle
                        const storeLabel = `${storeTitle}${dockAtMax && !storeOpen ? ` · ${t.tapp.dockAtMax}` : ''}`
                        return (
                          <button
                            key="host-store"
                            type="button"
                            className={`tapp-launchpad-item${storeOpen ? ' is-open' : ''}${dockAtMax && !storeOpen ? ' is-disabled' : ''}`}
                            onClick={(e) =>
                              activateFromDock(
                                HOST_PANEL_STORE_ID,
                                e.altKey || e.metaKey || e.ctrlKey,
                              )
                            }
                            aria-label={storeLabel}
                          >
                            <span className="tapp-launchpad-icon tapp-launchpad-icon--store">
                              <TappIcon
                                icon={TAPP_ICON_TOKENS.store}
                                name={storeTitle}
                                sizeClass="w-full h-full"
                                className="tapp-multi-dock-store-glyph"
                              />
                            </span>
                            <span className="tapp-launchpad-name">
                              {storeTitle}
                            </span>
                            <span
                              className="tapp-launchpad-dot"
                              style={{
                                opacity: storeOpen ? 1 : 0,
                                background: 'var(--color-primary, #6366f1)',
                              }}
                              aria-hidden
                            />
                          </button>
                        )
                      }

                      const { tapp } = entry
                      const style = getTappIconStyle(tapp.manifest)
                      const text = resolveManifestText(tapp.manifest, locale)
                      const openCount = openCountByTappId.get(tapp.id) ?? 0
                      const isOpen = openCount > 0
                      const accent = getTappIconAccentColor({
                        icon: tapp.manifest.icon,
                        iconSvg: tapp.manifest.iconSvg,
                        themeColor: tapp.manifest.themeColor,
                        category: tapp.manifest.category,
                        id: tapp.id,
                        permissions: tapp.manifest.permissions,
                      })
                      return (
                        <button
                          key={tapp.id}
                          type="button"
                          className={`tapp-launchpad-item${isOpen ? ' is-open' : ''}${dockAtMax && !isOpen ? ' is-disabled' : ''}`}
                          onClick={(e) =>
                            activateFromDock(
                              tapp.id,
                              e.altKey || e.metaKey || e.ctrlKey,
                            )
                          }
                          aria-label={text.name}
                        >
                          <span className="tapp-launchpad-icon">
                            <TappIconBadge
                              icon={tapp.manifest.icon}
                              iconSvg={tapp.manifest.iconSvg}
                              name={text.name}
                              id={tapp.manifest.id || tapp.id}
                              themeColor={tapp.manifest.themeColor}
                              category={tapp.manifest.category}
                              permissions={tapp.manifest.permissions}
                              iconStyle={style}
                              shellClassName="tapp-launchpad-badge-shell h-full w-full"
                              glyphSizeClass="w-[52%] h-[52%]"
                              glyphTextClass="text-[0.85em]"
                            />
                          </span>
                          <span className="tapp-launchpad-name">
                            {text.name}
                          </span>
                          <span
                            className="tapp-launchpad-dot"
                            style={{
                              opacity: isOpen ? 1 : 0,
                              background: accent,
                            }}
                            aria-hidden
                          />
                        </button>
                      )
                    })}
                  </div>

                  {launchpadPageCount > 1 && (
                    <div
                      className="tapp-launchpad-pages"
                      role="tablist"
                      aria-label={t.tapp.dockAppPanel}
                    >
                      {Array.from({ length: launchpadPageCount }, (_, i) => (
                        <button
                          key={i}
                          type="button"
                          role="tab"
                          aria-selected={i === launchpadPage}
                          className={`tapp-launchpad-page-dot${i === launchpadPage ? ' is-active' : ''}`}
                          onClick={() => setLaunchpadPage(i)}
                          aria-label={`${i + 1} / ${launchpadPageCount}`}
                        />
                      ))}
                    </div>
                  )}
                </>
              )}
            </motion.div>
          </div>
        )}
      </AnimatePresence>

      {/* macOS 风格底部 Dock：应用入口(左) + 最多 18 快捷应用 */}
      <nav
        className="tapp-multi-dock"
        aria-label={t.tapp.dockLabel}
        data-at-max={dockAtMax ? 'true' : undefined}
      >
        <div className="tapp-multi-dock-inner glass">
          {/* 启动台入口：最左，名称「应用」 */}
          <motion.button
            type="button"
            className={`tapp-multi-dock-item tapp-multi-dock-more${showLaunchpad ? ' is-open' : ''}`}
            onClick={() =>
              showLaunchpad ? closeLaunchpad() : openLaunchpad()
            }
            aria-label={t.tapp.dockAppPanel}
            aria-expanded={showLaunchpad}
            aria-haspopup="dialog"
            whileHover={noAnimation ? undefined : { y: -5, scale: 1.1 }}
            whileTap={noAnimation ? undefined : { scale: 0.92 }}
          >
            <span className="tapp-multi-dock-label" aria-hidden>
              {t.tapp.dockAppPanel}
            </span>
            {/* 启动台风格：顶行胶囊 + 下两行彩格 */}
            <span className="tapp-multi-dock-apps-shell" aria-hidden>
              <span className="tapp-multi-dock-apps-grid">
                <span className="tapp-multi-dock-apps-search">
                  <span className="tapp-multi-dock-apps-search-dot" />
                </span>
                <i />
                <i />
                <i />
                <i />
                <i />
                <i />
              </span>
            </span>
          </motion.button>

          {/* 快捷应用槽（最多 18） */}
          {dockVisibleApps.length > 0 && (
            <div className="tapp-multi-dock-sep" aria-hidden />
          )}
          {dockVisibleApps.map((tapp) => renderDockAppItem(tapp))}
        </div>
      </nav>
    </div>
  )
}

export default TappWindowManager
