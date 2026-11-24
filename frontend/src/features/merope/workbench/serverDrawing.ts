/**
 * Drawings the server makes on its own (gesture arms, expression redraws): a
 * request only asks for one and is answered once the server has taken it up.
 * What became of it is read off the list: the ones being drawn, the ones kept,
 * and the ones that were not kept with what went wrong.
 */

/** How the page watches the server's list of them. */
export interface DrawingWatch {
  /** Resolves once the list read last names none being drawn. */
  settled: () => Promise<void>
  /** Reads the list again. */
  reread: () => Promise<void>
  /** The ones the list read last names as being drawn. */
  drawing: () => readonly unknown[]
  /** The page shows another picture by now: these are no longer waited for. */
  gone?: () => boolean
}

/** As long as a request that waited for its drawing was given: past it, the page goes on with what the list has. */
export const DRAWN_WITHIN_MS = 15 * 60_000

/**
 * Waits until the server draws none any more, `within` at the longest. The
 * list that ended a wait may have been read before the server took the last
 * ones up, so it is read once more, and waited on again if it names some. A
 * read that fails leaves the list as it was read last.
 */
export async function untilNoneDrawn(watch: DrawingWatch, within = DRAWN_WITHIN_MS): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined
  const late = new Promise<'late'>((resolve) => {
    timer = setTimeout(resolve, within, 'late')
  })
  try {
    do {
      if (await Promise.race([watch.settled(), late]) === 'late') return
      if (watch.gone?.()) return
      await watch.reread().catch(() => {})
    } while (watch.drawing().length > 0 && !watch.gone?.())
  } finally {
    clearTimeout(timer)
  }
}

/**
 * Why each drawing asked for did not come to be, in the order they were asked
 * (none: all were kept). `begun` is how each ask was answered; `before` and
 * `after` are each one's kept picture when it was asked and now; `failed` is
 * what the list says went wrong.
 */
export function drawingFailures<K extends string>(
  asked: readonly K[],
  begun: ReadonlyArray<PromiseSettledResult<unknown>>,
  kept: { before: (key: K) => string | undefined; after: (key: K) => string | undefined },
  failed: Partial<Record<K, unknown>>,
  lost: () => unknown,
): unknown[] {
  return asked.flatMap((key, index) => {
    const answer = begun[index]
    // Not taken up at all: what the server answered.
    if (!answer || answer.status === 'rejected') return [answer?.reason ?? lost()]
    // Taken up and not kept: what went wrong, as the list hands it on.
    if (failed[key]) return [failed[key]]
    // Neither a new picture nor a reason: the server lost it (it restarted meanwhile).
    const now = kept.after(key)
    return now && now !== kept.before(key) ? [] : [lost()]
  })
}
