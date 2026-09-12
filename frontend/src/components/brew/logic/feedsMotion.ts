/** Target vs displayed site-grid, plus a generation so stale flips cannot commit. */

export interface SitesIntent {
  targetOpen: boolean
  displayOpen: boolean
  flipping: boolean
  generation: number
}

export function idleSitesIntent(open = false): SitesIntent {
  return {
    targetOpen: open,
    displayOpen: open,
    flipping: false,
    generation: 0,
  }
}

export type SitesRequestAction = 'noop' | 'flip' | 'retarget'

export function requestSiteView(
  state: SitesIntent,
  open: boolean,
): { state: SitesIntent; action: SitesRequestAction } {
  if (state.targetOpen === open && (state.flipping || state.displayOpen === open)) {
    return { state, action: 'noop' }
  }
  return {
    state: {
      targetOpen: open,
      displayOpen: open,
      flipping: true,
      generation: state.generation + 1,
    },
    action: state.flipping ? 'retarget' : 'flip',
  }
}

export type SitesCompleteAction = 'idle' | 'ignore' | 'flip'

export function completeSiteView(
  state: SitesIntent,
  generation: number,
): { state: SitesIntent; action: SitesCompleteAction } {
  if (generation !== state.generation) return { state, action: 'ignore' }
  if (state.targetOpen !== state.displayOpen) {
    return {
      state: {
        targetOpen: state.targetOpen,
        displayOpen: state.targetOpen,
        flipping: true,
        generation: state.generation + 1,
      },
      action: 'flip',
    }
  }
  return {
    state: { ...state, flipping: false },
    action: 'idle',
  }
}

export function articleSwapStale(
  startedGeneration: number,
  currentGeneration: number,
): boolean {
  return startedGeneration !== currentGeneration
}
