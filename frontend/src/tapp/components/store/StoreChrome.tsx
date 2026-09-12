import type { MouseEvent, ReactNode } from 'react'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useEffect, useMemo, useState } from 'react'
import { isExlight, useAnimationLevel } from '../../../hooks/useAnimationLevel'

export function CategoryPill({
  active,
  label,
  onClick,
}: {
  active: boolean
  label: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      data-active={active ? 'true' : 'false'}
      className="as-store__cat"
    >
      {label}
    </button>
  )
}

function labelKey(label: ReactNode, kind: string): string {
  if (typeof label === 'string' || typeof label === 'number') {
    return `${kind}:${label}`
  }
  return kind
}

export function StoreGetButton({
  kind,
  label,
  disabled,
  onClick,
  title,
}: {
  kind: 'get' | 'open' | 'update' | 'busy'
  label: ReactNode
  disabled?: boolean
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void
  title?: string
}) {
  const key = labelKey(label, kind)
  return (
    <button
      type="button"
      className={`as-get as-get--${kind}`}
      data-kind={kind}
      disabled={disabled}
      onClick={onClick}
      title={title}
    >
      <span key={key} className="as-get__label">
        {label}
      </span>
    </button>
  )
}

export function ProgressPercent({
  value,
  className = '',
}: {
  value: number
  className?: string
}) {
  return (
    <span className={`tabular-nums font-bold leading-none ${className}`}>
      {Math.round(value)}
      <span className="text-[0.72em] font-semibold opacity-80">%</span>
    </span>
  )
}

const SUBTITLE_ROTATE_MS = 3200

let sharedSubtitleTick = 0
const sharedSubtitleListeners = new Set<() => void>()
let sharedSubtitleIntervalId: ReturnType<typeof setInterval> | null = null

function subscribeSharedSubtitleTick(listener: () => void): () => void {
  sharedSubtitleListeners.add(listener)
  sharedSubtitleIntervalId ??= setInterval(() => {
    sharedSubtitleTick += 1
    for (const fn of sharedSubtitleListeners) fn()
  }, SUBTITLE_ROTATE_MS)
  return () => {
    sharedSubtitleListeners.delete(listener)
    if (sharedSubtitleListeners.size === 0 && sharedSubtitleIntervalId != null) {
      clearInterval(sharedSubtitleIntervalId)
      sharedSubtitleIntervalId = null
    }
  }
}

function uniqueSubtitleLines(lines: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const line of lines) {
    const text = line.trim()
    if (!text || seen.has(text)) continue
    seen.add(text)
    out.push(text)
  }
  return out
}

export interface RotatingSubtitleProps {
  lines: string[]
  phaseOffset?: number
  sharedClock?: boolean
  className?: string
  viewportClassName?: string
  lineClassName?: string
  as?: 'p' | 'div'
}

export function RotatingSubtitle({
  lines,
  phaseOffset = 0,
  sharedClock = false,
  className = 'as-detail__category',
  viewportClassName = 'as-detail__subtitle-viewport',
  lineClassName = 'as-detail__subtitle-line',
  as: Tag = 'p',
}: RotatingSubtitleProps) {
  const animConfig = useAnimationLevel()
  const reduced = isExlight(animConfig)
  const unique = useMemo(() => uniqueSubtitleLines(lines), [lines])
  const uniqueKey = unique.join('\0')

  const [index, setIndex] = useState(() =>
    unique.length > 0 ? phaseOffset % unique.length : 0,
  )
  const [, bumpShared] = useState(0)

  useEffect(() => {
    setIndex(unique.length > 0 ? phaseOffset % unique.length : 0)
  }, [uniqueKey, phaseOffset, unique.length])

  useEffect(() => {
    if (unique.length <= 1 || reduced) return
    if (sharedClock) {
      return subscribeSharedSubtitleTick(() => bumpShared((n) => n + 1))
    }
    const id = window.setInterval(() => {
      setIndex((prev) => (prev + 1) % unique.length)
    }, SUBTITLE_ROTATE_MS)
    return () => window.clearInterval(id)
  }, [unique.length, uniqueKey, reduced, sharedClock])

  if (unique.length === 0) return null

  const active = reduced
    ? unique.join(' · ')
    : sharedClock
      ? unique[(sharedSubtitleTick + phaseOffset) % unique.length]!
      : unique[index % unique.length]!

  return (
    <Tag className={className} aria-live="polite">
      {reduced ? (
        <span className={lineClassName}>{active}</span>
      ) : (
        <span className={viewportClassName}>
          <AnimatePresence mode="wait" initial={false}>
            <motion.span
              key={active}
              className={lineClassName}
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
    </Tag>
  )
}

export function RotatingDetailSubtitle({ lines }: { lines: string[] }) {
  return <RotatingSubtitle lines={lines} />
}
