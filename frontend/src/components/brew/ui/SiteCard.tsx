/** 不认识 brew_sources。 */

import type { CSSProperties, ReactNode } from 'react'

import { LuEdit3 as Edit3 } from '@lib/icons'
import { cx } from './cx'
import { BrewPick } from './Pick'

export function SiteCard({
  id,
  name,
  description,
  icon,
  unread,
  latestTitle,
  latestWhen,
  on,
  cover,
  editing,
  picked,
  ink,
  emptyLabel,
  editLabel,
  onActivate,
  onOpenLatest,
  onEdit,
  onIconLoad,
}: {
  id: number | string
  name: string
  description?: string
  icon?: string | null
  unread?: number
  latestTitle?: string
  latestWhen?: string
  on?: boolean
  cover?: string | null
  editing?: boolean
  picked?: boolean
  ink?: string | null
  emptyLabel: string
  editLabel?: string
  onActivate: () => void
  onOpenLatest?: () => void
  onEdit?: () => void
  onIconLoad?: (img: HTMLImageElement) => void
}) {
  return (
    <div
      data-rail-id={id}
      className={cx(
        'brew-site',
        on && 'is-on',
        cover && 'is-cover',
        editing && 'is-edit',
        picked && 'is-picked',
      )}
      style={{ '--site-ink': ink } as CSSProperties}
      onClick={onActivate}
    >
      {cover ? (
        <span className="brew-site__scene" aria-hidden>
          <img key={cover} src={cover} alt="" decoding="async" />
        </span>
      ) : icon ? (
        <span className="brew-site__bleed" aria-hidden>
          <img
            src={icon}
            alt=""
            loading="lazy"
            decoding="async"
            onError={(event) => {
              event.currentTarget.style.display = 'none'
            }}
            onLoad={(event) => onIconLoad?.(event.currentTarget)}
          />
        </span>
      ) : null}
      {editing ? <BrewPick on={!!picked} /> : null}
      {editing && onEdit ? (
        <button
          type="button"
          className="brew-site__edit"
          title={editLabel}
          aria-label={editLabel}
          onClick={(event) => {
            event.stopPropagation()
            onEdit()
          }}
        >
          <Edit3 />
        </button>
      ) : null}
      {unread && unread > 0 ? (
        <span className="brew-site__unread">{unread > 99 ? '99+' : unread}</span>
      ) : null}
      <button
        type="button"
        className="brew-site__head"
        onClick={onActivate}
        aria-pressed={on}
      >
        <span className="brew-site__name">{name}</span>
        {!cover && description ? (
          <span className="brew-site__dek">{description}</span>
        ) : null}
      </button>
      {cover ? null : latestTitle ? (
        <button
          type="button"
          className="brew-site__article"
          onClick={(event) => {
            event.stopPropagation()
            onOpenLatest?.()
          }}
        >
          <span className="brew-site__article-title">{latestTitle}</span>
          {latestWhen ? (
            <span className="brew-site__when">{latestWhen}</span>
          ) : null}
        </button>
      ) : (
        <span className="brew-site__none">{emptyLabel}</span>
      )}
    </div>
  )
}

export function SiteMark({
  name,
  icon,
}: {
  name: string
  icon?: string | null
}) {
  return (
    <span className="brew-mark" aria-hidden>
      {icon ? (
        <img
          src={icon}
          alt=""
          loading="lazy"
          onError={(event) => {
            event.currentTarget.style.display = 'none'
          }}
        />
      ) : (
        name.slice(0, 1)
      )}
    </span>
  )
}

export function SalonCard({
  cardKey,
  arrive,
  note,
  cover,
  editing,
  picked,
  onClick,
  children,
}: {
  cardKey?: string
  arrive?: number
  note?: boolean
  cover?: string | null
  editing?: boolean
  picked?: boolean
  onClick?: () => void
  children: ReactNode
}) {
  return (
    <div
      className={cx(
        'brew-salon__card',
        note && 'is-note',
        cover && 'has-cover',
        editing && 'is-edit',
        picked && 'is-picked',
        arrive != null && 'is-arrive',
      )}
      data-brew-card={cardKey}
      style={
        arrive != null
          ? ({ '--brew-card-i': arrive } as CSSProperties)
          : undefined
      }
      onClick={onClick}
    >
      {cover ? (
        <span className="brew-salon__cover" aria-hidden>
          <img src={cover} alt="" loading="lazy" />
        </span>
      ) : null}
      {children}
    </div>
  )
}

export function SalonNote({
  cardKey,
  arrive,
  cover,
  kicker,
  title,
  summary,
  onClick,
}: {
  cardKey?: string
  arrive?: number
  cover?: string | null
  kicker?: ReactNode
  title: ReactNode
  summary?: ReactNode
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      className={cx(
        'brew-salon__card is-note',
        cover && 'has-cover',
        arrive != null && 'is-arrive',
      )}
      data-brew-card={cardKey}
      style={
        arrive != null
          ? ({ '--brew-card-i': arrive } as CSSProperties)
          : undefined
      }
      onClick={onClick}
    >
      {cover ? (
        <span className="brew-salon__cover" aria-hidden>
          <img src={cover} alt="" loading="lazy" />
        </span>
      ) : null}
      {kicker != null ? (
        <span className="brew-salon__kicker">{kicker}</span>
      ) : null}
      <span className="brew-salon__title">{title}</span>
      {summary ? <span className="brew-salon__summary">{summary}</span> : null}
    </button>
  )
}

export function SalonHit({
  pressed,
  title,
  mark,
  summary,
  onClick,
}: {
  pressed?: boolean
  title: ReactNode
  mark?: ReactNode
  summary?: ReactNode
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      className="brew-salon__hit"
      onClick={onClick}
      aria-pressed={pressed}
    >
      <span className="brew-salon__head">
        {mark}
        <span className="brew-salon__title">{title}</span>
      </span>
      {summary ? <span className="brew-salon__summary">{summary}</span> : null}
    </button>
  )
}

export function SalonEdit({
  label,
  onClick,
}: {
  label: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className="brew-salon__edit"
      title={label}
      aria-label={label}
      onClick={(event) => {
        event.stopPropagation()
        onClick()
      }}
    >
      <Edit3 />
    </button>
  )
}

export function SalonGrid({ children }: { children: ReactNode }) {
  return <div className="brew-skin brew-salon">{children}</div>
}
