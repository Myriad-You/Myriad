import type { CSSProperties } from 'react'
import { currentCopy } from '../i18n/localeCopy'

export type SpinnerSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl' | number
export type SpinnerColor = 'current' | 'primary' | 'white' | (string & {})

const SIZE_PX: Record<'xs' | 'sm' | 'md' | 'lg' | 'xl', number> = {
  xs: 12,
  sm: 16,
  md: 24,
  lg: 32,
  xl: 48,
}

const COLOR_VALUE: Record<string, string> = {
  current: 'currentcolor',
  primary: 'var(--color-primary, #94a3b8)',
  white: '#fff',
}

export interface SpinnerProps {
  size?: SpinnerSize
  color?: SpinnerColor
  thickness?: number
  speed?: number
  delay?: number
  center?: boolean
  label?: string
  className?: string
}

export function Spinner({
  size = 'sm',
  color,
  thickness,
  speed,
  delay,
  center = false,
  label = currentCopy().common.loading,
  className = '',
}: SpinnerProps) {
  const px = typeof size === 'number' ? size : SIZE_PX[size]
  const resolved = color ?? 'primary'
  const style = {
    '--spinner-size': `${px}px`,
    '--spinner-color': COLOR_VALUE[resolved] ?? resolved,
    '--spinner-thickness': `${
      thickness ?? Math.min(4, Math.max(1.5, Math.round(px / 8)))
    }px`,
    ...(speed ? { '--spinner-speed': `${speed}s` } : {}),
    ...(delay ? { '--spinner-delay': `${delay}ms` } : {}),
  } as CSSProperties

  const el = (
    <span
      role="status"
      aria-label={label}
      className={center ? 'spinner' : `spinner ${className}`.trim()}
      style={style}
    />
  )

  if (!center) return el
  return <div className={`spinner-center ${className}`.trim()}>{el}</div>
}

export function ButtonSpinner({
  size = 'xs',
  className = '',
}: Pick<SpinnerProps, 'size' | 'className'>) {
  return <Spinner size={size} color="current" className={className} />
}

export default Spinner
