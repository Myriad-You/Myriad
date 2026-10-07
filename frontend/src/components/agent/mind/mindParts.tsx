import type { ReactNode } from 'react'
import { useState } from 'react'

export function fill(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (whole, key: string) =>
    key in values ? String(values[key]) : whole,
  )
}

/** A day as month/day, with the year only when it is not this one. */
export function shortDate(at: string | null | undefined, locale: string): string {
  if (!at) return ''
  const date = new Date(at)
  if (Number.isNaN(date.getTime())) return at
  const thisYear = date.getFullYear() === new Date().getFullYear()
  return date.toLocaleDateString(locale, {
    year: thisYear ? undefined : 'numeric',
    month: 'numeric',
    day: 'numeric',
  })
}

/** Newest first, whatever order they came in. */
export function newestFirst<T>(list: readonly T[], at: (item: T) => string | null | undefined): T[] {
  const time = (item: T) => {
    const value = Date.parse(at(item) ?? '')
    return Number.isNaN(value) ? 0 : value
  }
  return [...list].sort((left, right) => time(right) - time(left))
}

export function Tag({ children, tone }: { children: ReactNode; tone?: 'accent' | 'warn' | 'muted' }) {
  return <span className={`merope-mind__tag${tone ? ` is-${tone}` : ''}`}>{children}</span>
}

/** A titled part; one with nothing in it says so on the title's line. */
export function Section({
  title,
  note,
  count,
  empty,
  emptyText,
  children,
}: {
  title: string
  note?: string
  count?: number
  empty?: boolean
  emptyText: string
  children?: ReactNode
}) {
  return (
    <section className={`merope-mind__section${empty ? ' is-empty' : ''}`}>
      <h4 className="merope-mind__heading">
        {title}
        {count ? <span className="merope-mind__count">{count}</span> : null}
        {empty ? <span className="merope-mind__none">{emptyText}</span> : null}
      </h4>
      {!empty && note ? <p className="merope-mind__note">{note}</p> : null}
      {empty ? null : children}
    </section>
  )
}

/** The first few of a list, the rest a click away. */
export function Fold<T>({
  items,
  limit,
  render,
  moreText,
  lessText,
  as: List = 'ul',
  className = 'merope-mind__list',
}: {
  items: readonly T[]
  limit: number
  render: (item: T, index: number) => ReactNode
  moreText: (hidden: number) => string
  lessText: string
  as?: 'ul' | 'ol'
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const hidden = Math.max(0, items.length - limit)
  const shown = open ? items : items.slice(0, limit)
  return (
    <>
      <List className={className}>{shown.map(render)}</List>
      {hidden > 0 ? (
        <button
          type="button"
          className="merope-mind__more"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          {open ? lessText : moreText(hidden)}
        </button>
      ) : null}
    </>
  )
}

/**
 * What still holds, the first few shown; what stopped holding behind its own
 * click. With nothing still holding, what stopped is the list.
 */
export function History<T>({
  current,
  ended,
  limit,
  render,
  moreText,
  endedText,
  lessText,
}: {
  current: readonly T[]
  ended: readonly T[]
  limit: number
  render: (item: T, index: number) => ReactNode
  moreText: (hidden: number) => string
  endedText: (count: number) => string
  lessText: string
}) {
  const [all, setAll] = useState(false)
  const [showEnded, setShowEnded] = useState(false)
  const main = current.length > 0 ? current : ended
  const rest = current.length > 0 ? ended : []
  const hidden = Math.max(0, main.length - limit)
  const shown = [...(all ? main : main.slice(0, limit)), ...(showEnded ? rest : [])]
  return (
    <>
      <ul className="merope-mind__list">{shown.map(render)}</ul>
      {hidden > 0 || rest.length > 0 ? (
        <div className="merope-mind__folds">
          {hidden > 0 && (
            <button type="button" className="merope-mind__more" aria-expanded={all} onClick={() => setAll((value) => !value)}>
              {all ? lessText : moreText(hidden)}
            </button>
          )}
          {rest.length > 0 && (
            <button
              type="button"
              className="merope-mind__more is-quiet"
              aria-expanded={showEnded}
              onClick={() => setShowEnded((value) => !value)}
            >
              {showEnded ? lessText : endedText(rest.length)}
            </button>
          )}
        </div>
      ) : null}
    </>
  )
}
