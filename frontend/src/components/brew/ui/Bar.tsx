/** 不含排序 / 编辑 / 收藏波次。 */

import type { ReactNode, Ref } from 'react'

import { BrewChipLane, BrewPanelLane } from './Chip'
import { cx } from './cx'
import './brew.css'

export function BrewBar({
  children,
  panel,
  page,
  wave = 'default',
  onDisplayed,
}: {
  children: ReactNode
  panel?: ReactNode
  page?: boolean
  wave?: string
  onDisplayed?: (wave: string) => void
}) {
  return (
    <div className={cx('brew-skin brew-bar', page && 'brew-bar--page')}>
      <BrewChipLane wave={wave} onDisplayed={onDisplayed}>
        {children}
      </BrewChipLane>
      <BrewPanelLane>{panel}</BrewPanelLane>
    </div>
  )
}

export function BrewTag({
  children,
  onClick,
  href,
  pressed,
  disabled,
  danger,
  title,
  htmlFor,
}: {
  children: ReactNode
  onClick?: () => void
  href?: string
  pressed?: boolean
  disabled?: boolean
  danger?: boolean
  title?: string
  htmlFor?: string
}) {
  const className = cx(
    'brew-bar__tag',
    pressed && 'is-on',
    danger && 'is-danger',
  )
  if (href) {
    return (
      <a
        className={className}
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        title={title}
        aria-label={title}
      >
        {children}
      </a>
    )
  }
  if (htmlFor) {
    return (
      <label className={className} htmlFor={htmlFor} title={title}>
        {children}
      </label>
    )
  }
  return (
    <button
      type="button"
      className={className}
      onClick={onClick}
      disabled={disabled}
      aria-pressed={pressed}
      title={title}
      aria-label={title}
    >
      {children}
    </button>
  )
}

export function BrewMark({ children }: { children: ReactNode }) {
  return (
    <span className="brew-bar__mark" aria-hidden>
      {children}
    </span>
  )
}

export function BrewLabel({ children }: { children: ReactNode }) {
  return <span className="brew-bar__label">{children}</span>
}

export function BrewBarWrap({
  children,
  wrapRef,
}: {
  children: ReactNode
  wrapRef?: Ref<HTMLDivElement>
}) {
  return (
    <div className="brew-bar__wrap" ref={wrapRef}>
      {children}
    </div>
  )
}

export function BrewBarMenu({ children }: { children: ReactNode }) {
  return (
    <div className="brew-bar__menu" role="listbox">
      {children}
    </div>
  )
}

export function BrewBarMenuItem({
  children,
  on,
  onClick,
}: {
  children: ReactNode
  on?: boolean
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={on}
      className={cx('brew-bar__menu-item', on && 'is-on')}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

export function BrewBarMeta({ children }: { children: ReactNode }) {
  return <span className="brew-bar__meta">{children}</span>
}

export function BrewBarTitle({ children }: { children: ReactNode }) {
  return <span className="brew-bar__title">{children}</span>
}

export function BrewBarSearchField({
  value,
  onChange,
  placeholder,
}: {
  value: string
  onChange?: (value: string) => void
  placeholder?: string
}) {
  return (
    <input
      className="brew-bar__search"
      type="search"
      value={value}
      onChange={(event) => onChange?.(event.target.value)}
      placeholder={placeholder}
      autoFocus
    />
  )
}
