import { useEffect, useRef } from 'react'

/** A lease older than this is a generation the server lost (it restarted), as the server itself judges. */
const LEASE_MS = 15 * 60_000
/** How often a running generation is asked after. */
const WATCH_MS = 5000

/**
 * Whether a generation record (the persona's `portraitGeneration` or
 * `avatarGeneration`) says one is running now: its lease is taken and fresh.
 */
export function generationPending(record: Record<string, unknown> | null | undefined, now = Date.now()): boolean {
  const pending = record?.pending
  if (!pending || typeof pending !== 'object') return false
  const startedAt = Date.parse(String((pending as { startedAt?: unknown }).startedAt ?? ''))
  return Number.isFinite(startedAt) && now - startedAt < LEASE_MS
}

/**
 * A generation still running on the server, begun before a reload or in
 * another tab: while `pending`, the persona is read again every few seconds
 * (`reread`); once it is no longer pending, `done` takes up what it made.
 * Returns whether one is running, for the page to show it.
 */
export function useServerGeneration(pending: boolean, reread: () => Promise<void>, done: () => void): boolean {
  const watched = useRef(false)
  const latest = useRef({ reread, done })
  latest.current = { reread, done }
  useEffect(() => {
    if (pending) {
      watched.current = true
      const ask = () => { latest.current.reread().catch(() => {}) }
      const timer = setInterval(ask, WATCH_MS)
      // A tab left in the background has its timers slowed by the browser: asked at once when it is looked at again.
      const looked = () => { if (document.visibilityState === 'visible') ask() }
      document.addEventListener('visibilitychange', looked)
      return () => {
        clearInterval(timer)
        document.removeEventListener('visibilitychange', looked)
      }
    }
    if (watched.current) {
      watched.current = false
      latest.current.done()
    }
    return undefined
  }, [pending])
  return pending
}
