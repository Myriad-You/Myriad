export type PerceptionKind =
  | 'page'
  | 'pointer'
  | 'surface'
  | 'music'
  | 'voice'
  | 'presence'
  | 'screen'
export type PerceptionPrivacy = 'local' | 'consented' | 'system'

export interface PerceptionSnapshot {
  sourceId: string
  kind: PerceptionKind
  revision: number
  capturedAt: number
  expiresAt: number
  /** Remaining life at list time. Server expiry uses this, not expiresAt. */
  ttlMs: number
  summary: string
  safeFacts: Record<string, string | number | boolean>
  privacy: PerceptionPrivacy
}

export const KINDS: readonly PerceptionKind[] = [
  'page',
  'pointer',
  'surface',
  'music',
  'voice',
  'presence',
  'screen',
]
const KIND_ORDER: readonly PerceptionKind[] = KINDS
const MAX_SUMMARY = 400
const MAX_FACTS = 12
/** Must match `perception_view::MAX_PERCEPTION_ITEMS`. */
export const MAX_PERCEPTION_ITEMS = 12

export class PerceptionRegistry {
  private readonly items = new Map<string, PerceptionSnapshot>()
  private readonly revisions = new Map<string, number>()

  replace(
    input: Omit<PerceptionSnapshot, 'revision' | 'capturedAt' | 'ttlMs'> & {
      revision?: number
      capturedAt?: number
    },
  ): PerceptionSnapshot {
    const capturedAt = input.capturedAt ?? Date.now()
    const revision =
      input.revision ?? (this.revisions.get(input.sourceId) ?? 0) + 1
    this.revisions.set(input.sourceId, revision)
    const snapshot: PerceptionSnapshot = {
      sourceId: input.sourceId.slice(0, 80),
      kind: KINDS.includes(input.kind) ? input.kind : 'presence',
      revision,
      capturedAt,
      expiresAt: input.expiresAt,
      ttlMs: Math.max(0, input.expiresAt - capturedAt),
      summary: input.summary.trim().slice(0, MAX_SUMMARY),
      safeFacts: boundFacts(input.safeFacts),
      privacy: input.privacy,
    }
    this.items.set(snapshot.sourceId, snapshot)
    return snapshot
  }

  forget(sourceId: string): void {
    this.items.delete(sourceId)
  }

  active(nowMs: number = Date.now()): PerceptionSnapshot[] {
    const live: PerceptionSnapshot[] = []
    for (const [id, snapshot] of this.items) {
      if (snapshot.expiresAt <= nowMs) {
        this.items.delete(id)
        continue
      }
      live.push({
        ...snapshot,
        ttlMs: Math.max(0, snapshot.expiresAt - nowMs),
      })
    }
    return live.sort(
      (left, right) =>
        KIND_ORDER.indexOf(left.kind) - KIND_ORDER.indexOf(right.kind),
    )
  }
}

function boundFacts(
  facts: Record<string, string | number | boolean>,
): Record<string, string | number | boolean> {
  const out: Record<string, string | number | boolean> = {}
  let count = 0
  for (const [key, value] of Object.entries(facts)) {
    if (count >= MAX_FACTS) break
    if (typeof value === 'string') out[key.slice(0, 40)] = value.slice(0, 120)
    else out[key.slice(0, 40)] = value
    count += 1
  }
  return out
}

export const perceptionRegistry = new PerceptionRegistry()
