import type en from '../../i18n/en-US.json'
import type { MeropeHerResponse } from '../../services/agent/types'
import type { HerLately } from './herLife'
import React from 'react'
import { askToPlay } from './herLife'

type Copy = (typeof en)['agentPanel']['her']
type Format = (template: string, params: Record<string, string | number>) => string

/**
 * Her life as shown: the turtle soups she made (each to play, unless this
 * person has), what she wants, and what she took in lately with what she
 * made of it. Only what she has; nothing is shown for what she has not.
 */
export const HerLifeView: React.FC<{
  life: MeropeHerResponse
  copy: Copy
  format: Format
  ago: (iso: string) => string
  onPlay: (text: string) => void
}> = ({ life, copy, format, ago, onPlay }) => {
  const took = (item: HerLately): string => {
    const title = item.by ? `《${item.title}》 · ${item.by}` : `《${item.title}》`
    return item.kind === 'inquiry'
      ? format(copy.foundOut, { title: item.title })
      : format(copy.kinds[item.kind], { title })
  }

  return (
    <div className="agent-panel-her">
      {life.puzzles.length > 0 && (
        <section className="agent-panel-her-section">
          <h3 className="agent-panel-her-heading">{copy.puzzles}</h3>
          {life.puzzles.map((puzzle) => (
            <div className="agent-panel-her-item" key={puzzle.surface}>
              <p className="agent-panel-her-text">{puzzle.surface}</p>
              <div className="agent-panel-her-meta">
                <span>
                  {puzzle.played > 0
                    ? format(copy.playedCount, {
                        played: puzzle.played,
                        solved: puzzle.solved,
                      })
                    : copy.unplayed}
                </span>
                {puzzle.yours ? (
                  <span>{copy.playedByYou}</span>
                ) : (
                  <button
                    type="button"
                    className="agent-panel-tag agent-panel-tag-strong"
                    data-tone="primary"
                    onClick={() => onPlay(askToPlay(puzzle, copy.askToPlay))}
                  >
                    <span className="agent-panel-tag-text">{copy.play}</span>
                  </button>
                )}
              </div>
            </div>
          ))}
        </section>
      )}

      {life.wants.length > 0 && (
        <section className="agent-panel-her-section">
          <h3 className="agent-panel-her-heading">{copy.wants}</h3>
          {life.wants.map((want) => (
            <div className="agent-panel-her-item" key={want.want}>
              <p className="agent-panel-her-text">{want.want}</p>
              {want.why && <p className="agent-panel-her-said">{want.why}</p>}
            </div>
          ))}
        </section>
      )}

      {life.lately.length > 0 && (
        <section className="agent-panel-her-section">
          <h3 className="agent-panel-her-heading">{copy.lately}</h3>
          {life.lately.map((item) => (
            <div className="agent-panel-her-item" key={`${item.at}-${item.title}`}>
              <div className="agent-panel-her-meta">
                <span className="agent-panel-her-what">{took(item)}</span>
                <span>
                  {[item.reaction ? copy.reactions[item.reaction] : null, ago(item.at)]
                    .filter(Boolean)
                    .join(' · ')}
                </span>
              </div>
              <p className="agent-panel-her-said">{item.said}</p>
            </div>
          ))}
        </section>
      )}
    </div>
  )
}
