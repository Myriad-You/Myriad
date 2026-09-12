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
import { API_URL as CONFIG_API_URL } from '../../config'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { isPageVisible, startPage } from '../../hooks/animation'
import { isExlight, useAnimationLevel } from '../../hooks/useAnimationLevel'
import { getCSRFToken } from '../../utils/csrf'
import { getUIConfigDeduped } from '../../utils/requestDedup'
import { showError } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
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
import { tappHasPage } from '../utils/manifestLayers'
import { resolveManifestText } from '../utils/manifestLocale'
import {
  resolveTappCategory,
  TAPP_CATEGORIES,
  TAPP_CATEGORY_I18N_KEYS,
} from '../utils/tappCategories'
import { getTappIconAccentColor, getTappIconStyle } from '../utils/tappColors'
import { TappIcon } from './TappIcon'
import { TappIconBadge } from './TappIconBadge'
import { TappStore } from './TappStore'
import './TappWindowManager.css'

const API_URL = CONFIG_API_URL

export type TappWindowKind = 'tapp' | 'host'

export interface TappWindow {
  windowId: string
  tappId: string
  kind: TappWindowKind
  tapp: TappInstance | null
  code: TappCodeStructure | null
  loading: boolean
  error: string | null
  position: { x: number; y: number }
  size: { width: number; height: number }
  isMaximized: boolean
  isMinimized: boolean
  zIndex: number
}

const HOST_STORE_WINDOW_SIZE = { width: 960, height: 720 }

export interface TappWindowManagerProps {
  initialTappId?: string
  onBack?: () => void
}

const MAX_WINDOWS = 5

const MAX_DOCK_APPS = 18

const LAUNCHPAD_COLS = 7
const LAUNCHPAD_ROWS = 3
const LAUNCHPAD_PAGE_SIZE = LAUNCHPAD_COLS * LAUNCHPAD_ROWS

const DEFAULT_WINDOW_SIZE = { width: 400, height: 600 }

type LaunchpadEntry = { kind: 'store' } | { kind: 'app'; tapp: TappInstance }

interface WindowSchemeItem {
  tappId: string
  position: { x: number; y: number }
  size: { width: number; height: number }
}

interface WindowScheme {
  id: string
  name: string
  windows: WindowSchemeItem[]
  createdAt: number
}

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

function generateWindowId(): string {
  return `window-${Date.now()}-${Math.random().toString(36).slice(2, 11)}`
}

function getInitialPosition(windowCount: number): { x: number; y: number } {
  const offset = windowCount * 30
  return {
    x: 100 + offset,
    y: 100 + offset,
  }
}

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

    const currentPositionRef = useRef({
      x: window.position.x,
      y: window.position.y,
    })
    const currentSizeRef = useRef({
      width: window.size.width,
      height: window.size.height,
    })

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

    const iconStyle = useMemo(() => {
      if (isStoreHostPanel(window.tappId)) return null
      return window.tapp ? getTappIconStyle(window.tapp.manifest) : null
    }, [window.tapp, window.tappId])

    const handleDragStart = useCallback(
      (e: React.MouseEvent | React.TouchEvent) => {
        e.preventDefault()
        e.stopPropagation()
        setIsDragging(true)
        const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
        const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY
        dragStartRef.current = { x: clientX, y: clientY }
        positionStartRef.current = { ...currentPositionRef.current }
        onFocus(window.windowId)
      },
      [window.windowId, onFocus],
    )

    const handleResizeStart = useCallback(
      (e: React.MouseEvent | React.TouchEvent, direction: string) => {
        e.preventDefault()
        e.stopPropagation()
        setIsResizing(true)
        setResizeDirection(direction)
        const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
        const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY
        dragStartRef.current = { x: clientX, y: clientY }
        positionStartRef.current = { ...currentPositionRef.current }
        sizeStartRef.current = { ...currentSizeRef.current }
        onFocus(window.windowId)
      },
      [window.windowId, onFocus],
    )

    useEffect(() => {
      if (!isDragging && !isResizing) return

      if (!isPageVisible()) return

      let rafId: number | null = null
      let lastX = dragStartRef.current.x
      let lastY = dragStartRef.current.y

      const handleMove = (e: MouseEvent | TouchEvent) => {
        const clientX =
          'touches' in e ? (e.touches[0]?.clientX ?? lastX) : e.clientX
        const clientY =
          'touches' in e ? (e.touches[0]?.clientY ?? lastY) : e.clientY

        if (clientX === lastX && clientY === lastY) return
        lastX = clientX
        lastY = clientY

        if (rafId) cancelAnimationFrame(rafId)

        rafId = requestAnimationFrame(() => {
          if (!windowRef.current) return

          const deltaX = clientX - dragStartRef.current.x
          const deltaY = clientY - dragStartRef.current.y

          if (isDragging) {
            let newX = positionStartRef.current.x + deltaX
            let newY = positionStartRef.current.y + deltaY

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

            windowRef.current.style.transform = `translate3d(${newX}px, ${newY}px, 0)`
            currentPositionRef.current = { x: newX, y: newY }
          } else if (isResizing && resizeDirection) {
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

            newWidth = Math.min(newWidth, containerBounds.width - newX)
            newHeight = Math.min(newHeight, containerBounds.height - newY)

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

      document.addEventListener('mousemove', handleMove, { passive: true })
      document.addEventListener('mouseup', handleEnd)
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
    }, [isDragging, isResizing, resizeDirection, containerBounds])

    const windowStyle = useMemo(
      () => ({
        width: window.size.width,
        height: window.size.height,
        zIndex: window.zIndex,
        transform: `translate3d(${window.position.x}px, ${window.position.y}px, 0)`,
        transition: isDragging || isResizing ? 'none' : 'box-shadow 0.15s',
      }),
      [window.position, window.size, window.zIndex, isDragging, isResizing],
    )

    const isInteracting = isDragging || isResizing

    const boxShadowStyle = useMemo(
      () => ({
        boxShadow: isActive
          ? '0 8px 24px rgba(0, 0, 0, 0.2)'
          : '0 4px 12px rgba(0, 0, 0, 0.1)',
        border: '1px solid var(--border-color)',
      }),
      [isActive],
    )

    const headerStyle = useMemo(
      () => ({
        borderBottom: `1px solid ${isStorePanel ? 'var(--surface-border)' : 'var(--border-color)'}`,
        opacity: isActive ? 1 : 0.7,
        transition: 'opacity 0.2s ease',
      }),
      [isActive, isStorePanel],
    )

    const handleWindowClick = useCallback(() => {
      onFocus(window.windowId)
    }, [onFocus, window.windowId])

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

    const stopTitleControlPointer = useCallback(
      (e: React.MouseEvent | React.TouchEvent) => {
        e.stopPropagation()
      },
      [],
    )

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
        <div
          className={`flex items-center justify-between px-3 h-10 shrink-0 select-none rounded-t-xl ${isStorePanel ? 'glass glass-chrome-free' : 'glass-surface glass-80'} ${isDragging ? 'cursor-grabbing' : 'cursor-grab'}`}
          style={headerStyle}
          onMouseDown={handleDragStart}
          onTouchStart={handleDragStart}
        >
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

        <div
          className="flex-1 overflow-hidden relative rounded-b-xl"
          style={{
            backgroundColor: isStorePanel ? 'transparent' : 'var(--bg-primary)',
          }}
        >
          {/* 交互时遮罩，防止 iframe 捕获事件。 */}
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

TappWindowComponent.displayName = 'TappWindowComponent'

export const TappWindowManager: React.FC<TappWindowManagerProps> = ({
  initialTappId,
  onBack,
}) => {
  const { t, locale, format } = useI18n()
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
  const [schemeLoadFailed, setSchemeLoadFailed] = useState(false)

  const resizeTimeoutRef = useRef<number | null>(null)
  const [isSaving, setIsSaving] = useState(false)

  const windowsRef = useRef(windows)
  windowsRef.current = windows
  const activeWindowIdRef = useRef(activeWindowId)
  activeWindowIdRef.current = activeWindowId

  useEffect(() => {
    startPage('tapp-multi')
  }, [])

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

    const timer = setTimeout(() => {
      document.addEventListener('mousedown', handleClickOutside)
    }, 0)

    return () => {
      clearTimeout(timer)
      document.removeEventListener('mousedown', handleClickOutside)
    }
  }, [showSchemeMenu])

  useEffect(() => {
    const loadSchemes = async () => {
      try {
        const data = await getUIConfigDeduped()
        if (data.tapp_window_schemes) {
          const schemes = JSON.parse(data.tapp_window_schemes)
          if (Array.isArray(schemes)) {
            setSavedSchemes(schemes)
            setSchemeLoadFailed(false)
          }
        }
      } catch (e) {
        console.warn('Failed to load window schemes from cloud:', e)
        setSchemeLoadFailed(true)
      }
    }
    loadSchemes()
  }, [])

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

    updateBounds()
    window.addEventListener('resize', debouncedUpdateBounds, { passive: true })
    return () => {
      window.removeEventListener('resize', debouncedUpdateBounds)
      if (resizeTimeoutRef.current) {
        cancelAnimationFrame(resizeTimeoutRef.current)
      }
    }
  }, [])

  const refreshDockApps = useCallback(() => {
    const next = runtime.getAllTapps().filter(
      (item) =>
        tappHasPage(item.manifest) &&
        item.installationStatus !== 'error' &&
        item.status !== 'error',
    )
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
        setWindows((prev) => {
          const remaining = prev.filter((w) => w.tappId !== id)
          setActiveWindowId((cur) => {
            if (cur && remaining.some((w) => w.windowId === cur)) return cur
            if (remaining.length === 0) return null
            return remaining.reduce((a, b) => (a.zIndex >= b.zIndex ? a : b))
              .windowId
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
            modules: resources.modules,
            moduleResolutions: resources.moduleResolutions,
            coreEntry: resources.coreEntry,
            pageEntry: resources.pageEntry,
            pageHtml: resources.html,
            styles: resources.styles,
            pageCSS: resources.css,
            i18n: resources.i18n,
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
            userFacingError(error, t.tapp.loadAppFailed)
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

  useEffect(() => {
    if (initialTappId && windows.length === 0) {
      openTappWindow(initialTappId)
    }
  }, [initialTappId])

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
                    userFacingError(err, t.tapp.loadAppFailed),
                }
              : w,
          ),
        )
      }
    },
    [windows, nextZIndex, runtime, t.tapp.appNotExist, t.tapp.loadAppFailed],
  )

  const closeWindow = useCallback(
    (windowId: string) => {
      setWindows((prev) => {
        const remaining = prev.filter((w) => w.windowId !== windowId)
        if (activeWindowId === windowId && remaining.length > 0) {
          const candidates = remaining.filter((w) => !w.isMinimized)
          const pool = candidates.length > 0 ? candidates : remaining
          const topWindow = pool.reduce((a, b) => (a.zIndex > b.zIndex ? a : b))
          setActiveWindowId(topWindow.windowId)
        } else if (remaining.length === 0) {
          setActiveWindowId(null)
        }
        return remaining
      })
    },
    [activeWindowId],
  )

  // 最小化：隐藏进 Dock，保留 iframe；paused → lifecycle:pause，不销毁。
  const minimizeWindow = useCallback(
    (windowId: string) => {
      setWindows((prev) => {
        const next = prev.map((w) =>
          w.windowId === windowId ? { ...w, isMinimized: true } : w,
        )
        if (activeWindowId === windowId) {
          const visible = next.filter((w) => !w.isMinimized)
          if (visible.length > 0) {
            const top = visible.reduce((a, b) => (a.zIndex > b.zIndex ? a : b))
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

  const moveWindow = useCallback(
    (windowId: string, position: { x: number; y: number }) => {
      setWindows((prev) =>
        prev.map((w) => (w.windowId === windowId ? { ...w, position } : w)),
      )
    },
    [],
  )

  const resizeWindow = useCallback(
    (windowId: string, size: { width: number; height: number }) => {
      setWindows((prev) =>
        prev.map((w) => (w.windowId === windowId ? { ...w, size } : w)),
      )
    },
    [],
  )

  useWindowAgentHandler({
    windowsRef,
    activeWindowIdRef,
    openTappWindow,
    closeWindow,
    focusWindow,
  })

  const saveToCloud = useCallback(async (schemes: WindowScheme[]) => {
    const csrfToken = await getCSRFToken(true)
    if (!csrfToken) {
      throw new Error('csrf token unavailable')
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
      throw new Error(`Failed to save window schemes: HTTP ${response.status}`)
    }
  }, [])

  const saveCurrentScheme = useCallback(async () => {
    if (windows.length === 0 || isSaving) return

    setIsSaving(true)

    const schemeWindows: WindowSchemeItem[] = windows
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
    try {
      await saveToCloud(updatedSchemes)
      setSavedSchemes(updatedSchemes)
      setShowSchemeMenu(false)
    } catch (e) {
      showError(userFacingError(e, t.tapp.schemeSaveFailed))
    } finally {
      setIsSaving(false)
    }
  }, [
    windows,
    savedSchemes,
    isSaving,
    saveToCloud,
    t.tapp.schemeNamePrefix,
    t.tapp.schemeSaveFailed,
  ])

  const loadScheme = useCallback(
    async (scheme: WindowScheme) => {
      setShowSchemeMenu(false)

      await new Promise<void>((resolve) => {
        setWindows([])
        setActiveWindowId(null)
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            resolve()
          })
        })
      })

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

      if (newWindows.length > 0) {
        setWindows(newWindows)
        setActiveWindowId(newWindows.at(-1)!.windowId)
        setNextZIndex(baseZIndex + newWindows.length)
      }

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
                      userFacingError(err, t.tapp.loadAppFailed),
                  }
                : w,
            ),
          )
        }
      }
    },
    [nextZIndex, runtime, t.tapp.appNotExist, t.tapp.loadAppFailed],
  )

  const deleteScheme = useCallback(
    async (schemeId: string) => {
      const updatedSchemes = savedSchemes.filter((s) => s.id !== schemeId)
      try {
        await saveToCloud(updatedSchemes)
        setSavedSchemes(updatedSchemes)
      } catch (e) {
        showError(userFacingError(e, t.tapp.schemeSaveFailed))
      }
    },
    [savedSchemes, saveToCloud, t.tapp.schemeSaveFailed],
  )

  const dockVisibleApps = useMemo(() => {
    const openIds = new Set(
      windows.map((w) => w.tappId).filter((id) => !isHostPanelId(id)),
    )
    const openApps: TappInstance[] = []
    const restApps: TappInstance[] = []
    for (const app of availableTapps) {
      if (openIds.has(app.id)) openApps.push(app)
      else restApps.push(app)
    }
    return [...openApps, ...restApps].slice(0, MAX_DOCK_APPS)
  }, [availableTapps, windows])

  const dockPanelApps = availableTapps

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

  const launchpadCategories = useMemo(() => {
    const present = new Set<TappCategory>()
    for (const app of dockPanelApps) {
      present.add(resolveTappCategory(app.manifest))
    }
    return Iterator.from(TAPP_CATEGORIES)
      .filter((c) => present.has(c))
      .toArray()
  }, [dockPanelApps])

  const launchpadCategoryLabel = useCallback(
    (cat: TappCategory) => {
      const key = TAPP_CATEGORY_I18N_KEYS[cat]
      return (t.tapp as Record<string, string>)[key] ?? cat
    },
    [t.tapp],
  )

  const launchpadEntries = useMemo((): LaunchpadEntry[] => {
    const q = launchpadQuery.trim().toLowerCase()
    const storeTitle = t.tapp.storeTitle
    const entries: LaunchpadEntry[] = []
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

  useEffect(() => {
    setLaunchpadPage(0)
  }, [launchpadQuery, launchpadCategory])

  useEffect(() => {
    setLaunchpadPage((p) => Math.min(p, launchpadPageCount - 1))
  }, [launchpadPageCount])

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

  const activateFromDock = useCallback(
    (tappId: string, forceNew: boolean) => {
      if (!forceNew) {
        const same = windows.filter((w) => w.tappId === tappId)
        if (same.length > 0) {
          const byZ = same.toSorted((a, b) => b.zIndex - a.zIndex)
          const top = byZ[0]
          const minimizedTop = byZ.find((w) => w.isMinimized)
          const target =
            !top.isMinimized && activeWindowId === top.windowId && minimizedTop
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
    <div className="fixed inset-0 z-100 overflow-hidden" data-no-ripple>
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
                                  {format(t.tapp.windowCount, {
                                    count: scheme.windows.length,
                                  })}
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
                      ) : schemeLoadFailed || windows.length === 0 ? (
                        <div
                          className="px-4 py-5 text-center text-sm"
                          style={{ color: 'var(--text-muted)' }}
                        >
                          {schemeLoadFailed
                            ? t.tapp.schemeLoadFailed
                            : t.tapp.noSavedSchemes}
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
            title={format(t.tapp.windowCount, { count: windows.length })}
          >
            {windows.length}/{MAX_WINDOWS}
          </span>
        </div>
      </div>

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

      <AnimatePresence>
        {showLaunchpad && (
          <div className="tapp-launchpad" aria-hidden={false}>
            <motion.div
              ref={launchpadStageRef}
              className="tapp-launchpad-stage glass"
              role="dialog"
              aria-modal="false"
              aria-label={t.tapp.dockAppPanel}
              initial={noAnimation ? false : { opacity: 0, scale: 0.96, y: 10 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={noAnimation ? undefined : { opacity: 0, scale: 0.97, y: 8 }}
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
                          (openCountByTappId.get(HOST_PANEL_STORE_ID) ?? 0) > 0
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

      <nav
        className="tapp-multi-dock"
        aria-label={t.tapp.dockLabel}
        data-at-max={dockAtMax ? 'true' : undefined}
      >
        <div className="tapp-multi-dock-inner glass">
          <motion.button
            type="button"
            className={`tapp-multi-dock-item tapp-multi-dock-more${showLaunchpad ? ' is-open' : ''}`}
            onClick={() => (showLaunchpad ? closeLaunchpad() : openLaunchpad())}
            aria-label={t.tapp.dockAppPanel}
            aria-expanded={showLaunchpad}
            aria-haspopup="dialog"
            whileHover={noAnimation ? undefined : { y: -5, scale: 1.1 }}
            whileTap={noAnimation ? undefined : { scale: 0.92 }}
          >
            <span className="tapp-multi-dock-label" aria-hidden>
              {t.tapp.dockAppPanel}
            </span>
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
