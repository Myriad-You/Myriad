export interface ArticleFlags {
  is_read?: boolean
  is_starred?: boolean
}
interface Entry {
  value: ArticleFlags
  revisions: Partial<Record<keyof ArticleFlags, number>>
}

export class BrewItemState {
  private batching = false
  private revision = 0
  private entries = new Map<number, Entry>()
  private pending = new Map<number, ArticleFlags>()
  private mutationListeners = new Set<(patch: ArticleFlags) => void>()
  private listeners = new Set<() => void>()
  getSnapshot = () => this.revision
  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  subscribeMutations = (listener: (patch: ArticleFlags) => void) => {
    this.mutationListeners.add(listener)
    return () => { this.mutationListeners.delete(listener) }
  }

  private notifyMutation(patch: ArticleFlags) {
    for (const listener of this.mutationListeners) listener(patch)
  }

  private notify() {
    this.revision++
    if (!this.batching) {
      for (const listener of this.listeners) listener()
    }
  }

  clear() {
    this.entries.clear()
    this.pending.clear()
    this.notify()
  }

  preview(id: number, patch: ArticleFlags) {
    const current = this.pending.get(id) ?? {}
    this.pending.set(id, { ...current, ...this.flagPatch(patch) })
    this.notify()
  }

  discardPreview(id: number, patch: ArticleFlags) {
    if (this.dropPreview(id, patch, true)) this.notify()
  }

  commit(id: number, patch: ArticleFlags) {
    this.dropPreview(id, patch, false)
    this.observe(id, patch, this.revision)
    this.notifyMutation(patch)
  }

  private dropPreview(
    id: number,
    patch: ArticleFlags,
    matchingOnly: boolean,
  ): boolean {
    const current = this.pending.get(id)
    if (!current) return false
    const next = { ...current }
    let changed = false
    for (const field of ['is_read', 'is_starred'] as const) {
      if (typeof patch[field] !== 'boolean' || next[field] === undefined) continue
      if (matchingOnly && next[field] !== patch[field]) continue
      delete next[field]
      changed = true
    }
    if (!changed) return false
    if (next.is_read === undefined && next.is_starred === undefined) {
      this.pending.delete(id)
    } else {
      this.pending.set(id, next)
    }
    return true
  }

  private flagPatch(patch: ArticleFlags): ArticleFlags {
    const next: ArticleFlags = {}
    if (typeof patch.is_read === 'boolean') next.is_read = patch.is_read
    if (typeof patch.is_starred === 'boolean') next.is_starred = patch.is_starred
    return next
  }

  observe(id: number, patch: ArticleFlags, startedAt: number) {
    const existing = this.entries.get(id)
    const value = { ...existing?.value }
    const revisions = { ...existing?.revisions }
    let changed = false
    for (const field of ['is_read', 'is_starred'] as const) {
      if (
        typeof patch[field] !== 'boolean' ||
        (revisions[field] ?? -1) > startedAt
      ) {
        continue
}
      value[field] = patch[field]
      revisions[field] = this.revision + 1
      changed = true
    }
    if (!changed) return
    this.entries.set(id, { value, revisions })
    this.notify()
  }

  observeMany(items: Array<ArticleFlags & { id: number }>, startedAt: number) {
    const before = this.revision
    this.batching = true
    try {
      for (const item of items) this.observe(item.id, item, startedAt)
    } finally {
      this.batching = false
      if (before !== this.revision) {
        for (const listener of this.listeners) listener()
      }
    }
  }

  markAllRead() {
    for (const id of Iterator.from(this.pending.keys()).toArray()) {
      this.dropPreview(id, { is_read: true }, false)
    }
    this.observeMany(
      Iterator.from(this.entries.keys())
        .map((id) => ({ id, is_read: true }))
        .toArray(),
      this.revision,
    )
    this.notifyMutation({ is_read: true })
  }

  project<T extends { id: number }>(item: T): T {
    const flags = this.entries.get(item.id)?.value
    const preview = this.pending.get(item.id)
    if (!flags && !preview) return item
    return { ...item, ...flags, ...preview }
  }
}
export const brewItemState = new BrewItemState()
