import type { ProgressPersist } from './progressOutbox'
import { BrewSyncConflictError } from '../../../utils/brewSyncConflict'

/** Pending progress survives the view until confirmed or cancelled. */
export class ProgressSync {
  private latest: number | null = null
  private observedAt = 0
  private confirmed: number | null = null
  private timer: ReturnType<typeof setTimeout> | null = null
  private pending: Promise<void> | null = null
  private stopped = false
  private conflicted = false
  private retryDelay = 2000
  private released = false

  constructor(
    private send: (progress: number, observedAt: number) => Promise<void>,
    private signal: AbortSignal,
    private onError: (error: unknown) => void = () => {},
    private persist?: ProgressPersist,
  ) {
    signal.addEventListener('abort', this.cancel, { once: true })
    const stored = persist?.load()
    if (stored && Number.isFinite(stored.progress)) {
      this.latest = Math.max(0, Math.min(100, Math.round(stored.progress)))
      this.observedAt = stored.observedAt
      this.schedule(0)
    }
  }

  record(progress: number): void {
    if (this.stopped || this.signal.aborted || !Number.isFinite(progress))
      return
    const value = Math.max(0, Math.min(100, Math.round(progress)))
    if (value !== this.latest) this.observedAt = Date.now()
    this.latest = value
    this.persist?.save(value, this.observedAt)
    if (this.latest !== this.confirmed) this.schedule(2000)
  }

  private schedule(delay: number): void {
    if (this.timer || this.stopped || this.conflicted || this.signal.aborted) return
    this.timer = setTimeout(() => {
      this.timer = null
      void this.flush()
    }, delay)
  }

  flush = (): Promise<void> => {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    if (this.pending) return this.pending
    if (
      this.stopped ||
      this.conflicted ||
      this.signal.aborted ||
      this.latest === null ||
      this.latest === this.confirmed
    ) {
      if (this.released) this.cancel()
      return Promise.resolve()
    }
    const value = this.latest
    const observedAt = this.observedAt
    this.pending = Promise.resolve()
      .then(() => {
        this.signal.throwIfAborted()
        return this.send(value, observedAt)
      })
      .then(() => {
        this.confirmed = value
        this.retryDelay = 2000
        if (this.latest === value) this.persist?.clear()
      })
      .catch((error) => {
        if (!this.signal.aborted) {
          if (error instanceof BrewSyncConflictError) this.conflicted = true
          this.onError(error)
          this.retryDelay = Math.min(30_000, this.retryDelay * 2)
        }
      })
      .finally(() => {
        this.pending = null
        if (this.released && this.conflicted) this.cancel()
        else if (this.latest !== this.confirmed) this.schedule(this.retryDelay)
        else if (this.released) this.cancel()
      })
    return this.pending
  }

  /** Call only after conflict resolution has a new baseline. */
  resume(): void {
    if (this.stopped || this.signal.aborted) return
    this.conflicted = false
    if (this.latest !== this.confirmed) this.schedule(2000)
  }

  /** Take the server position as confirmed and drop the local outbox. */
  adopt(progress: number): void {
    if (this.stopped || this.signal.aborted || !Number.isFinite(progress)) return
    const value = Math.max(0, Math.min(100, Math.round(progress)))
    this.latest = value
    this.confirmed = value
    this.conflicted = false
    this.persist?.clear()
  }

  /** View is gone; finish the last write, then release the abort listener. */
  release(): void {
    this.released = true
    void this.flush()
  }

  cancel = (): void => {
    this.stopped = true
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.latest = null
    this.signal.removeEventListener('abort', this.cancel)
  }
}
