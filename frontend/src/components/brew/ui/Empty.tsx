import type { ReactNode } from 'react'

import { cx } from './cx'

export function BrewEmpty({
  children,
  arrive = true,
  cardKey = 'empty',
}: {
  children: ReactNode
  arrive?: boolean
  cardKey?: string
}) {
  return (
    <div
      className={cx('brew-skin brew-empty', arrive && 'is-arrive')}
      data-brew-card={cardKey}
    >
      {children}
    </div>
  )
}

export function BrewEmptyMark({ children }: { children: ReactNode }) {
  return (
    <span className="brew-empty__mark" aria-hidden>
      {children}
    </span>
  )
}

export function BrewEmptyHint({ children }: { children: ReactNode }) {
  return <p className="brew-empty__hint">{children}</p>
}

export function BrewEmptyRow({ children }: { children: ReactNode }) {
  return <div className="brew-empty__row">{children}</div>
}

export function BrewEmptyAction({
  children,
  onClick,
}: {
  children: ReactNode
  onClick?: () => void
}) {
  return (
    <button type="button" className="brew-empty__action" onClick={onClick}>
      {children}
    </button>
  )
}
