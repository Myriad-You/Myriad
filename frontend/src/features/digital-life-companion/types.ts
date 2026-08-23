export type CompanionActivity = 'idle' | 'thinking' | 'talking'

/** Minimal snapshot identity used by overlay polling deduplication. */
export interface CompanionSnapshot {
  fingerprint: string
}

/** Message shape consumed by the proactive unread selector. */
export interface CompanionMessage {
  role: 'user' | 'assistant' | 'system' | 'proactive'
  content: string
  meta: Record<string, unknown>
}
