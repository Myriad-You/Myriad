interface TranscriptionTask<T> {
  controller: AbortController
  run: (signal: AbortSignal) => Promise<T>
  state: 'waiting' | 'running' | 'ready'
  result?: { value: T } | { error: unknown }
}

/**
 * Two requests can overlap without submitting the second sentence first.
 * Reset detaches every task before aborting, including providers that ignore
 * cancellation. No network response owns the current UI listening session.
 */
export class TranscriptionQueue<T> {
  private tasks: TranscriptionTask<T>[] = []
  private running = 0

  constructor(
    private readonly host: {
      onResult: (value: T) => void
      onError: (error: unknown) => void
      onBusy: (busy: boolean) => void
    },
  ) {}

  enqueue(run: (signal: AbortSignal) => Promise<T>): void {
    this.tasks.push({
      controller: new AbortController(),
      run,
      state: 'waiting',
    })
    this.host.onBusy(true)
    this.pump()
  }

  reset(): void {
    const abandoned = this.tasks
    this.tasks = []
    this.running = 0
    for (const task of abandoned) task.controller.abort()
    this.host.onBusy(false)
  }

  private pump(): void {
    for (const task of this.tasks) {
      if (this.running >= 2) break
      if (task.state !== 'waiting') continue
      task.state = 'running'
      this.running += 1
      void this.execute(task)
    }
  }

  private async execute(task: TranscriptionTask<T>): Promise<void> {
    // Bound both provider waits and the amount of time a later result is held.
    let timer: ReturnType<typeof setTimeout> | undefined
    const signal = task.controller.signal
    let onAbort = () => {}
    try {
      const deadline = new Promise<never>((_resolve, reject) => {
        onAbort = () => reject(signal.reason)
        signal.addEventListener('abort', onAbort, { once: true })
        timer = setTimeout(
          () =>
            task.controller.abort(new Error('Speech recognition timed out')),
          30_000,
        )
      })
      const value = await Promise.race([task.run(signal), deadline])
      task.result = { value }
    } catch (error) {
      task.result = { error }
    } finally {
      clearTimeout(timer)
      signal.removeEventListener('abort', onAbort)
    }
    if (!this.tasks.includes(task)) return
    task.state = 'ready'
    this.running -= 1
    while (this.tasks[0]?.state === 'ready') {
      const next = this.tasks.shift()!
      if (next.result && 'value' in next.result)
        this.host.onResult(next.result.value)
      else if (next.result && 'error' in next.result)
        this.host.onError(next.result.error)
    }
    this.host.onBusy(this.tasks.length > 0)
    this.pump()
  }
}
