export class KeyedWrites {
  private barrierTail: Promise<unknown> | undefined
  private tails = new Map<string, Promise<unknown>>()

  run<T>(key: string, signal: AbortSignal, task: () => Promise<T>): Promise<T> {
    const previous = Promise.allSettled([this.tails.get(key), this.barrierTail])
    const pending = Promise.resolve(previous)
      .catch(() => {})
      .then(() => {
        signal.throwIfAborted()
        return task()
      })
    this.tails.set(key, pending)
    const cleanup = () => {
      if (this.tails.get(key) === pending) this.tails.delete(key)
    }
    void pending.then(cleanup, cleanup)
    return pending
  }

  barrier<T>(signal: AbortSignal, task: () => Promise<T>): Promise<T> {
    const pending = Promise.allSettled([
      ...Iterator.from(this.tails.values()).toArray(),
      this.barrierTail,
    ]).then(() => {
      signal.throwIfAborted()
      return task()
    })
    this.barrierTail = pending
    const cleanup = () => { if (this.barrierTail === pending) this.barrierTail = undefined }
    void pending.then(cleanup, cleanup)
    return pending
  }
}
