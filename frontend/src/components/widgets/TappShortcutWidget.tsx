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

import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from 'react'
import type {
  RecentTappItem,
  TappListItem,
} from '../../tapp/services/TappLifecycleApi'
import type { TappManifestLocales } from '../../tapp/types'
import type { WidgetComponentProps } from '../WidgetGrid'
import { FaTh, FaTimes } from '@lib/icons'
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
import { hasStandaloneTappIcon, TappIcon } from '../../tapp/components/TappIcon'
import { TappIconBadge } from '../../tapp/components/TappIconBadge'
import {
  getRecentTapps,
  listTappDetails,
  listTapps,
} from '../../tapp/services/TappLifecycleApi'
import { resolveManifestText } from '../../tapp/utils/manifestLocale'
import { getTappIconStyle } from '../../tapp/utils/tappColors'
import { tappRunPath } from '../../tapp/utils/tappPaths'
import { userFacingError } from '../../utils/userFacingError'
import { Spinner } from '../Spinner'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetLongPressHint } from './shared/WidgetLongPressHint'
import { WidgetShell } from './shared/WidgetShell'

// Types

interface ResolvedTapp {
  id: string
  name: string
  description?: string
  icon?: string
  iconSvg?: string
  /** manifest 主题色（与 Tapp 页实际渲染一致） */
  themeColor?: string
  /** manifest.locales 透传，渲染时按当前语言解析 */
  locales?: TappManifestLocales
}

const DEFAULT_GLOW = '#6366f1'

// Per-Tapp accent color —— 与 Tapp 页实际渲染保持一致

/** 由 Tapp id 稳定散列出一个色相；仅用于 manifest 未提供主题色时的兜底 */
function accentHue(seed: string): number {
  let h = 0
  for (let i = 0; i < seed.length; i++) {
    h = (Math.imul(h, 31) + seed.charCodeAt(i)) >>> 0
  }
  return h % 360
}

/** 给任意 CSS 颜色（hex / hsl）叠加透明度，用于渐变/描边/投影 */
function withAlpha(color: string, alpha: number): string {
  const c = color.trim()
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(c)
  if (hex) {
    let h = hex[1]
    if (h.length === 3) {
      h = h
        .split('')
        .map((ch) => ch + ch)
        .join('')
    }
    const r = Number.parseInt(h.slice(0, 2), 16)
    const g = Number.parseInt(h.slice(2, 4), 16)
    const b = Number.parseInt(h.slice(4, 6), 16)
    return `rgba(${r}, ${g}, ${b}, ${alpha})`
  }
  const hsl = /^hsl\(([^)]+)\)$/i.exec(c)
  if (hsl) return `hsl(${hsl[1]} / ${alpha})`
  return c
}

/** App 图标风格的实底样式：主题色实底 + 顶部高光 + 柔和同色投影，衬托白色图标 */
function appIconFill(color: string) {
  return {
    background: `linear-gradient(145deg, ${withAlpha(color, 1)} 0%, ${withAlpha(color, 0.78)} 100%)`,
    boxShadow: `inset 0 1px 1px rgba(255, 255, 255, 0.3), inset 0 -1px 2px rgba(0, 0, 0, 0.12), 0 6px 14px -6px ${withAlpha(color, 0.65)}`,
  }
}

/**
 * App 图标风格的图标底座（2x1 / 2x2 用）。
 * 自有整图图标铺满圆角区，不套主题色壳。
 */
const IconTile = memo(
  ({
    color,
    tileClass,
    standalone = false,
    children,
  }: {
    color: string | null
    tileClass: string
    standalone?: boolean
    children: ReactNode
  }) => {
    if (standalone) {
      return (
        <div
          className={`${tileClass} rounded-lg overflow-hidden shrink-0 tapp-icon-badge--standalone`}
        >
          {children}
        </div>
      )
    }
    return (
      <div
        className={`${tileClass} rounded-lg flex items-center justify-center overflow-hidden shrink-0 ${
          color === null ? 'bg-black/5 dark:bg-white/10' : ''
        }`}
        style={color === null ? undefined : appIconFill(color)}
      >
        {children}
      </div>
    )
  },
)

IconTile.displayName = 'TappShortcutIconTile'

// Global settings modal (singleton, same pattern as SocialNetworkWidget)

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
    const { locale } = useI18n()
    const { name: tappName, description: tappDescription } =
      resolveManifestText(tapp, locale)
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
        <TappIconBadge
          icon={tapp.icon}
          iconSvg={tapp.iconSvg}
          name={tappName}
          id={tapp.id}
          iconStyle={getTappIconStyle({
            icon: tapp.icon,
            iconSvg: tapp.iconSvg,
            id: tapp.id,
          })}
          shellClassName="w-8 h-8 rounded-lg"
          glyphSizeClass="w-5 h-5"
          glyphTextClass="text-base"
        />
        <div className="min-w-0 flex-1 text-left">
          <div className="text-sm font-medium text-gray-800 dark:text-gray-100 truncate">
            {tappName}
          </div>
          {tappDescription ? (
            <div className="text-[11px] text-gray-500 dark:text-gray-400 truncate">
              {tappDescription}
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
  const [loadError, setLoadError] = useState<string | null>(null)

  useEffect(() => subscribeToModalState(() => forceUpdate({})), [])

  const { isOpen, selectedTappId, anchorRect, onSelect } = globalModalState

  // 仅在弹窗打开时请求列表，避免预览/未打开时乱请求
  useEffect(() => {
    if (!isOpen) return
    let cancelled = false
    setLoading(true)
    setLoadError(null)
    ;(async () => {
      try {
        const [list, recent] = await Promise.all([
          listTapps(),
          getRecentTapps(20).catch(() => [] as RecentTappItem[]),
        ])
        if (cancelled) return
        setTapps(sortTappsByRecent(list, recent))
      } catch (error) {
        if (!cancelled) {
          setTapps([])
          setLoadError(userFacingError(error, tw.loadFailed))
        }
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [isOpen, tw.loadFailed])

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
              {loadError}
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

// Widget

export const TappShortcutWidget = memo(
  ({ config, isEditMode, isPreview, onConfigChange }: WidgetComponentProps) => {
    const { t, locale } = useI18n()
    const tw = t.tappShortcut
    const navigate = useNavigate()
    const anim = useAnimationLevel()
    const { containerRef, fontScale, scale } = useWidgetSize(
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
      // list：拿 iconSvg（详情接口不含）；details：拿 manifest 主题色
      Promise.all([
        listTapps(),
        listTappDetails().catch(() => []),
      ])
        .then(([list, details]) => {
          if (cancelled) return
          const found = list.find((item) => item.id === tappId)
          if (found) {
            const detail = details.find((d) => d.id === tappId)
            setResolved({
              id: found.id,
              name: found.name,
              description: found.description,
              icon: found.icon,
              iconSvg: found.iconSvg,
              themeColor: detail?.theme_color,
              locales: found.locales,
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

    // 强调色（光晕 + 图标底座）：优先用 Tapp manifest 主题色，与 Tapp 页
    // 实际渲染一致；manifest 未提供时才退回按 id 散列的稳定色。
    const tileColor = useMemo(() => {
      if (!resolved?.id) return null
      const theme = resolved.themeColor?.trim()
      if (theme) return theme
      return `hsl(${accentHue(resolved.id)} 72% 58%)`
    }, [resolved?.id, resolved?.themeColor])
    const glowColor = tileColor ?? DEFAULT_GLOW

    const handleClick = useCallback(() => {
      if (isLongPressRef.current) {
        isLongPressRef.current = false
        return
      }
      if (isEditMode) return
      if (resolved?.id) {
        navigate(tappRunPath(resolved.id))
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
        // 编辑模式下未配置：呈现「添加」态（虚线 + 加号），其余为中性玻璃底
        const isAddState = isEditMode && !missing && !tappId && !isPreview
        const tileClass =
          config.size === '1x1'
            ? 'w-12 h-12'
            : config.size === '2x1'
              ? 'w-10 h-10'
              : 'w-14 h-14'
        // 编辑未配置：虚线 +；加载中：Spinner；其余默认空状态：Tapp 网格图标提示
        const spinnerPx =
          (config.size === '2x2' ? 22 : config.size === '1x1' ? 20 : 18) *
          fontScale
        const emptyIconPx =
          (config.size === '2x2' ? 22 : config.size === '1x1' ? 20 : 18) *
          fontScale
        const glyph = isAddState ? (
          <span
            className="text-gray-400 dark:text-gray-500"
            aria-hidden
            style={{
              fontSize: `${(config.size === '2x2' ? 26 : config.size === '1x1' ? 24 : 20) * fontScale}px`,
              lineHeight: 1,
            }}
          >
            +
          </span>
        ) : loading ? (
          <Spinner size={spinnerPx} color="primary" />
        ) : (
          <FaTh
            aria-hidden
            className="text-gray-400/80 dark:text-gray-500/80"
            style={{ width: emptyIconPx, height: emptyIconPx }}
          />
        )
        const tile = (
          <div
            className={`${tileClass} rounded-lg flex items-center justify-center shrink-0 ${
              isAddState
                ? 'border-2 border-dashed border-black/15 dark:border-white/20'
                : 'bg-black/5 dark:bg-white/10'
            }`}
            title={placeholderLabel}
          >
            {glyph}
          </div>
        )

        if (config.size === '1x1') {
          return (
            <div className="h-full w-full flex items-center justify-center">
              {tile}
            </div>
          )
        }

        if (config.size === '2x1') {
          return (
            <div className="h-full w-full flex items-center justify-center gap-2.5">
              {tile}
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
          <div className="h-full w-full flex flex-col items-center justify-center gap-2.5 text-center">
            {tile}
            <span
              className="font-medium text-gray-500 dark:text-gray-400"
              style={{ fontSize: `${13 * fontScale}px` }}
            >
              {placeholderLabel}
            </span>
          </div>
        )
      }

      const { name, description } = resolveManifestText(resolved!, locale)

      // 1x1 — 原生玻璃底 + 主题色图标（无底座），光晕同为主题色
      if (config.size === '1x1') {
        return (
          <div
            className="h-full w-full flex items-center justify-center"
            style={{ color: tileColor ?? undefined }}
          >
            <TappIcon
              icon={resolved!.icon}
              iconSvg={resolved!.iconSvg}
              name={name}
              sizeClass="w-8 h-8"
              textSizeClass="text-3xl"
              svgColor={tileColor ?? DEFAULT_GLOW}
            />
          </div>
        )
      }

      const standaloneIcon = hasStandaloneTappIcon(resolved!)

      // 2x1 — 图标 + 名称
      if (config.size === '2x1') {
        return (
          <div className="h-full w-full flex items-center justify-center gap-3">
            <IconTile
              color={tileColor}
              tileClass="w-10 h-10"
              standalone={standaloneIcon}
            >
              <TappIcon
                icon={resolved!.icon}
                iconSvg={resolved!.iconSvg}
                name={name}
                sizeClass={standaloneIcon ? 'w-full h-full' : 'w-6 h-6'}
                textSizeClass="text-xl"
                svgColor="#fff"
                className={
                  standaloneIcon ? 'tapp-icon-badge__media' : undefined
                }
              />
            </IconTile>
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
        <div className="h-full w-full flex flex-col items-center justify-center gap-2.5 text-center">
          <IconTile
            color={tileColor}
            tileClass="w-14 h-14"
            standalone={standaloneIcon}
          >
            <TappIcon
              icon={resolved!.icon}
              iconSvg={resolved!.iconSvg}
              name={name}
              sizeClass={standaloneIcon ? 'w-full h-full' : 'w-8 h-8'}
              textSizeClass="text-3xl"
              svgColor="#fff"
              className={
                standaloneIcon ? 'tapp-icon-badge__media' : undefined
              }
            />
          </IconTile>
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
                className="mt-1 inline-flex items-center gap-0.5 font-medium"
                style={{
                  fontSize: `${10.5 * fontScale}px`,
                  color: glowColor,
                }}
              >
                {tw.clickToOpen}
                <span aria-hidden>›</span>
              </div>
            )}
          </div>
        </div>
      )
    }, [
      isPlaceholder,
      isEditMode,
      isPreview,
      missing,
      tappId,
      loading,
      config.size,
      resolved,
      locale,
      tileColor,
      glowColor,
      fontScale,
      placeholderLabel,
      tw.clickToOpen,
    ])

    const ariaLabel = resolved
      ? `${resolveManifestText(resolved, locale).name}: ${tw.clickToOpen}`
      : placeholderLabel

    return (
      <WidgetShell
        as={motion.div}
        containerRef={mergedRef}
        scale={scale}
        padding={config.size === '1x1' ? 8 : 12}
        className={`select-none ${canLaunch ? 'cursor-pointer' : ''} ${
          isEditMode ? 'cursor-grab' : ''
        }`}
        background={
          <GlowBackground
            color={glowColor}
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
        <WidgetLongPressHint
          visible={isEditMode}
          title={tw.longPressToEdit}
        />
      </WidgetShell>
    )
  },
)

TappShortcutWidget.displayName = 'TappShortcutWidget'

export { GlobalSettingsModal as TappShortcutSettingsModal }
