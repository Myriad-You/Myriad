/** 不认识 brew_items。 */

import type { CSSProperties, ReactNode } from 'react'
import { LuStar as Star } from '@lib/icons'

import { forwardRef } from 'react'
import { cx } from './cx'
import { BrewPick } from './Pick'

export interface StoryCardFace {
  id: number | string
  title: string
  summary?: string
  cover?: string | null
  source?: string
  sourceIcon?: string | null
  when?: string
  topic?: string | null
  hue?: string | null
  author?: string | null
  unread?: boolean
  starred?: boolean
}

export const StoryCard = forwardRef<
  HTMLDivElement,
  {
    face: StoryCardFace
    unreadLabel: string
    starLabel: string
    unstarLabel: string
    onOpen: () => void
    onPeek?: () => void
    onPeekEnd?: () => void
    onToggleStar?: () => void
    current?: boolean
    picked?: boolean
    picking?: boolean
    arrive?: number
    railId?: number | string
  }
>((
  {
    face,
    unreadLabel,
    starLabel,
    unstarLabel,
    onOpen,
    onPeek,
    onPeekEnd,
    onToggleStar,
    current = false,
    picked = false,
    picking = false,
    arrive,
    railId,
  },
  ref,
) => {
  const style = {
    ...(face.hue ? { '--story-topic': face.hue } : null),
    ...(arrive != null ? { '--brew-card-i': arrive } : null),
  } as CSSProperties

  return (
    <div
      ref={ref}
      data-rail-id={railId}
      data-brew-card={arrive != null ? `story:${face.id}` : undefined}
      className={cx(
        'brew-story',
        !face.unread && null,
        face.unread && 'is-unread',
        face.cover && 'has-cover',
        onToggleStar && !picking && 'has-star',
        current && 'is-current',
        picked && 'is-picked',
        picking && 'is-picking',
        arrive != null && 'is-arrive',
      )}
      style={Object.keys(style).length ? style : undefined}
      onPointerEnter={(event) => {
        if (event.pointerType === 'touch') return
        onPeek?.()
      }}
      onPointerLeave={onPeekEnd}
    >
      <button
        type="button"
        className="brew-story__hit"
        onClick={onOpen}
        aria-pressed={picking ? picked : undefined}
      >
        <span className="brew-float brew-story__shell">
          <span className="brew-story__body">
            <span className="brew-story__kicker">
              {face.topic ? (
                <span className="brew-story__topic">{face.topic}</span>
              ) : null}
              {face.source || face.sourceIcon ? (
                <span className="brew-story__source">
                  {face.sourceIcon ? (
                    <img
                      src={face.sourceIcon}
                      alt=""
                      loading="lazy"
                      decoding="async"
                      onError={(event) => {
                        event.currentTarget.hidden = true
                      }}
                    />
                  ) : face.source ? (
                    <span className="brew-story__source-mark" aria-hidden>
                      {face.source.slice(0, 1)}
                    </span>
                  ) : null}
                  {face.source ? <span>{face.source}</span> : null}
                </span>
              ) : null}
              {face.when ? (
                <span className="brew-story__meta">{face.when}</span>
              ) : null}
              {face.unread ? (
                <span className="brew-story__unread">{unreadLabel}</span>
              ) : null}
            </span>
            <span className="brew-story__title">{face.title}</span>
            {face.author ? (
              <span className="brew-story__byline">{face.author}</span>
            ) : null}
            {face.cover || face.summary ? (
              <span className="brew-story__foot">
                {face.cover ? (
                  <span className="brew-story__thumb" aria-hidden>
                    <img
                      src={face.cover}
                      alt=""
                      loading="lazy"
                      decoding="async"
                      onError={(event) => {
                        const thumb = event.currentTarget.closest(
                          '.brew-story__thumb',
                        )
                        const story = event.currentTarget.closest('.brew-story')
                        if (thumb instanceof HTMLElement) thumb.hidden = true
                        story?.classList.remove('has-cover')
                      }}
                    />
                  </span>
                ) : null}
                {face.summary ? (
                  <span className="brew-story__summary">{face.summary}</span>
                ) : null}
              </span>
            ) : null}
          </span>
        </span>
      </button>
      {picking ? (
        <BrewPick on={picked} />
      ) : onToggleStar ? (
        <button
          type="button"
          className={cx('brew-story__star', face.starred && 'is-on')}
          title={face.starred ? unstarLabel : starLabel}
          aria-label={face.starred ? unstarLabel : starLabel}
          aria-pressed={!!face.starred}
          onClick={(event) => {
            event.stopPropagation()
            onToggleStar()
          }}
        >
          <Star />
        </button>
      ) : null}
    </div>
  )
})

export function StoryGrid({ children }: { children: ReactNode }) {
  return <div className="brew-skin brew-stories">{children}</div>
}
