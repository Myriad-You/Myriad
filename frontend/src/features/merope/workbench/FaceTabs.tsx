import type { ReactNode } from 'react'

export function FaceTabs<T extends string>({
  ariaLabel,
  value,
  options,
  onChange,
  className,
  trailing,
}: {
  ariaLabel: string
  value: T
  options: Array<{ value: T; label: string }>
  onChange: (value: T) => void
  className?: string
  /** Sits at the end of the tab row, outside the tablist. */
  trailing?: ReactNode
}) {
  const tabs = (
    <div
      className={['merope-motion-page__tabs', className, trailing && 'is-bare']
        .filter(Boolean)
        .join(' ')}
      role="tablist"
      aria-label={ariaLabel}
    >
      {options.map((item) => (
        <button
          key={item.value}
          type="button"
          role="tab"
          aria-selected={value === item.value}
          className={`merope-motion-page__tab${value === item.value ? ' is-active' : ''}`}
          onClick={() => onChange(item.value)}
        >
          {item.label}
        </button>
      ))}
    </div>
  )
  if (!trailing) return tabs
  return (
    <div className="merope-motion-page__tabbar">
      {tabs}
      {trailing}
    </div>
  )
}
