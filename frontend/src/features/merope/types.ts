export type MeropeActivity = 'idle' | 'thinking' | 'talking'

/** Minimal snapshot identity used by overlay polling deduplication. */
export interface MeropeSnapshot {
  fingerprint: string
}

/** Message shape consumed by the proactive unread selector. */
export interface MeropeMessage {
  role: 'user' | 'assistant' | 'system' | 'proactive'
  content: string
  meta: Record<string, unknown>
}
