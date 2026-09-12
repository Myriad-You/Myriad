import { brewItemState } from './brewItemState'
import { requestCache } from './requestCache'

export interface BrewSubjectSnapshot {
  readonly key: string
  readonly active: boolean
  readonly generation: number
  readonly signal: AbortSignal
}

export class BrewSubjectScope {
  private controller = new AbortController()
  private listeners = new Set<() => void>()
  private snapshot: BrewSubjectSnapshot = {
    key: 'guest',
    active: true,
    generation: 0,
    signal: this.controller.signal,
  }

  constructor(private invalidate: () => void) {}

  getSnapshot = (): BrewSubjectSnapshot => this.snapshot

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  change(key: string, active = true, force = false): void {
    if (!force && key === this.snapshot.key && active === this.snapshot.active)
      return
    this.controller.abort()
    this.controller = new AbortController()
    this.invalidate()
    this.snapshot = {
      key,
      active,
      generation: this.snapshot.generation + 1,
      signal: this.controller.signal,
    }
    for (const listener of this.listeners) listener()
  }

  capture(): BrewSubjectSnapshot {
    this.assert(this.snapshot)
    return this.snapshot
  }

  assert(snapshot: BrewSubjectSnapshot): void {
    if (
      !snapshot.active ||
      snapshot !== this.snapshot ||
      snapshot.signal.aborted
    ) {
      throw new DOMException('Brew subject changed', 'AbortError')
    }
  }
}

export const brewSubject = new BrewSubjectScope(() => {
  requestCache.deleteByPrefix('brew:')
  brewItemState.clear()
})

export function brewSubjectKey(
  user: { id: number; is_admin?: boolean } | null,
): string {
  return user
    ? `user:${user.id}:${user.is_admin ? 'admin' : 'member'}`
    : 'guest'
}
