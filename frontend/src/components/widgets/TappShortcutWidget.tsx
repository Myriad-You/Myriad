/**
 * Tapp 快捷方式小组件
 *
 * 在主页放置已安装 Tapp 的快捷入口：
 * - 1x1 仅图标 / 2x1 图标+名称 / 2x2 图标+名称+描述
 * - 编辑模式长按打开设置选择 Tapp
 * - 非编辑模式点击跳转 /tapp/run/:id
 *
 * 交互与设置弹窗模式对齐 SocialNetworkWidget / GamePresenceWidget。
 */

import type { KeyboardEvent as ReactKeyboardEvent } from 'react'
import type {
  RecentTappItem,
  TappListItem,
} from '../../tapp/services/TappLifecycleApi'
import type { WidgetComponentProps } from '../WidgetGrid'
import { FaTimes } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { TappIcon } from '../../tapp/components/TappIcon'
import {
  getRecentTapps,
  listTapps,
} from '../../tapp/services/TappLifecycleApi'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface TappShortcutWidgetConfig {
  tappId?: string
}

interface ResolvedTapp {
  id: string
  name: string
  description?: string
  icon?: string
  iconSvg?: string
}

const DEFAULT_GLOW = '#6366f1'

// ---------------------------------------------------------------------------
// Global settings modal (singleton, same pattern as SocialNetworkWidget)
// ---------------------------------------------------------------------------

interface SettingsModalState {
  isOpen: boolean
  selectedTappId?: string
  anchorRect?: DOMRect
  onSelect?: (tappId: string) => void
}

let globalModalState: SettingsModalState = {
  isOpen: false,
}

const modalListeners = new Set<() => void>()

function openSettingsModal(
  selectedTappId: string | undefined,
  anchorRect: DOMRect,
  onSelect: (tappId: string) => void,
) {
  globalModalState = {
    isOpen: true,
    selectedTappId,
    anchorRect,
    onSelect,
  }
  modalListeners.forEach((l) => l())
}

function closeSettingsModal() {
  globalModalState = { ...globalModalState, isOpen: false }
  modalListeners.forEach((l) => l())
}

function subscribeToModalState(listener: () => void) {
  modalListeners.add(listener)
  return () => {
    modalListeners.delete(listener)
  }
}

/** 最近使用优先排序已安装列表 */
function sortTappsByRecent(
  tapps: TappListItem[],
  recent: RecentTappItem[],
): TappListItem[] {
  if (recent.length === 0) return tapps
  const byId = new Map(tapps.map((t) => [t.id, t]))
  const ordered: TappListItem[] = []
  const seen = new Set<string>()
  for (const r of recent) {
    const item = byId.get(r.id)
    if (item) {
      ordered.push(item)
      seen.add(item.id)
    }
  }
  for (const t of tapps) {
    if (!seen.has(t.id)) ordered.push(t)
  }
  return ordered
}

const TappButton = memo(
  ({
    tapp,
    isSelected,
    onSelect,
  }: {
    tapp: TappListItem
    isSelected: boolean
    onSelect: (tappId: string) => void
  }) => {
    const handleClick = useCallback(() => {
      onSelect(tapp.id)
    }, [onSelect, tapp.id])

    return (
      <button
        type="button"
        onClick={handleClick}
        className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-xl transition-all duration-200 ${
          isSelected
            ? 'bg-black/5 dark:bg-white/10 ring-1 ring-black/10 dark:ring-white/20 hover:bg-black/10 dark:hover:bg-white/15'
            : 'hover:bg-black/5 dark:hover:bg-white/8 hover:shadow-sm active:scale-[0.98]'
        }`}
      >
        <div className="w-8 h-8 rounded-lg flex items-center justify-center shrink-0 bg-black/5 dark:bg-white/10 overflow-hidden">
          <TappIcon
            icon={tapp.icon}
            iconSvg={tapp.iconSvg}
            name={tapp.name}
            sizeClass="w-5 h-5"
            textSizeClass="text-base"
            svgColor="currentColor"
          />
        </div>
        <div className="min-w-0 flex-1 text-left">
          <div className="text-sm font-medium text-gray-800 dark:text-gray-100 truncate">
            {tapp.name}
          </div>
          {tapp.description ? (
            <div className="text-[11px] text-gray-500 dark:text-gray-400 truncate">
              {tapp.description}
            </div>
          ) : null}
        </div>
      </button>
    )
  },
)

TappButton.displayName = 'TappButton'

const GlobalSettingsModal = memo(() => {
  const { t } = useI18n()
  const tw = t.tappShortcut
  const [, forceUpdate] = useState({})
  const modalRef = useRef<HTMLDivElement>(null)
  const [tapps, setTapps] = useState<TappListItem[]>([])
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState(false)

  useEffect(() => subscribeToModalState(() => forceUpdate({})), [])

  const { isOpen, selectedTappId, anchorRect, onSelect } = globalModalState

  // 仅在弹窗打开时请求列表，避免预览/未打开时乱请求
  useEffect(() => {
    if (!isOpen) return
    let cancelled = false
    setLoading(true)
    setLoadError(false)
    ;(async () => {
      try {
        const [list, recent] = await Promise.all([
          listTapps(),
          getRecentTapps(20).catch(() => [] as RecentTappItem[]),
        ])
        if (cancelled) return
        setTapps(sortTappsByRecent(list, recent))
      } catch {
        if (!cancelled) {
          setTapps([])
          setLoadError(true)
        }
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [isOpen])

  const position = useMemo(() => {
    if (!anchorRect) return { top: 0, left: 0 }
    const modalWidth = 300
    const modalHeight = 360
    const padding = 16
    let top = anchorRect.bottom + 8
    let left = anchorRect.left + (anchorRect.width - modalWidth) / 2
    if (left + modalWidth > window.innerWidth - padding) {
      left = window.innerWidth - modalWidth - padding
    }
    if (left < padding) left = padding
    if (top + modalHeight > window.innerHeight - padding) {
      top = anchorRect.top - modalHeight - 8
    }
    if (top < padding) top = padding
    return { top, left }
  }, [anchorRect])

  useEffect(() => {
    if (!isOpen) return
    const handleClickOutside = (e: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(e.target as Node)) {
        closeSettingsModal()
      }
    }
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeSettingsModal()
    }
    const timer = setTimeout(() => {
      document.addEventListener('mousedown', handleClickOutside, {
        passive: true,
      })
      document.addEventListener('keydown', handleKeyDown)
    }, 100)
    return () => {
      clearTimeout(timer)
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [isOpen])

  const handleSelect = useCallback(
    (tappId: string) => {
      onSelect?.(tappId)
      closeSettingsModal()
    },
    [onSelect],
  )

  if (!isOpen) return null

  return createPortal(
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-10000"
      style={{ pointerEvents: 'none' }}
    >
      <motion.div
        ref={modalRef}
        initial={{ opacity: 0, scale: 0.95, y: -5 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        transition={{ duration: 0.15 }}
        className="absolute glass rounded-2xl shadow-2xl overflow-hidden border border-white/20 dark:border-white/10"
        style={{
          top: position.top,
          left: position.left,
          width: 300,
          pointerEvents: 'auto',
        }}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200/50 dark:border-white/10">
          <span className="font-bold text-sm text-gray-800 dark:text-gray-200">
            {tw.selectTapp}
          </span>
          <button
            type="button"
            onClick={closeSettingsModal}
            className="w-6 h-6 flex items-center justify-center rounded-full hover:bg-black/5 dark:hover:bg-white/10 transition-colors"
            title={tw.close}
            aria-label={tw.close}
          >
            <FaTimes size={12} className="text-gray-500 dark:text-gray-400" />
          </button>
        </div>

        <div className="p-3 space-y-1.5 max-h-80 overflow-y-auto">
          {loading ? (
            <div className="py-8 text-center text-sm text-gray-500 dark:text-gray-400">
              {tw.loading}
            </div>
          ) : loadError ? (
            <div className="py-8 text-center text-sm text-gray-500 dark:text-gray-400">
              {tw.loadFailed}
            </div>
          ) : tapps.length === 0 ? (
            <div className="py-8 text-center text-sm text-gray-500 dark:text-gray-400">
              {tw.emptyTapps}
            </div>
          ) : (
            tapps.map((tapp) => (
              <TappButton
                key={tapp.id}
                tapp={tapp}
                isSelected={selectedTappId === tapp.id}
                onSelect={handleSelect}
              />
            ))
          )}
        </div>
      </motion.div>
    </motion.div>,
    document.body,
  )
})

GlobalSettingsModal.displayName = 'TappShortcutSettingsModal'

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

export const TappShortcutWidget = memo(
  ({ config, isEditMode, isPreview, onConfigChange }: WidgetComponentProps) => {
    const { t } = useI18n()
    const tw = t.tappShortcut
    const navigate = useNavigate()
    const anim = useAnimationLevel()
    const { containerRef, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const localRef = useRef<HTMLDivElement | null>(null)
    const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
      null,
    )
    const isLongPressRef = useRef(false)

    const [tappId, setTappId] = useState<string | undefined>(
      config.config?.tappId as string | undefined,
    )
    const [resolved, setResolved] = useState<ResolvedTapp | null>(null)
    const [missing, setMissing] = useState(false)
    const [loading, setLoading] = useState(false)

    // 同步外部 config
    useEffect(() => {
      const next = config.config?.tappId as string | undefined
      setTappId((prev) => (prev === next ? prev : next))
    }, [config.config?.tappId])

    // 解析已配置的 Tapp；preview 不请求
    useEffect(() => {
      if (isPreview) {
        setResolved(null)
        setMissing(false)
        setLoading(false)
        return
      }
      if (!tappId) {
        setResolved(null)
        setMissing(false)
        setLoading(false)
        return
      }

      let cancelled = false
      setLoading(true)
      listTapps()
        .then((list) => {
          if (cancelled) return
          const found = list.find((item) => item.id === tappId)
          if (found) {
            setResolved({
              id: found.id,
              name: found.name,
              description: found.description,
              icon: found.icon,
              iconSvg: found.iconSvg,
            })
            setMissing(false)
          } else {
            setResolved(null)
            setMissing(true)
          }
        })
        .catch(() => {
          if (!cancelled) {
            setResolved(null)
            setMissing(true)
          }
        })
        .finally(() => {
          if (!cancelled) setLoading(false)
        })

      return () => {
        cancelled = true
      }
    }, [tappId, isPreview])

    const persist = useCallback(
      (nextTappId: string) => {
        setTappId(nextTappId)
        const payload = { ...config.config, tappId: nextTappId }
        if (typeof onConfigChange === 'function') {
          onConfigChange(payload)
        } else {
          window.dispatchEvent(
            new CustomEvent('widget-config-update', {
              detail: { widgetId: config.id, config: payload },
            }),
          )
        }
      },
      [config.config, config.id, onConfigChange],
    )

    const openSettings = useCallback(() => {
      if (!localRef.current) return
      openSettingsModal(
        tappId,
        localRef.current.getBoundingClientRect(),
        persist,
      )
    }, [tappId, persist])

    const handlePressStart = useCallback(() => {
      if (!isEditMode) return
      isLongPressRef.current = false
      longPressTimerRef.current = setTimeout(() => {
        isLongPressRef.current = true
        openSettings()
      }, 500)
    }, [isEditMode, openSettings])

    const handlePressEnd = useCallback(() => {
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current)
        longPressTimerRef.current = null
      }
    }, [])

    useEffect(() => {
      return () => {
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current)
        }
      }
    }, [])

    const canLaunch = !isEditMode && !!resolved?.id

    const handleClick = useCallback(() => {
      if (isLongPressRef.current) {
        isLongPressRef.current = false
        return
      }
      if (isEditMode) return
      if (resolved?.id) {
        navigate(`/tapp/run/${resolved.id}`)
      }
    }, [isEditMode, resolved?.id, navigate])

    const handleKeyDown = useCallback(
      (event: ReactKeyboardEvent<HTMLDivElement>) => {
        if (canLaunch && (event.key === 'Enter' || event.key === ' ')) {
          event.preventDefault()
          handleClick()
        }
      },
      [canLaunch, handleClick],
    )

    const mergedRef = useCallback(
      (node: HTMLDivElement | null) => {
        localRef.current = node
        if (typeof containerRef === 'function') {
          containerRef(node)
        }
      },
      [containerRef],
    )

    const isPlaceholder = isPreview || !resolved
    const placeholderLabel = (() => {
      if (isPreview) return tw.previewLabel
      if (missing) return tw.notInstalled
      if (!tappId) {
        return isEditMode ? tw.longPressToEdit : tw.noTappSelected
      }
      if (loading) return tw.loading
      return tw.noTappSelected
    })()

    const content = useMemo(() => {
      // 占位：未配置 / 已卸载 / preview
      if (isPlaceholder) {
        const iconSize =
          config.size === '1x1' ? 'w-8 h-8' : config.size === '2x1' ? 'w-7 h-7' : 'w-10 h-10'
        const textSize =
          config.size === '1x1' ? 'text-2xl' : config.size === '2x1' ? 'text-xl' : 'text-3xl'

        if (config.size === '1x1') {
          return (
            <div className="h-full w-full flex items-center justify-center">
              <div
                className={`${iconSize} rounded-xl bg-black/5 dark:bg-white/10 flex items-center justify-center text-gray-400 dark:text-gray-500`}
                title={placeholderLabel}
              >
                <span className={textSize} aria-hidden>
                  ⚡
                </span>
              </div>
            </div>
          )
        }

        if (config.size === '2x1') {
          return (
            <div className="h-full w-full flex items-center justify-center gap-3 px-3">
              <div
                className={`${iconSize} rounded-xl bg-black/5 dark:bg-white/10 flex items-center justify-center text-gray-400 dark:text-gray-500 shrink-0`}
              >
                <span className={textSize} aria-hidden>
                  ⚡
                </span>
              </div>
              <span
                className="font-medium text-gray-500 dark:text-gray-400 truncate"
                style={{ fontSize: `${14 * fontScale}px` }}
              >
                {placeholderLabel}
              </span>
            </div>
          )
        }

        // 2x2
        return (
          <div className="h-full w-full flex flex-col items-center justify-center gap-2 p-3 text-center">
            <div
              className={`${iconSize} rounded-xl bg-black/5 dark:bg-white/10 flex items-center justify-center text-gray-400 dark:text-gray-500`}
            >
              <span className={textSize} aria-hidden>
                ⚡
              </span>
            </div>
            <span
              className="font-medium text-gray-500 dark:text-gray-400"
              style={{ fontSize: `${13 * fontScale}px` }}
            >
              {placeholderLabel}
            </span>
          </div>
        )
      }

      const name = resolved!.name
      const description = resolved!.description

      // 1x1 — 仅图标
      if (config.size === '1x1') {
        return (
          <div className="h-full w-full flex items-center justify-center">
            <div className="w-10 h-10 rounded-xl flex items-center justify-center overflow-hidden bg-black/5 dark:bg-white/10">
              <TappIcon
                icon={resolved!.icon}
                iconSvg={resolved!.iconSvg}
                name={name}
                sizeClass="w-7 h-7"
                textSizeClass="text-2xl"
                svgColor="currentColor"
              />
            </div>
          </div>
        )
      }

      // 2x1 — 图标 + 名称
      if (config.size === '2x1') {
        return (
          <div className="h-full w-full flex items-center justify-center gap-3 px-3">
            <div className="w-9 h-9 rounded-xl flex items-center justify-center overflow-hidden bg-black/5 dark:bg-white/10 shrink-0">
              <TappIcon
                icon={resolved!.icon}
                iconSvg={resolved!.iconSvg}
                name={name}
                sizeClass="w-6 h-6"
                textSizeClass="text-xl"
                svgColor="currentColor"
              />
            </div>
            <span
              className="font-bold text-gray-800 dark:text-gray-100 truncate"
              style={{ fontSize: `${16 * fontScale}px` }}
            >
              {name}
            </span>
          </div>
        )
      }

      // 2x2 — 图标 + 名称 + 描述
      return (
        <div className="h-full w-full flex flex-col items-center justify-center gap-2 p-3 text-center">
          <div className="w-12 h-12 rounded-xl flex items-center justify-center overflow-hidden bg-black/5 dark:bg-white/10">
            <TappIcon
              icon={resolved!.icon}
              iconSvg={resolved!.iconSvg}
              name={name}
              sizeClass="w-8 h-8"
              textSizeClass="text-3xl"
              svgColor="currentColor"
            />
          </div>
          <div className="w-full min-w-0">
            <div
              className="font-bold text-gray-800 dark:text-gray-100 truncate"
              style={{ fontSize: `${15 * fontScale}px` }}
            >
              {name}
            </div>
            {description ? (
              <div
                className="mt-0.5 text-gray-500 dark:text-gray-400 line-clamp-2"
                style={{ fontSize: `${11 * fontScale}px` }}
              >
                {description}
              </div>
            ) : (
              <div
                className="mt-0.5 text-gray-400 dark:text-gray-500"
                style={{ fontSize: `${11 * fontScale}px` }}
              >
                {tw.clickToOpen}
              </div>
            )}
          </div>
        </div>
      )
    }, [
      isPlaceholder,
      config.size,
      resolved,
      fontScale,
      placeholderLabel,
      tw.clickToOpen,
    ])

    const ariaLabel = resolved
      ? `${resolved.name}: ${tw.clickToOpen}`
      : placeholderLabel

    return (
      <WidgetShell
        as={motion.div}
        containerRef={mergedRef}
        padding={0}
        className={`${canLaunch ? 'cursor-pointer' : ''} ${
          isEditMode ? 'cursor-grab' : ''
        }`}
        background={
          <GlowBackground
            color={DEFAULT_GLOW}
            animLevel={anim.level}
            shouldAnimate={anim.loop}
          />
        }
        rootProps={{
          role: canLaunch ? 'button' : undefined,
          tabIndex: canLaunch ? 0 : undefined,
          'aria-label': ariaLabel,
          onClick: handleClick,
          onKeyDown: handleKeyDown,
          onMouseDown: handlePressStart,
          onMouseUp: handlePressEnd,
          onMouseLeave: handlePressEnd,
          onTouchStart: handlePressStart,
          onTouchEnd: handlePressEnd,
          onTouchCancel: handlePressEnd,
          whileHover: canLaunch ? { filter: 'brightness(1.03)' } : undefined,
          whileTap: canLaunch ? { scale: 0.98 } : undefined,
        }}
      >
        {content}
        {isEditMode && (
          <motion.div
            className="absolute top-1.5 right-1.5 w-5 h-5 rounded-md flex items-center justify-center bg-black/15 dark:bg-white/15 backdrop-blur-sm pointer-events-none"
            initial={{ opacity: 0, scale: 0.8 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ type: 'spring', stiffness: 400, damping: 20 }}
            title={tw.longPressToEdit}
          >
            <svg
              className="w-3 h-3 text-gray-700 dark:text-gray-200"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
              />
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
              />
            </svg>
          </motion.div>
        )}
      </WidgetShell>
    )
  },
)

TappShortcutWidget.displayName = 'TappShortcutWidget'

export { GlobalSettingsModal as TappShortcutSettingsModal }
