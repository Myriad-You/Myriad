import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from 'react'
import type {
  RecentTappItem,
  TappListItem,
} from '../../tapp/services/TappLifecycleApi'
import type { TappManifestLocales } from '../../tapp/types'
import type { WidgetComponentProps } from '../widgetGridTypes'
import { FaTh } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { armWidgetSettingsHost } from '../../lib/widgetSettingsHost'
import { hasStandaloneTappIcon, TappIcon } from '../../tapp/components/TappIcon'
import { TappIconBadge } from '../../tapp/components/TappIconBadge'
import {
  getRecentTapps,
  listTapps,
} from '../../tapp/services/TappLifecycleApi'
import { resolveManifestText } from '../../tapp/utils/manifestLocale'
import { getTappIconStyle } from '../../tapp/utils/tappColors'
import { tappRunPath } from '../../tapp/utils/tappPaths'
import { userFacingError } from '../../utils/userFacingError'
import { Spinner } from '../Spinner'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetLongPressHint } from './shared/WidgetLongPressHint'
import { WidgetSettingsSection, WidgetSettingsTip } from './shared/WidgetSettingsTip'
import { WidgetShell } from './shared/WidgetShell'

interface ResolvedTapp {
  id: string
  name: string
  description?: string
  icon?: string
  iconSvg?: string
  themeColor?: string
  locales?: TappManifestLocales
}

const DEFAULT_GLOW = '#6366f1'

function accentHue(seed: string): number {
  let h = 0
  for (let i = 0; i < seed.length; i++) {
    h = (Math.imul(h, 31) + seed.charCodeAt(i)) >>> 0
  }
  return h % 360
}

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

function appIconFill(color: string) {
  return {
    background: `linear-gradient(145deg, ${withAlpha(color, 1)} 0%, ${withAlpha(color, 0.78)} 100%)`,
    boxShadow: `inset 0 1px 1px rgba(255, 255, 255, 0.3), inset 0 -1px 2px rgba(0, 0, 0, 0.12), 0 6px 14px -6px ${withAlpha(color, 0.65)}`,
  }
}

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
  armWidgetSettingsHost()
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
  const [tapps, setTapps] = useState<TappListItem[]>([])
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)

  useEffect(() => subscribeToModalState(() => forceUpdate({})), [])

  const { isOpen, selectedTappId, anchorRect, onSelect } = globalModalState

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

  const handleSelect = useCallback(
    (tappId: string) => {
      onSelect?.(tappId)
      closeSettingsModal()
    },
    [onSelect],
  )

  return (
    <WidgetSettingsTip
      open={isOpen}
      anchor={anchorRect ?? null}
      title={t.widgets.tappShortcut}
      width={300}
      height={420}
      onClose={closeSettingsModal}
    >
      <WidgetSettingsSection label={tw.selectTapp}>
        <div className="widget-settings-tip__body space-y-1.5">
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
      </WidgetSettingsSection>
    </WidgetSettingsTip>
  )
})

GlobalSettingsModal.displayName = 'TappShortcutSettingsModal'

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

    useEffect(() => {
      const next = config.config?.tappId as string | undefined
      setTappId((prev) => (prev === next ? prev : next))
    }, [config.config?.tappId])

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
              themeColor: found.themeColor,
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
        onConfigChange?.(payload)
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
      if (isPlaceholder) {
        const isAddState = isEditMode && !missing && !tappId && !isPreview
        const tileClass =
          config.size === '1x1'
            ? 'w-12 h-12'
            : config.size === '2x1'
              ? 'w-10 h-10'
              : 'w-14 h-14'
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
          onClick={openSettings}
        />
      </WidgetShell>
    )
  },
)

TappShortcutWidget.displayName = 'TappShortcutWidget'

export { GlobalSettingsModal as TappShortcutSettingsModal }
