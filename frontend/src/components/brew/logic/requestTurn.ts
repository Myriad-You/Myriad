/** Latest intent wins, including close/unmount. */
export class RequestTurn {
  private controller = new AbortController()

  begin(): AbortSignal {
    this.controller.abort()
    this.controller = new AbortController()
    return this.controller.signal
  }

  cancel(): void {
    this.controller.abort()
  }
}

export function unlessAborted(signal: AbortSignal, apply: () => void): boolean {
  if (signal.aborted) return false
  apply()
  return true
}
