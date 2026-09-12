const STORAGE_KEY = 'brew:progress-outbox'

export interface ProgressPersist {
  load: () => { progress: number; observedAt: number } | null
  save: (progress: number, observedAt: number) => void
  clear: () => void
}

interface ProgressOutboxEntry {
  subjectKey: string
  generation: number
  itemId: number
  progress: number
  observedAt: number
}

function readAll(storage: Storage): ProgressOutboxEntry[] {
  try {
    const raw = storage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) return []
    return parsed.filter(isEntry)
  } catch {
    return []
  }
}

function isEntry(value: unknown): value is ProgressOutboxEntry {
  if (!value || typeof value !== 'object') return false
  const entry = value as ProgressOutboxEntry
  return (
    typeof entry.subjectKey === 'string' &&
    Number.isInteger(entry.generation) &&
    Number.isInteger(entry.itemId) &&
    entry.itemId > 0 &&
    Number.isFinite(entry.progress) &&
    Number.isFinite(entry.observedAt)
  )
}

function writeAll(storage: Storage, entries: ProgressOutboxEntry[]): void {
  storage.setItem(STORAGE_KEY, JSON.stringify(entries))
}

export function progressOutbox(
  storage: Storage,
  subjectKey: string,
  generation: number,
  itemId: number,
): ProgressPersist {
  const belongs = (entry: ProgressOutboxEntry) =>
    entry.subjectKey === subjectKey &&
    entry.generation === generation &&
    entry.itemId === itemId

  return {
    load() {
      const hit = readAll(storage).find(belongs)
      return hit
        ? { progress: hit.progress, observedAt: hit.observedAt }
        : null
    },
    save(progress, observedAt) {
      const rest = readAll(storage).filter(
        (entry) =>
          entry.subjectKey === subjectKey &&
          entry.generation === generation &&
          entry.itemId !== itemId,
      )
      rest.push({
        subjectKey,
        generation,
        itemId,
        progress,
        observedAt,
      })
      writeAll(storage, rest)
    },
    clear() {
      writeAll(
        storage,
        readAll(storage).filter(
          (entry) =>
            entry.subjectKey === subjectKey &&
            entry.generation === generation &&
            entry.itemId !== itemId,
        ),
      )
    },
  }
}
