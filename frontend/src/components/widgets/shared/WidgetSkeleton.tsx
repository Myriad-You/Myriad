import type { CSSProperties, ReactNode } from 'react'
import { memo, useEffect, useMemo, useState } from 'react'
import './WidgetSkeleton.css'

export const TAPP_WIDGET_SKELETON = {
  preset: 'block' as const,
  deferMs: 100,
  offscreenHold: true as const,
  // iframe 一直不 ready 时，超过此时长显示卡住提示。
  readyTimeoutMs: 12_000,
  exitMs: 280,
} as const

export type WidgetSkeletonAccent =
  | string
  | {
      color: string
      soft?: string
    }

export type WidgetSkeletonPreset =
  | 'block'
  | 'lines'
  | 'media-row'
  | 'media-grid'
  | 'list'
  | 'hero'
  | 'stats-grid'
  | 'report'

export interface SkeletonBoneProps {
  className?: string
  style?: CSSProperties
  w?: string | number
  h?: string | number
  rounded?: 'none' | 'sm' | 'md' | 'lg' | 'xl' | 'full'
  muted?: boolean
}

export interface WidgetSkeletonProps {
  preset?: WidgetSkeletonPreset
  accent?: WidgetSkeletonAccent
  className?: string
  style?: CSSProperties
  count?: number
  columns?: number
  rows?: number
  children?: ReactNode
  label?: string
  animated?: boolean
  // deferMs=0 立刻画；Tapp 宿主用 TAPP_WIDGET_SKELETON.deferMs，避免 <100ms 闪骨架。
  deferMs?: number
  hold?: boolean
  stallMessage?: string
}

function cx(...parts: Array<string | false | undefined | null>): string {
  return parts.filter(Boolean).join(' ')
}

function toCssSize(value: string | number | undefined): string | undefined {
  if (value === undefined) return undefined
  return typeof value === 'number' ? `${value}rem` : value
}

const ROUND: Record<NonNullable<SkeletonBoneProps['rounded']>, string> = {
  none: 'rounded-none',
  sm: 'rounded-sm',
  md: 'rounded-md',
  lg: 'rounded-lg',
  xl: 'rounded-xl',
  full: 'rounded-full',
}

export function resolveSkeletonAccent(accent?: WidgetSkeletonAccent): {
  color: string
  soft?: string
} {
  if (!accent) {
    return { color: 'var(--color-primary, #6366f1)' }
  }
  if (typeof accent === 'string') {
    return { color: accent }
  }
  return { color: accent.color, soft: accent.soft }
}

export const SkeletonBone = memo(({
  className,
  style,
  w,
  h,
  rounded = 'md',
  muted = false,
}: SkeletonBoneProps) => {
  return (
    <span
      className={cx(
        muted ? 'widget-skeleton-bone--muted' : 'widget-skeleton-bone',
        ROUND[rounded],
        className,
      )}
      style={{
        width: toCssSize(w),
        height: toCssSize(h),
        ...style,
      }}
    />
  )
})

function LinesLayout({ rows }: { rows: number }) {
  const n = Math.max(1, rows)
  return (
    <>
      {Array.from({ length: n }, (_, i) => (
        <SkeletonBone
          key={i}
          h={i === 0 ? 0.7 : 0.55}
          w={i === 0 ? '72%' : i === n - 1 ? '48%' : '88%'}
          rounded="md"
          muted={i > 0}
        />
      ))}
    </>
  )
}

function MediaRowLayout() {
  return (
    <>
      <SkeletonBone className="ws-avatar" rounded="lg" />
      <div className="ws-col">
        <SkeletonBone h={0.45} w="28%" rounded="sm" muted />
        <SkeletonBone h={0.7} w="62%" rounded="md" />
        <SkeletonBone h={0.5} w="44%" rounded="sm" muted />
      </div>
    </>
  )
}

function MediaGridLayout({ count }: { count: number }) {
  return (
    <>
      {Array.from({ length: Math.max(1, count) }, (_, i) => (
        <SkeletonBone key={i} className="ws-tile" rounded="xl" />
      ))}
    </>
  )
}

function ListLayout({ rows }: { rows: number }) {
  return (
    <>
      {Array.from({ length: Math.max(1, rows) }, (_, i) => (
        <div key={i} className="ws-row">
          <SkeletonBone className="ws-row-avatar" rounded="md" />
          <div className="ws-row-body">
            <SkeletonBone h={0.55} w={`${58 + (i % 3) * 8}%`} rounded="md" />
            <SkeletonBone h={0.4} w={`${36 + (i % 2) * 10}%`} rounded="sm" muted />
          </div>
        </div>
      ))}
    </>
  )
}

function HeroLayout() {
  return (
    <>
      <SkeletonBone className="ws-hero-main" />
      <SkeletonBone className="ws-hero-sub" muted />
      <SkeletonBone className="ws-hero-meta" muted />
    </>
  )
}

function StatsGridLayout({ count }: { count: number }) {
  return (
    <>
      {Array.from({ length: Math.max(2, count) }, (_, i) => (
        <div key={i} className="ws-stat">
          <SkeletonBone h={0.45} w="40%" rounded="sm" muted />
          <SkeletonBone h={0.85} w="55%" rounded="md" />
        </div>
      ))}
    </>
  )
}

function ReportLayout() {
  return (
    <>
      <span className="ws-report-wash" aria-hidden />
      <div className="ws-report-body">
        <SkeletonBone h={0.75} w="70%" rounded="md" />
        <SkeletonBone h={0.55} w="90%" rounded="sm" muted />
        <SkeletonBone h={0.55} w="55%" rounded="sm" muted />
      </div>
      <div className="ws-report-footer" aria-hidden>
        <span className="ws-logo" />
      </div>
    </>
  )
}

export const WidgetSkeleton = memo(({
  preset = 'lines',
  accent,
  className,
  style,
  count = 4,
  columns = 2,
  rows = 3,
  children,
  label = 'Loading',
  animated = true,
  deferMs = 0,
  hold = false,
  stallMessage,
}: WidgetSkeletonProps) => {
  const [armed, setArmed] = useState(deferMs <= 0)

  useEffect(() => {
    if (deferMs <= 0) {
      setArmed(true)
      return
    }
    setArmed(false)
    const id = window.setTimeout(setArmed, deferMs, true)
    return () => window.clearTimeout(id)
  }, [deferMs])

  const resolved = useMemo(() => resolveSkeletonAccent(accent), [accent])

  const cssVars = useMemo(() => {
    const vars: CSSProperties & Record<string, string | number> = {
      '--ws-accent': resolved.color,
      '--ws-cols': columns,
    }
    if (resolved.soft) {
      vars['--ws-soft'] = resolved.soft
      vars['--ws-shine'] =
        `color-mix(in srgb, ${resolved.color} 28%, transparent)`
      vars['--ws-muted'] =
        `color-mix(in srgb, ${resolved.color} 8%, transparent)`
    }
    return vars
  }, [resolved, columns])

  if (hold) {
    return (
      <div
        className={cx('widget-skeleton', 'widget-skeleton--hold', className)}
        style={{ ...cssVars, ...style }}
        role="status"
        aria-busy="true"
        aria-label={label}
      />
    )
  }

  if (!armed) {
    return (
      <div
        className={cx(
          'widget-skeleton',
          'widget-skeleton--placeholder',
          className,
        )}
        style={{ ...cssVars, ...style }}
        role="status"
        aria-busy="true"
        aria-label={label}
      />
    )
  }

  const body = children ?? (
    <>
      {preset === 'block' && (
        <SkeletonBone className="h-full w-full" rounded="xl" />
      )}
      {preset === 'lines' && <LinesLayout rows={rows} />}
      {preset === 'media-row' && <MediaRowLayout />}
      {preset === 'media-grid' && <MediaGridLayout count={count} />}
      {preset === 'list' && <ListLayout rows={rows} />}
      {preset === 'hero' && <HeroLayout />}
      {preset === 'stats-grid' && <StatsGridLayout count={count} />}
      {preset === 'report' && <ReportLayout />}
    </>
  )

  return (
    <div
      className={cx(
        'widget-skeleton',
        !children && `widget-skeleton--${preset}`,
        !animated && 'widget-skeleton--static',
        className,
      )}
      style={{ ...cssVars, ...style }}
      role="status"
      aria-busy="true"
      aria-label={label}
    >
      {body}
      {stallMessage ? (
        <div className="widget-skeleton-stall">{stallMessage}</div>
      ) : null}
    </div>
  )
})

export const WidgetSkeletonCover = memo(({
  active,
  className,
  style,
  fill = true,
  exitMs = TAPP_WIDGET_SKELETON.exitMs,
  ...skeletonProps
}: WidgetSkeletonProps & {
  active: boolean
  fill?: boolean
  exitMs?: number
}) => {
  const [present, setPresent] = useState(active)
  const [exiting, setExiting] = useState(false)

  useEffect(() => {
    if (active) {
      setPresent(true)
      setExiting(false)
      return
    }
    if (!present) return
    setExiting(true)
    const id = window.setTimeout(() => {
      setPresent(false)
      setExiting(false)
    }, exitMs)
    return () => window.clearTimeout(id)
  }, [active, present, exitMs])

  if (!present) return null

  return (
    <div
      className={cx(
        'widget-skeleton-cover',
        fill && 'widget-skeleton-cover--fill',
        exiting ? 'widget-skeleton-cover--exit' : 'widget-skeleton-cover--shown',
        className,
      )}
      style={
        {
          '--ws-exit-ms': `${exitMs}ms`,
          ...style,
        } as CSSProperties
      }
      aria-hidden={exiting || undefined}
    >
      <WidgetSkeleton
        {...skeletonProps}
        animated={exiting ? false : skeletonProps.animated}
        deferMs={exiting ? 0 : skeletonProps.deferMs}
      />
    </div>
  )
})

export default WidgetSkeleton
