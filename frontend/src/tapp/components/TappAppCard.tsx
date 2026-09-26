import type { CSSProperties, DragEvent, KeyboardEvent, MouseEvent } from 'react'
import type { TappInstance, TappPermission } from '../types'
import type { IconStyle } from '../utils/tappColors'
import {
  FaCog,
  FaCompress,
  FaExclamationTriangle,
  FaExpand,
  FaGripVertical,
  FaLock,
  FaPause,
  FaPlay,
  FaTrash,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {

  forwardRef,

  useEffect,
  useMemo,
  useState,
} from 'react'
import { GlowBackground } from '../../components/widgets/shared/GlowBackground'
import { useI18n } from '../../contexts/I18nContext'
import { useTappStagger } from '../../hooks/animation'
import {
  isExlight,
  isStandardAnimation,
  useAnimationLevel,
} from '../../hooks/useAnimationLevel'
import { PERMISSION_CONFIG } from '../constants/permissions'
import { PERMISSION_LEVELS } from '../runtime/permissionConfig'
import { tappHasPage } from '../utils/manifestLayers'
import { resolveManifestText } from '../utils/manifestLocale'
import {
  resolveTappCategory,
  TAPP_CATEGORY_I18N_KEYS,
} from '../utils/tappCategories'
import { getTappIconStyle as getTappIconStyleFromManifest } from '../utils/tappColors'
import { TappIconBadge } from './TappIconBadge'
import './TappAppCard.css'

export type TappAppCardSize = '1x1' | '2x1'

const SUBTITLE_ROTATE_MS = 3200
const CARD_LAYOUT_STORAGE_KEY = 'tapp.listCardLayout.v2'
const CARD_SIZE_STORAGE_KEY_LEGACY = 'tapp.listCardSizes.v1'

function getTappIconStyle(tapp: TappInstance): IconStyle {
  return getTappIconStyleFromManifest({
    ...tapp.manifest,
    id: tapp.manifest.id || tapp.id,
  })
}

function isCardSize(v: unknown): v is TappAppCardSize {
  return v === '1x1' || v === '2x1'
}

export interface TappAppCardLayoutLocal {
  sizes: Record<string, TappAppCardSize>
  order: string[]
}

function normalizeLocalLayout(parsed: unknown): TappAppCardLayoutLocal {
  if (!parsed || typeof parsed !== 'object') return { sizes: {}, order: [] }
  const obj = parsed as Record<string, unknown>
  const sizes: Record<string, TappAppCardSize> = {}
  const order: string[] = []
  const seen = new Set<string>()

  if (Object.hasOwn(obj, 'sizes') && obj.sizes && typeof obj.sizes === 'object') {
    for (const [id, size] of Object.entries(
      obj.sizes as Record<string, unknown>,
    )) {
      if (isCardSize(size)) sizes[id] = size
    }
  } else {
    for (const [id, size] of Object.entries(obj)) {
      if (id === 'sizes' || id === 'order') continue
      if (isCardSize(size)) sizes[id] = size
    }
  }

  if (Array.isArray(obj.order)) {
    for (const item of obj.order) {
      if (typeof item !== 'string') continue
      const id = item.trim()
      if (!id || seen.has(id)) continue
      seen.add(id)
      order.push(id)
    }
  }

  return { sizes, order }
}

export function loadTappAppCardLayout(): TappAppCardLayoutLocal {
  if (typeof window === 'undefined') return { sizes: {}, order: [] }
  try {
    const raw =
      window.localStorage.getItem(CARD_LAYOUT_STORAGE_KEY) ??
      window.localStorage.getItem(CARD_SIZE_STORAGE_KEY_LEGACY)
    if (!raw) return { sizes: {}, order: [] }
    return normalizeLocalLayout(JSON.parse(raw) as unknown)
  } catch {
    return { sizes: {}, order: [] }
  }
}

export function saveTappAppCardLayout(layout: TappAppCardLayoutLocal): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(
      CARD_LAYOUT_STORAGE_KEY,
      JSON.stringify({
        sizes: layout.sizes,
        order: layout.order,
      }),
    )
  } catch {
  }
}

export function loadTappAppCardSizes(): Record<string, TappAppCardSize> {
  return loadTappAppCardLayout().sizes
}

export {
  applyTappAppCardOrder,
  isSiteOwnerLayoutPending,
} from '../utils/tappAppCardOrder'

export function toggleTappAppCardSize(size: TappAppCardSize): TappAppCardSize {
  return size === '2x1' ? '1x1' : '2x1'
}

/** 优先授予权限；否则回退 Manifest。 */
function resolveCardPermissions(
  granted: readonly TappPermission[] | undefined,
  manifestPerms: readonly string[] | undefined,
): TappPermission[] {
  if (granted && granted.length > 0) return Iterator.from(granted).toArray()
  return (manifestPerms ?? []) as TappPermission[]
}

function CardPermissionIndicators({
  permissions,
  variant = 'chips',
  maxVisible = 8,
}: {
  permissions: TappPermission[]
  variant?: 'chips' | 'full'
  maxVisible?: number
}) {
  const { t } = useI18n()
  const items = useMemo(() => {
    const rank = { privileged: 0, elevated: 1, basic: 2 } as const
    const seen = new Set<string>()
    const out: {
      key: TappPermission
      level: 'basic' | 'elevated' | 'privileged'
      label: string
    }[] = []
    for (const p of permissions) {
      if (!p || seen.has(p)) continue
      seen.add(p)
      const config = PERMISSION_CONFIG[p]
      const level = PERMISSION_LEVELS[p] ?? 'basic'
      const label = config
        ? String(
            (t.tapp as Record<string, string>)[config.labelKey] ?? p,
          )
        : p
      out.push({ key: p, level, label })
    }
    return out.toSorted((a, b) => rank[a.level] - rank[b.level])
  }, [permissions, t.tapp])

  if (items.length === 0) {
    if (variant !== 'full') return null
    return (
      <div
        className="tapp-app-card__perms tapp-app-card__perms--full"
        role="list"
        aria-label={t.tapp.permissions}
      >
        <span className="tapp-app-card__perms-empty">
          {t.tapp.noPermissions}
        </span>
      </div>
    )
  }

  const showAll = variant === 'full'
  const visible = showAll ? items : items.slice(0, maxVisible)
  const overflow = showAll ? 0 : items.length - visible.length

  return (
    <div
      className={
        showAll
          ? 'tapp-app-card__perms tapp-app-card__perms--full'
          : 'tapp-app-card__perms'
      }
      role="list"
      aria-label={t.tapp.permissions}
    >
      {visible.map(({ key, level, label }) => {
        const Icon = PERMISSION_CONFIG[key]?.icon ?? FaLock
        const showLabel = showAll && level !== 'basic'
        return (
          <span
            key={key}
            className={`tapp-app-card__perm tapp-app-card__perm--${level}${
              showLabel ? ' tapp-app-card__perm--labeled' : ''
            }`}
            role="listitem"
            title={label}
            aria-label={label}
          >
            <Icon className="tapp-app-card__perm-icon" aria-hidden />
            {showLabel ? (
              <span className="tapp-app-card__perm-label">{label}</span>
            ) : null}
          </span>
        )
      })}
      {overflow > 0 && (
        <span
          className="tapp-app-card__perm tapp-app-card__perm--more"
          role="listitem"
          title={items
            .slice(maxVisible)
            .map((i) => i.label)
            .join(' · ')}
          aria-label={`+${overflow}`}
        >
          +{overflow}
        </span>
      )}
    </div>
  )
}

function RotatingCardSubtitle({
  lines,
  phaseOffset = 0,
}: {
  lines: string[]
  phaseOffset?: number
}) {
  const animConfig = useAnimationLevel()
  const reduced = isExlight(animConfig)
  const unique = useMemo(() => {
    const seen = new Set<string>()
    const out: string[] = []
    for (const line of lines) {
      const text = line.trim()
      if (!text || seen.has(text)) continue
      seen.add(text)
      out.push(text)
    }
    return out
  }, [lines])

  const [index, setIndex] = useState(() =>
    unique.length > 0 ? phaseOffset % unique.length : 0,
  )

  useEffect(() => {
    setIndex(unique.length > 0 ? phaseOffset % unique.length : 0)
  }, [unique.join('\0'), phaseOffset, unique.length])

  useEffect(() => {
    if (unique.length <= 1 || reduced) return
    const id = window.setInterval(() => {
      setIndex((prev) => (prev + 1) % unique.length)
    }, SUBTITLE_ROTATE_MS)
    return () => window.clearInterval(id)
  }, [unique, reduced])

  if (unique.length === 0) return null

  const active = reduced
    ? unique.join(' · ')
    : unique[index % unique.length]

  return (
    <p className="tapp-app-card__sub" aria-live="polite">
      {reduced ? (
        <span className="tapp-app-card__sub-line">{active}</span>
      ) : (
        <span className="tapp-app-card__sub-viewport">
          <AnimatePresence mode="wait" initial={false}>
            <motion.span
              key={active}
              className="tapp-app-card__sub-line"
              initial={{ opacity: 0, y: 3 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -3 }}
              transition={{
                duration: 0.32 * animConfig.durationScale,
                ease: [0.22, 1, 0.36, 1],
              }}
            >
              {active}
            </motion.span>
          </AnimatePresence>
        </span>
      )}
    </p>
  )
}

function CardActionsDock({
  size,
  isRunning,
  canStartStop,
  canConfigure,
  canUninstall,
  canResize,
  hasPage,
  onToggleRun,
  onConfigure,
  onUninstall,
  onToggleSize,
  labels,
}: {
  size: TappAppCardSize
  isRunning: boolean
  canStartStop: boolean
  canConfigure: boolean
  canUninstall: boolean
  /** 访客不能改公开布局。 */
  canResize: boolean
  hasPage: boolean
  onToggleRun: (e: MouseEvent) => void
  onConfigure: () => void
  onUninstall: (anchor: HTMLElement) => void
  onToggleSize?: () => void
  labels: {
    start: string
    stop: string
    settings: string
    uninstall: string
    expand: string
    shrink: string
    openHint: string
  }
}) {
  const isWide = size === '2x1'
  const showSize = canResize && typeof onToggleSize === 'function'
  const hasLifecycle = canStartStop || canConfigure || canUninstall

  /* 无操作访客：底栏显示「点击打开」。不要 stopPropagation。 */
  if (!hasLifecycle && !showSize) {
    if (!hasPage) return null
    return (
      <div
        className="tapp-app-card__dock tapp-app-card__dock--open-hint"
        aria-hidden={false}
      >
        <span className="tapp-app-card__open-hint">{labels.openHint}</span>
      </div>
    )
  }

  return (
    <div
      className={`tapp-app-card__dock${showSize ? ' tapp-app-card__dock--4' : ' tapp-app-card__dock--3'}`}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <button
        type="button"
        className={`tapp-app-card__dock-btn ${
          isRunning
            ? 'tapp-app-card__dock-btn--stop'
            : 'tapp-app-card__dock-btn--run'
        }`}
        disabled={!canStartStop}
        onClick={onToggleRun}
        title={
          !canStartStop ? undefined : isRunning ? labels.stop : labels.start
        }
        aria-label={
          !canStartStop
            ? labels.start
            : isRunning
              ? labels.stop
              : labels.start
        }
      >
        {isRunning ? (
          <FaPause className="tapp-app-card__dock-icon" />
        ) : (
          <FaPlay className="tapp-app-card__dock-icon" />
        )}
      </button>

      <button
        type="button"
        className="tapp-app-card__dock-btn tapp-app-card__dock-btn--settings"
        disabled={!canConfigure}
        onClick={(e) => {
          e.stopPropagation()
          if (canConfigure) onConfigure()
        }}
        title={canConfigure ? labels.settings : undefined}
        aria-label={labels.settings}
      >
        <FaCog className="tapp-app-card__dock-icon" />
      </button>

      <button
        type="button"
        className="tapp-app-card__dock-btn tapp-app-card__dock-btn--danger"
        disabled={!canUninstall}
        onClick={(e) => {
          e.stopPropagation()
          if (canUninstall) onUninstall(e.currentTarget)
        }}
        title={canUninstall ? labels.uninstall : undefined}
        aria-label={labels.uninstall}
      >
        <FaTrash className="tapp-app-card__dock-icon" />
      </button>

      {showSize && (
        <button
          type="button"
          className="tapp-app-card__dock-btn tapp-app-card__dock-btn--size"
          onClick={(e) => {
            e.stopPropagation()
            onToggleSize?.()
          }}
          title={isWide ? labels.shrink : labels.expand}
          aria-label={isWide ? labels.shrink : labels.expand}
        >
          {isWide ? (
            <FaCompress className="tapp-app-card__dock-icon" />
          ) : (
            <FaExpand className="tapp-app-card__dock-icon" />
          )}
        </button>
      )}
    </div>
  )
}

export interface TappAppCardProps {
  tapp: TappInstance
  isRunning: boolean
  onStart: () => void
  onStop: () => void
  onUninstall: (anchor: HTMLElement) => void
  onConfigure: () => void
  onOpen: () => void
  index: number
  size: TappAppCardSize
  /** 切换 1x1↔2x1。访客只读公开布局。 */
  onToggleSize?: () => void
  canResize?: boolean
  canReorder?: boolean
  dragLabel?: string
  isDragging?: boolean
  isDragOver?: boolean
  onDragHandleStart?: (e: DragEvent, tappId: string) => void
  onDragOverCard?: (e: DragEvent, tappId: string) => void
  onDragLeaveCard?: (e: DragEvent, tappId: string) => void
  onDropOnCard?: (e: DragEvent, tappId: string) => void
  onDragEndCard?: () => void
}

export const TappAppCard = forwardRef<HTMLDivElement, TappAppCardProps>(
  (
    {
      tapp,
      isRunning,
      onStart,
      onStop,
      onUninstall,
      onConfigure,
      onOpen,
      index,
      size,
      onToggleSize,
      canResize = true,
      canReorder = false,
      dragLabel,
      isDragging = false,
      isDragOver = false,
      onDragHandleStart,
      onDragOverCard,
      onDragLeaveCard,
      onDropOnCard,
      onDragEndCard,
    },
    ref,
  ) => {
    const { manifest } = tapp
    const { t, locale } = useI18n()
    const { name: displayName, description: displayDescription } =
      resolveManifestText(manifest, locale)
    const animConfig = useAnimationLevel()
    const [isHovered, setIsHovered] = useState(false)

    useEffect(() => {
      if (isDragging) {
        setIsHovered(false)
      }
    }, [isDragging])

    useEffect(() => {
      if (!isDragOver) return
      return () => {
        setIsHovered(false)
      }
    }, [isDragOver])

    const isWide = size === '2x1'

    const animationsEnabled = !isExlight(animConfig)
    const { canAnimate, onComplete } = useTappStagger(index, {
      enabled: animationsEnabled,
    })

    const categoryId = resolveTappCategory(manifest)
    const needsReauthorization = tapp.needsReauthorization === true
    const isUnusable =
      tapp.installationStatus === 'error' || tapp.status === 'error'
    const unusableReason = isUnusable
      ? tapp.error?.trim() || t.tapp.packageUnusableMessage
      : ''
    const canStartStop =
      !needsReauthorization &&
      !isUnusable &&
      ((tapp.userRole === 'admin' && tapp.isAdminTapp === true) ||
        (tapp.userRole === 'user' && tapp.isTemporary === true))
    const canUninstall =
      tapp.userRole === 'admin' ||
      (tapp.userRole === 'user' && tapp.isTemporary === true)
    const canConfigure =
      tapp.userRole === 'admin' ||
      (tapp.userRole === 'user' && tapp.isTemporary === true)
    const category = t.tapp[TAPP_CATEGORY_I18N_KEYS[categoryId]]

    const hasPage =
      tappHasPage(manifest) && !needsReauthorization && !isUnusable
    const iconStyle = getTappIconStyle(tapp)
    const accent =
      iconStyle.accentColor ||
      manifest.themeColor?.trim() ||
      'var(--color-primary, #6366f1)'

    const versionLabel = manifest.version ? `v${manifest.version}` : ''
    const descriptionText = displayDescription?.trim() || ''
    const cardPermissions = useMemo(
      () =>
        needsReauthorization
          ? []
          : resolveCardPermissions(
              tapp.grantedPermissions,
              tapp.manifest.permissions,
            ),
      [
        needsReauthorization,
        tapp.grantedPermissions,
        tapp.manifest.permissions,
      ],
    )

    const subtitleLines = useMemo(() => {
      const lines: string[] = []
      if (descriptionText) lines.push(descriptionText)
      if (category?.trim()) lines.push(category.trim())
      if (versionLabel) lines.push(versionLabel)
      return lines
    }, [descriptionText, category, versionLabel])

    const detailDescription =
      descriptionText || category?.trim() || t.tapp.listSubtitle || ''

    const handleCardClick = () => {
      if (hasPage) onOpen()
    }

    const handleToggleRun = (e: MouseEvent) => {
      e.stopPropagation()
      if (!canStartStop) return
      if (isRunning) onStop()
      else onStart()
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (!hasPage) return
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault()
        onOpen()
      }
    }

    const actionLabels = {
      start: t.tapp.start,
      stop: t.tapp.stop,
      settings: t.tapp.settings,
      uninstall: t.tapp.uninstall,
      expand: t.tapp.cardExpand,
      shrink: t.tapp.cardShrink,
      openHint: t.tapp.clickToOpen,
    }

    const iconBadge = (
      face: 'rest' | 'detail',
      glyphSize: string,
      glyphText: string,
    ) => (
      <div
        className={
          face === 'rest'
            ? 'tapp-app-card__icon tapp-app-card__icon--rest'
            : 'tapp-app-card__icon tapp-app-card__icon--detail'
        }
      >
        <TappIconBadge
          icon={manifest.icon}
          iconSvg={manifest.iconSvg}
          name={displayName}
          id={manifest.id || tapp.id}
          themeColor={manifest.themeColor}
          category={manifest.category}
          permissions={manifest.permissions}
          iconStyle={iconStyle}
          shellClassName="tapp-page-icon tapp-page-icon--md"
          glyphSizeClass={glyphSize}
          glyphTextClass={glyphText}
        >
          {isRunning && face === 'rest' && (
            <div className="tapp-app-card__icon-pulse" aria-hidden />
          )}
        </TappIconBadge>
      </div>
    )

    return (
      <motion.div
        ref={ref}
        layout={!isExlight(animConfig)}
        initial={animationsEnabled ? { opacity: 0, y: 10 } : false}
        animate={
          !animationsEnabled || canAnimate
            ? { opacity: 1, y: 0 }
            : { opacity: 0, y: 10 }
        }
        exit={animationsEnabled ? { opacity: 0, scale: 0.98 } : undefined}
        onAnimationComplete={onComplete}
        transition={
          !animationsEnabled
            ? { duration: 0 }
            : !animConfig.spring || !isStandardAnimation(animConfig)
              ? { type: 'tween', duration: 0.22 }
              : {
                  type: 'spring',
                  stiffness: 420,
                  damping: 30,
                }
        }
        whileHover={
          isStandardAnimation(animConfig) && hasPage ? { y: -2 } : {}
        }
        whileTap={animationsEnabled && hasPage ? { scale: 0.99 } : {}}
        onClick={handleCardClick}
        onMouseEnter={() => {
          if (isDragging) return
          // 触控：不进入 hover/detail 面。
          if (
            typeof window !== 'undefined' &&
            window.matchMedia('(hover: none)').matches
          ) {
            return
          }
          setIsHovered(true)
        }}
        onMouseLeave={() => setIsHovered(false)}
        className={[
          'tapp-app-card',
          'glass',
          'glass-chrome-free',
          `tapp-app-card--${size}`,
          hasPage ? 'is-openable' : '',
          // 拖拽时不显示 hover 面。
          isHovered && !isDragging ? 'is-hovered' : '',
          canReorder ? 'is-reorderable' : '',
          isDragging ? 'is-dragging' : '',
          isDragOver ? 'is-drag-over' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        data-size={size}
        data-tapp-id={tapp.id}
        style={
          {
            ['--tapp-app-accent' as string]: accent,
          } as CSSProperties
        }
        role={hasPage ? 'button' : undefined}
        tabIndex={hasPage ? 0 : undefined}
        onKeyDown={hasPage ? handleKeyDown : undefined}
        title={hasPage ? t.tapp.clickToOpen : undefined}
        onDragOver={
          canReorder && onDragOverCard
            ? (e: DragEvent) => onDragOverCard(e, tapp.id)
            : undefined
        }
        onDragLeave={
          canReorder && onDragLeaveCard
            ? (e: DragEvent) => onDragLeaveCard(e, tapp.id)
            : undefined
        }
        onDrop={
          canReorder && onDropOnCard
            ? (e: DragEvent) => onDropOnCard(e, tapp.id)
            : undefined
        }
        onDragEnd={
          canReorder
            ? () => {
                setIsHovered(false)
                onDragEndCard?.()
              }
            : undefined
        }
      >
        <GlowBackground
          color={accent}
          animLevel={animConfig.level}
          shouldAnimate={isHovered && animConfig.loop}
          variant="dual"
          size={isWide ? 'md' : 'sm'}
          opacity={isRunning ? 0.24 : 0.15}
        />

        {canReorder && (
          <div
            className="tapp-app-card__drag-handle"
            role="button"
            tabIndex={0}
            draggable
            title={dragLabel || t.tapp.cardDragReorder}
            aria-label={dragLabel || t.tapp.cardDragReorder}
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                e.stopPropagation()
              }
            }}
            onDragStart={(e) => {
              e.stopPropagation()
              setIsHovered(false)
              onDragHandleStart?.(e, tapp.id)
              const card = e.currentTarget.closest(
                '.tapp-app-card',
              ) as HTMLElement | null
              if (card) {
                try {
                  e.dataTransfer.setDragImage(card, 28, 20)
                } catch {
                }
              }
              e.dataTransfer.effectAllowed = 'move'
              e.dataTransfer.setData('text/plain', tapp.id)
            }}
            onDragEnd={(e) => {
              e.stopPropagation()
              setIsHovered(false)
              try {
                ;(e.currentTarget as HTMLElement).blur()
              } catch {
              }
              onDragEndCard?.()
            }}
          >
            <FaGripVertical className="tapp-app-card__drag-icon" aria-hidden />
          </div>
        )}

        <div className="tapp-app-card__body">
          <div className="tapp-app-card__face tapp-app-card__face--rest">
            <div className="tapp-app-card__icon-wrap">
              {iconBadge(
                'rest',
                isWide ? 'w-7 h-7' : 'w-6 h-6',
                'text-xl',
              )}
            </div>
            <div className="tapp-app-card__meta">
              <div className="tapp-app-card__title-row">
                <h3 className="tapp-app-card__name">{displayName}</h3>
                {isWide && versionLabel ? (
                  <span className="tapp-app-card__rest-version">{versionLabel}</span>
                ) : null}
                {isRunning && (
                  <span
                    className="tapp-app-card__running-dot"
                    title={t.tapp.running || 'Running'}
                    aria-label={t.tapp.running || 'Running'}
                  />
                )}
                {(isUnusable || needsReauthorization) && (
                  <FaExclamationTriangle
                    className="tapp-app-card__reauth-icon"
                    title={
                      isUnusable
                        ? t.tapp.packageUnusable
                        : t.tapp.reauthorizationRequired
                    }
                    aria-label={
                      isUnusable
                        ? t.tapp.packageUnusable
                        : t.tapp.reauthorizationRequired
                    }
                  />
                )}
              </div>
              {isWide ? (
                <>
                  {isUnusable ? (
                    <span className="tapp-app-card__reauth-label">
                      {t.tapp.packageUnusable}
                    </span>
                  ) : needsReauthorization ? (
                    <span className="tapp-app-card__reauth-label">
                      {t.tapp.reauthorizationRequired}
                    </span>
                  ) : category ? (
                    <span className="tapp-app-card__rest-cat">{category}</span>
                  ) : null}
                  {descriptionText ? (
                    <p className="tapp-app-card__rest-desc">{descriptionText}</p>
                  ) : null}
                </>
              ) : (
                <RotatingCardSubtitle
                  lines={subtitleLines}
                  phaseOffset={index}
                />
              )}
            </div>
          </div>

          <div
            className={[
              'tapp-app-card__face',
              'tapp-app-card__face--detail',
              isWide ? 'tapp-app-card__face--detail-perms' : '',
            ]
              .filter(Boolean)
              .join(' ')}
          >
            {isWide ? (
              <div className="tapp-app-card__detail-mid tapp-app-card__detail-mid--perms-only">
                {isUnusable ? (
                  <p className="tapp-app-card__reauth-message">
                    <FaExclamationTriangle aria-hidden />
                    <span>{unusableReason}</span>
                  </p>
                ) : needsReauthorization ? (
                  <p className="tapp-app-card__reauth-message">
                    <FaExclamationTriangle aria-hidden />
                    <span>{t.tapp.reauthorizationMessage}</span>
                  </p>
                ) : (
                  <CardPermissionIndicators
                    permissions={cardPermissions}
                    variant="full"
                  />
                )}
              </div>
            ) : (
              <>
                <header className="tapp-app-card__detail-head">
                  {iconBadge('detail', 'w-4 h-4', 'text-sm')}
                  <div className="tapp-app-card__detail-titles">
                    <div className="tapp-app-card__title-row">
                      <h3 className="tapp-app-card__name">{displayName}</h3>
                    </div>
                    {versionLabel ? (
                      <p className="tapp-app-card__version">{versionLabel}</p>
                    ) : category ? (
                      <p className="tapp-app-card__version">{category}</p>
                    ) : null}
                  </div>
                </header>

                <div className="tapp-app-card__detail-mid">
                  {isUnusable ? (
                    <p className="tapp-app-card__reauth-message">
                      <FaExclamationTriangle aria-hidden />
                      <span>{unusableReason}</span>
                    </p>
                  ) : needsReauthorization ? (
                    <p className="tapp-app-card__reauth-message">
                      <FaExclamationTriangle aria-hidden />
                      <span>{t.tapp.reauthorizationMessage}</span>
                    </p>
                  ) : detailDescription ? (
                    <p className="tapp-app-card__detail-desc">
                      {detailDescription}
                    </p>
                  ) : null}
                </div>
              </>
            )}

            <CardActionsDock
              size={size}
              isRunning={isRunning}
              canStartStop={canStartStop}
              canConfigure={canConfigure}
              canUninstall={canUninstall}
              canResize={canResize && typeof onToggleSize === 'function'}
              hasPage={hasPage}
              onToggleRun={handleToggleRun}
              onConfigure={onConfigure}
              onUninstall={onUninstall}
              onToggleSize={onToggleSize}
              labels={actionLabels}
            />
          </div>
        </div>
      </motion.div>
    )
  },
)

TappAppCard.displayName = 'TappAppCard'
