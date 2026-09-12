/** 一条链，后来的顶掉前一次收尾权。intro 不 dropFlip；只有 flip 能。ChipLane 不占 lane。 */
export type BrewMotionLane = 'idle' | 'intro' | 'flip' | 'lane'

let token = 0
let lane: BrewMotionLane = 'idle'

export function brewMotionQuiet(): boolean {
  if (typeof window === 'undefined') return true
  return (
    window.matchMedia('(prefers-reduced-motion: reduce)').matches ||
    document.documentElement.getAttribute('data-perf-mode') === 'exlight'
  )
}

export function brewMotionLane(): BrewMotionLane {
  return lane
}

export function brewMotionClaim(next: Exclude<BrewMotionLane, 'idle'>): number {
  token += 1
  lane = next
  return token
}

export function brewMotionOwns(id: number): boolean {
  return id === token
}

export function brewMotionRelease(id: number): void {
  if (id !== token) return
  lane = 'idle'
}

export function brewMotionReset(): void {
  token += 1
  lane = 'idle'
}
