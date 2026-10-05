import type en from '../../../i18n/en-US.json'
import type { MeropeHerResponse } from '../../../services/agent/types'
import type { ManagedListItem, ManagedListTone } from '../../settings/ManagedList'
import type { HerLately } from './herLife'
import { LuBookOpen, LuHeart, LuMusic, LuPuzzle, LuSearch } from '@lib/icons'
import React from 'react'
import { askToPlay } from './herLife'

type Copy = (typeof en)['agentPanel']['her']
type Format = (template: string, params: Record<string, string | number>) => string

export type HerKind = 'puzzles' | 'wants' | 'lately'
export type HerFilter = 'all' | HerKind

const REACTION_TONE: Record<NonNullable<HerLately['reaction']>, ManagedListTone> = {
  moved: 'success',
  liked: 'active',
  fine: 'default',
  not_for_me: 'muted',
}

/** What kind of row it is, in the list's own round icon slot. */
function mark(icon: React.ReactNode): React.ReactNode {
  return (
    <span className="managed-list-avatar-fallback" aria-hidden>
      {icon}
    </span>
  )
}

function latelyIcon(kind: HerLately['kind']): React.ReactNode {
  if (kind === 'song') return mark(<LuMusic />)
  if (kind === 'inquiry') return mark(<LuSearch />)
  return mark(<LuBookOpen />)
}

/**
 * Her life as rows of one list: the turtle soups she made (each to play,
 * unless this person has), what she wants, and what she took in lately with
 * what she made of it. Only what she has; nothing is shown for what she has not.
 */
export function herItems({
  life,
  filter,
  copy,
  format,
  ago,
  onPlay,
}: {
  life: MeropeHerResponse
  filter: HerFilter
  copy: Copy
  format: Format
  ago: (iso: string) => string
  onPlay: (text: string) => void
}): ManagedListItem[] {
  const shows = (kind: HerKind) => filter === 'all' || filter === kind

  const puzzles: ManagedListItem[] = shows('puzzles')
    ? life.puzzles.map((puzzle) => ({
        id: `puzzle:${puzzle.surface}`,
        leading: mark(<LuPuzzle />),
        title: puzzle.surface,
        meta:
          puzzle.played > 0
            ? format(copy.playedCount, {
                played: puzzle.played,
                solved: puzzle.solved,
              })
            : copy.unplayed,
        badge: puzzle.yours
          ? { label: copy.playedByYou, tone: 'muted' as const }
          : undefined,
        actions: puzzle.yours
          ? undefined
          : [
              {
                key: 'play',
                label: copy.play,
                variant: 'primary' as const,
                onClick: () => onPlay(askToPlay(puzzle, copy.askToPlay)),
              },
            ],
      }))
    : []

  const wants: ManagedListItem[] = shows('wants')
    ? life.wants.map((want) => ({
        id: `want:${want.want}`,
        leading: mark(<LuHeart />),
        title: want.want,
        subtitle: want.why || undefined,
        meta: want.since ? ago(want.since) : undefined,
      }))
    : []

  const lately: ManagedListItem[] = shows('lately')
    ? life.lately.map((item) => {
        const title = item.by ? `《${item.title}》 · ${item.by}` : `《${item.title}》`
        return {
          id: `lately:${item.at}:${item.title}`,
          leading: latelyIcon(item.kind),
          title:
            item.kind === 'inquiry'
              ? format(copy.foundOut, { title: item.title })
              : format(copy.kinds[item.kind], { title }),
          subtitle: item.said,
          meta: ago(item.at),
          badge: item.reaction
            ? {
                label: copy.reactions[item.reaction],
                tone: REACTION_TONE[item.reaction],
              }
            : undefined,
        }
      })
    : []

  return [...puzzles, ...wants, ...lately]
}
