import type { CSSProperties, ElementType, ReactNode, Ref } from 'react'

export const WIDGET_SAFE_PADDING = 14

export const WIDGET_RADIUS_SHELL = 'rounded-xl'
export const WIDGET_RADIUS_NESTED = 'rounded-lg'

export interface WidgetShellProps {
  children: ReactNode
  background?: ReactNode
  scale?: number
  padding?: number | { x: number; y: number }
  contentClassName?: string
  contentStyle?: CSSProperties
  className?: string
  style?: CSSProperties
  containerRef?: Ref<HTMLDivElement>
  as?: ElementType
  rootProps?: Record<string, unknown>
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
