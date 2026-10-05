import type { MeropeHerResponse } from '../../../services/agent/types'

export type HerLately = MeropeHerResponse['lately'][number]
export type HerPuzzle = MeropeHerResponse['puzzles'][number]

/**
 * What asking to play one of her puzzles says: the puzzle named by its
 * surface, so she brings out that one (the server matches its start).
 */
export function askToPlay(puzzle: HerPuzzle, template: string): string {
  return template.replace('{surface}', puzzle.surface)
}

/** Whether there is anything of her life to show yet. */
export function hasLife(her: MeropeHerResponse | null): boolean {
  return (
    !!her &&
    (her.lately.length > 0 || her.wants.length > 0 || her.puzzles.length > 0)
  )
}
