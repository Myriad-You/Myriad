/**
 * 磁贴外壳与站名行。
 *
 * `TileShell` 只是 `WidgetShell` 的薄封装：补上身份色光晕、点击语义和
 * 「exlight / prefers-reduced-motion 不渲染光晕」这条硬规则。圆角、安全内
 * 边距、overflow 裁切全部交给 WidgetShell，不在这里重写。
 *
 * 禁止在磁贴里出现装饰性 border / 左边框：层次靠留白、嵌套表面、字重与颜色反差。
 */

import type { CSSProperties, ReactNode } from 'react'
import { memo } from 'react'

import { isExlight, useAnimationLevel } from '../../../hooks/useAnimationLevel'
import { GlowBackground } from '../../widgets/shared/GlowBackground'
import { WidgetShell } from '../../widgets/shared/WidgetShell'
import { fs, MARK_SIZE, sp, T_META, T_TITLE } from './tokens'
import './TileShell.css'

export interface TileShellProps {
  children: ReactNode
  /** 身份色（已经过 normalizeThemeColor） */
  color: string
  scale: number
  containerRef?: React.Ref<HTMLDivElement>
  contentClassName?: string
  className?: string
  style?: CSSProperties
  onClick?: () => void
  /** 无障碍标签；给了就渲染成 button 语义 */
  label?: string
  /** 光晕布局；2×2 用 single，通栏用 dual */
  glow?: 'single' | 'dual' | 'single-left' | 'none'
  /**
   * 表面：首页几张 widget 用默认毛玻璃；磁贴墙一屏二十多张必须用 solid ——
   * 大量 backdrop-filter 兄弟会被 Chrome 合并成整面墙的一块矩形色块。
   */
  surface?: 'glass' | 'solid'
  /**
   * 覆盖安全内边距。只有 `1x2` / `2x1` 这两档窄卡需要 —— 默认的 14px
   * 在 61px 宽的竖条上会吃掉将近一半，正文放不下一个站名。
   * 其余尺寸一律用默认值，不要在这里调版。
   */
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
    // exlight 与 prefers-reduced-motion 一律不渲染光晕（不是「渲染但不动」）：
    // 一屏十几张卡各带一个合成层，静止的光晕也照样吃合成预算。
    const showGlow = glow !== 'none' && !isExlight(anim)

    const interactive = Boolean(onClick)

    return (
      <WidgetShell
        containerRef={containerRef}
        scale={scale}
        padding={padding}
        className={[
          interactive && 'cursor-pointer',
          surface === 'solid' && 'brew-tile-solid',
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

/**
 * 扁字标：站名首字 + 身份色。没有图标时不画灰块，直接用色底的首字。
 *
 * 两种形态：
 * - `mark` 行内小字标，跟在站名左边，只是个记号
 * - `avatar` 站点头像。入口型磁贴上它就是整张卡的主体，所以更大、更圆、
 *   底色更淡，并补一圈发丝描边把没有图标的那些也框成一个「头像」
 */
export function TileMark({
  name,
  color,
  scale,
  size = MARK_SIZE,
  icon,
  onIconLoad,
  variant = 'mark',
}: {
  name: string
  color: string
  scale: number
  size?: number
  /** 已经过 getIconUrl 的图标地址；缺失或加载失败时退回字标 */
  icon?: string | null
  /** 图标真正加载成功后回调（已排除 1×1 软失败占位）—— 主题色提取挂这里 */
  onIconLoad?: (img: HTMLImageElement) => void
  variant?: 'mark' | 'avatar'
}) {
  const px = sp(size, scale)
  const initial = name.trim().slice(0, 1) || '·'
  const avatar = variant === 'avatar'

  return (
    <span
      className={`relative flex shrink-0 items-center justify-center overflow-hidden ${
        avatar ? 'rounded-[28%]' : 'rounded-md'
      }`}
      style={{
        width: px,
        height: px,
        // 头像底色压到很淡：一墙入口如果每张都是饱和色块，整面墙就成了色卡
        background: avatar ? `${color}18` : `${color}22`,
        boxShadow: avatar ? `inset 0 0 0 1px ${color}28` : undefined,
        color,
        fontSize: `${Math.round(px * (avatar ? 0.42 : 0.56))}px`,
        fontWeight: avatar ? 500 : 600,
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
            // 后端软失败会回 1×1 透明 PNG，当作没有图标
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

/**
 * 站名行：左字标 + 站名，右侧未读数字。
 *
 * 未读只在登录后渲染（游客侧后端恒回 0，画出来就是假信息）。
 * 管理员看失败源时右侧换红色 token —— 这是唯一允许用颜色报警的位置。
 */
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
  /** null = 不显示（游客 / 无未读） */
  unread?: number | null
  /** 管理员视图下的失败态 */
  alert?: boolean
  /** 右侧自定义内容，优先于 unread */
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

/** 底部一行 meta 文本（时间 · 阅读时长 / 轴标签）。 */
export function TileMeta({
  children,
  fontScale,
  scale,
  className,
}: {
  children: ReactNode
  fontScale: number
  scale: number
  className?: string
}) {
  return (
    <div
      className={`flex min-w-0 items-center text-gray-400 dark:text-gray-500 ${className ?? ''}`}
      style={{ fontSize: fs(T_META, fontScale), gap: sp(5, scale), lineHeight: 1.4 }}
    >
      {children}
    </div>
  )
}
