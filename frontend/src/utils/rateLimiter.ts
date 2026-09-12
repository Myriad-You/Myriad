interface RateLimitConfig {
  maxRequests: number
  windowMs: number
}

interface RateLimitEntry {
  timestamps: number[]
  blocked: boolean
  blockedUntil?: number
}

const rateLimitStore = new Map<string, RateLimitEntry>()

const DEFAULT_CONFIGS: Record<string, RateLimitConfig> = {
  login: { maxRequests: 5, windowMs: 5 * 60 * 1000 },
  api: { maxRequests: 100, windowMs: 60 * 1000 },
  fetch: { maxRequests: 10, windowMs: 60 * 1000 },
  analysis: { maxRequests: 5, windowMs: 60 * 1000 },
}

const MAX_TRACKED_KEYS = 200

const MAX_WINDOW_MS = Math.max(
  ...Object.values(DEFAULT_CONFIGS).map((c) => c.windowMs),
)

function lastSeenAt(entry: RateLimitEntry): number {
  return entry.timestamps.at(-1) ?? 0
}

function isBlocked(entry: RateLimitEntry, now: number): boolean {
  return !!(entry.blocked && entry.blockedUntil && now < entry.blockedUntil)
}

function pruneRateLimitStore(now: number): void {
  for (const [key, entry] of rateLimitStore) {
    if (isBlocked(entry, now)) continue
    if (now - lastSeenAt(entry) > MAX_WINDOW_MS) {
      rateLimitStore.delete(key)
    }
  }

  if (rateLimitStore.size <= MAX_TRACKED_KEYS) return

  const evictable = Iterator.from(rateLimitStore.entries())
    .filter(([, entry]) => !isBlocked(entry, now))
    .toArray()
    .toSorted(([, a], [, b]) => lastSeenAt(a) - lastSeenAt(b))

  const overflow = rateLimitStore.size - MAX_TRACKED_KEYS
  for (const [key] of evictable.slice(0, overflow)) {
    rateLimitStore.delete(key)
  }
}

export function checkRateLimit(
  key: string,
  configName: keyof typeof DEFAULT_CONFIGS = 'api',
): boolean {
  const config = DEFAULT_CONFIGS[configName]
  const now = Date.now()

  let entry = rateLimitStore.get(key)

  if (!entry) {
    if (rateLimitStore.size >= MAX_TRACKED_KEYS) {
      pruneRateLimitStore(now)
    }
    entry = { timestamps: [], blocked: false }
    rateLimitStore.set(key, entry)
  }

  if (entry.blocked && entry.blockedUntil) {
    if (now < entry.blockedUntil) {
      return false
    } else {
      entry.blocked = false
      entry.blockedUntil = undefined
      entry.timestamps = []
    }
  }

  entry.timestamps = entry.timestamps.filter(
    (timestamp) => now - timestamp < config.windowMs,
  )

  if (entry.timestamps.length >= config.maxRequests) {
    entry.blocked = true
    entry.blockedUntil = now + config.windowMs
    return false
  }

  entry.timestamps.push(now)
  return true
}

export class RateLimitError extends Error {
  constructor(
    message: string,
    public retryAfter: number,
    public remaining: number = 0,
  ) {
    super(message)
    this.name = 'RateLimitError'
  }
}
