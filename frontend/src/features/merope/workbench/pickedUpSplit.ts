import type { TurnKeyformsAsked, TurnKeyformsBeside } from '../turnKeyformsApi'

/**
 * What a picked-up split's result is preflighted with. A split picked up by a
 * page that did not start it (opened or reloaded while it ran, or after it
 * ended with no page there) was asked for before this page's boxes were as
 * they are: the redrawn expressions and gesture arms its own page chose to
 * put on it are the job's to say, and this page's boxes speak only where the
 * job kept no choice.
 */
export function besideOfPickedUp(asked: TurnKeyformsAsked | undefined, page: TurnKeyformsBeside): TurnKeyformsBeside {
  return {
    expressions: asked?.expressions ?? page.expressions,
    arms: asked?.arms ?? page.arms,
  }
}

/**
 * The page's boxes as the picked-up job was asked, so that they show what is
 * being made and what the result carries; null leaves a box as it is. A kept
 * job used again says nothing of the parts to key.
 */
export function boxesOfPickedUp(asked: TurnKeyformsAsked): Record<'turn' | 'body' | 'expressions' | 'arms', boolean | null> {
  const kept = asked.fromArchive !== null
  return {
    turn: kept ? null : asked.head,
    body: kept ? null : asked.body,
    expressions: asked.expressions,
    arms: asked.arms,
  }
}
