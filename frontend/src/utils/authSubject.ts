/** Synchronous identity boundary; unrelated to which page or panel is visible. */
export class AuthSubjectScope {
  private key = 'guest'
  private controller = new AbortController()
  private readonly listeners = new Set<() => void>()

  get signal(): AbortSignal {
    return this.controller.signal
  }

  change(key: string, force = false): void {
    if (!force && key === this.key) return
    this.key = key
    this.controller.abort()
    this.controller = new AbortController()
    for (const listener of this.listeners) listener()
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }
}

export const authSubject = new AuthSubjectScope()

export function authSubjectKey(user: {
  id: number
  is_admin?: boolean
  is_owner?: boolean
}): string {
  return `user:${user.id}:${Boolean(user.is_admin)}:${Boolean(user.is_owner)}`
}
