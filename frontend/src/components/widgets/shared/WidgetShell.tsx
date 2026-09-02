/**
 * 小组件外壳 - 统一的「四周溢出保护区」
 *
 * 提供所有小组件共同的容器约定：
 * - 玻璃圆角根节点 + `overflow: hidden`（内容永远被裁在圆角内，不外溢）
 * - 一层**边到边的背景**（如 GlowBackground），铺满不受安全区影响
 * - 一层带**安全内边距**的内容区：四周留出随尺寸缩放的保护间距，
 *   文本/图标不会贴边或被圆角切到
 *
 * 各小组件把根 `div.relative.h-full.w-full.rounded-xl.overflow-hidden.glass`
 * 换成本组件即可获得一致的保护区，内容排版通过 `contentClassName` 传入。
 *
 * ## 圆角矩形层级（仅限 rounded rect，不含 pill/circle）
 *
 * 卡片外圆角固定 `xl`；卡片内嵌套的圆角矩形应逐级略小，避免内层比外壳更圆：
 * - shell  `rounded-xl` — 卡片本身
 * - nested `rounded-lg` — 内层表面（列表行、媒体块、icon tile、空状态框）
 * - micro  `rounded-md` — 更小的矩形控件（小图标底、行内 hover、提示角标）
 */

import type { CSSProperties, ElementType, ReactNode, Ref } from 'react'

/** 安全区基准内边距（px，会乘以 scale） */
export const WIDGET_SAFE_PADDING = 14

/** 卡片外壳圆角 class（12px） */
export const WIDGET_RADIUS_SHELL = 'rounded-xl'
/** 卡片内嵌套表面圆角 class（8px）— 面板 / 媒体 / tile / 列表行 */
export const WIDGET_RADIUS_NESTED = 'rounded-lg'

export interface WidgetShellProps {
  children: ReactNode
  /** 边到边的背景层（如 GlowBackground），不受安全内边距影响 */
  background?: ReactNode
  /** 尺寸缩放，用于按比例计算安全区内边距 */
  scale?: number
  /**
   * 四周安全内边距（溢出保护区）。number = 四边同一基准 px；
   * `{ x, y }` 分别指定横/纵。最终会乘以 scale。默认 {@link WIDGET_SAFE_PADDING}。
   */
  padding?: number | { x: number; y: number }
  /** 内容层排版类名（flex 方向等） */
  contentClassName?: string
  /** 内容层内联样式 */
  contentStyle?: CSSProperties
  className?: string
  style?: CSSProperties
  containerRef?: Ref<HTMLDivElement>
  /** 根节点组件，默认 'div'；可传 motion.div 以支持根级交互动画 */
  as?: ElementType
  /** 透传给根节点的额外 props（事件处理器、motion 动画属性等） */
  rootProps?: Record<string, unknown>
  /** 是否渲染表面底（默认 true；内嵌进已有外壳的容器时关掉） */
  glass?: boolean
}

function cx(...parts: (string | false | undefined)[]): string {
  return parts.filter(Boolean).join(' ')
}

export function WidgetShell({
  children,
  background,
  scale = 1,
  padding = WIDGET_SAFE_PADDING,
  contentClassName,
  contentStyle,
  className,
  style,
  containerRef,
  as: Root = 'div',
  rootProps,
  glass = true,
}: WidgetShellProps) {
  const px = typeof padding === 'number' ? padding : padding.x
  const py = typeof padding === 'number' ? padding : padding.y

  return (
    <Root
      ref={containerRef}
      className={cx(
        // 表面主题由全局 html[data-surface] 驱动 .glass，无需在此挂类
        `relative h-full w-full overflow-hidden ${WIDGET_RADIUS_SHELL}`,
        glass && 'glass',
        className,
      )}
      style={style}
      {...rootProps}
    >
      {background}
      <div
        className={cx('relative z-[1] h-full w-full min-w-0', contentClassName)}
        style={{
          paddingInline: `${Math.round(px * scale)}px`,
          paddingBlock: `${Math.round(py * scale)}px`,
          ...contentStyle,
        }}
      >
        {children}
      </div>
    </Root>
  )
}
