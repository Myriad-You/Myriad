/** exlight / prefers-reduced-motion 不渲染光晕。禁止装饰性 border。 */

import type { CSSProperties, ReactNode } from 'react'
import { memo } from 'react'

import { isExlight, useAnimationLevel } from '../../../hooks/useAnimationLevel'
import { GlowBackground } from '../../widgets/shared/GlowBackground'
import { WidgetShell } from '../../widgets/shared/WidgetShell'
import { fs, MARK_SIZE, sp, T_META, T_TITLE } from './tokens'
import './TileShell.css'

interface TileShellProps {
  children: ReactNode
  color: string
  scale: number
  containerRef?: React.Ref<HTMLDivElement>
  contentClassName?: string
  className?: string
  style?: CSSProperties
  onClick?: () => void
  label?: string
  glow?: 'single' | 'dual' | 'single-left' | 'none'
  /** 磁贴墙必须 solid，避免 backdrop-filter 合成整面色块。 */
  surface?: 'glass' | 'solid'
  /** 只有 2x1 覆盖安全内边距；其余用默认。 */
  padding?: number | { x: number; y: number }
}

export const TileShell = memo(
  ({
    children,
    color,
    scale,
    containerRef,
    contentClassName,
    className,
    style,
    onClick,
    label,
    glow = 'single',
    surface = 'glass',
    padding,
  }: TileShellProps) => {
    const anim = useAnimationLevel()
    // exlight / prefers-reduced-motion 不渲染光晕，不是渲染但不动。
    const showGlow = glow !== 'none' && !isExlight(anim)

    const interactive = Boolean(onClick)

    return (
      <WidgetShell
        containerRef={containerRef}
        scale={scale}
        padding={padding}
        className={[
          interactive && 'cursor-pointer',
          surface === 'solid' && 'phantasi-tile-solid',
          'text-left',
          className,
        ]
          .filter(Boolean)
          .join(' ')}
        style={style}
        contentClassName={contentClassName ?? 'flex min-h-0 flex-col'}
        background={
          showGlow ? (
            <GlowBackground
              color={color}
              animLevel={anim.level}
              shouldAnimate={anim.loop}
              variant={glow}
              size="md"
              opacity={0.16}
            />
          ) : undefined
        }
        rootProps={
          interactive
            ? {
                role: 'button',
                tabIndex: 0,
                'aria-label': label,
                onClick,
                onKeyDown: (e: React.KeyboardEvent) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    onClick?.()
                  }
                },
              }
            : undefined
        }
      >
        {children}
      </WidgetShell>
    )
  },
)

TileShell.displayName = 'TileShell'

/** 没有图标不画灰块，用色底首字。 */
export function TileMark({
  name,
  color,
  scale,
  size = MARK_SIZE,
  icon,
  onIconLoad,
}: {
  name: string
  color: string
  scale: number
  size?: number
  icon?: string | null
  /** 排除 1×1 软失败后再回调。 */
  onIconLoad?: (img: HTMLImageElement) => void
}) {
  const px = sp(size, scale)
  const initial = name.trim().slice(0, 1) || '·'

  return (
    <span
      className="relative flex shrink-0 items-center justify-center overflow-hidden rounded-md"
      style={{
        width: px,
        height: px,
        background: `${color}22`,
        color,
        fontSize: `${Math.round(px * 0.56)}px`,
        fontWeight: 600,
        lineHeight: 1,
      }}
      aria-hidden
    >
      {icon ? (
        <img
          src={icon}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
          className="absolute inset-0 h-full w-full object-cover"
          onLoad={(e) => {
            // 软失败 1×1 PNG 当作没有图标。
            const img = e.currentTarget
            if (img.naturalWidth <= 1 && img.naturalHeight <= 1) {
              img.style.display = 'none'
              return
            }
            onIconLoad?.(img)
          }}
          onError={(e) => {
            e.currentTarget.style.display = 'none'
          }}
        />
      ) : null}
      {initial}
    </span>
  )
}

/** 未读只在登录后渲染。失败色只给管理员。 */
export function TileHeader({
  name,
  color,
  scale,
  fontScale,
  icon,
  unread,
  alert,
  trailing,
  onIconLoad,
}: {
  name: string
  color: string
  scale: number
  fontScale: number
  icon?: string | null
  onIconLoad?: (img: HTMLImageElement) => void
  unread?: number | null
  alert?: boolean
  trailing?: ReactNode
}) {
  return (
    <div
      className="flex min-w-0 items-center"
      style={{ gap: sp(7, scale), marginBottom: sp(8, scale) }}
    >
      <TileMark
        name={name}
        color={color}
        scale={scale}
        icon={icon}
        onIconLoad={onIconLoad}
      />
      <span
        className="min-w-0 flex-1 truncate font-semibold text-gray-800 dark:text-gray-100"
        style={{ fontSize: fs(T_TITLE, fontScale), lineHeight: 1.25 }}
      >
        {name}
      </span>
      {trailing ??
        (unread !== null && unread !== undefined && unread > 0 ? (
          <span
            className={
              alert
                ? 'shrink-0 font-semibold text-red-500 dark:text-red-400'
                : 'shrink-0 font-semibold'
            }
            style={{
              fontSize: fs(T_TITLE, fontScale),
              lineHeight: 1,
              ...(alert ? {} : { color }),
            }}
          >
            {unread}
          </span>
        ) : null)}
    </div>
  )
}

export function TileMeta({
  children,
  fontScale,
  scale,
  className,
  size = T_META,
}: {
  children: ReactNode
  fontScale: number
  scale: number
  className?: string
  size?: number
}) {
  return (
    <div
      className={`flex min-w-0 items-center text-gray-400 dark:text-gray-500 ${className ?? ''}`}
      style={{ fontSize: fs(size, fontScale), gap: sp(5, scale), lineHeight: 1.4 }}
    >
      {children}
    </div>
  )
}
