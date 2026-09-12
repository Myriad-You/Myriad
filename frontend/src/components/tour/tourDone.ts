const STORAGE_KEY = 'myriad_tour_done_v1'

const listeners = new Set<() => void>()
let sessionDone: string[] = []

function persistDone(): boolean {
  return !import.meta.env.DEV
}

export function parseDoneIds(raw: string | null): string[] {
  if (!raw) return []
  try {
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) return []
    return parsed.filter((id): id is string => typeof id === 'string' && id.length > 0)
  } catch {
    return []
  }
}

export function addDoneId(ids: readonly string[], id: string): string[] {
  if (ids.includes(id)) return Iterator.from(ids).toArray()
  return [...ids, id]
}

function readRaw(): string | null {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage.getItem(STORAGE_KEY)
  } catch {
    return null
  }
}

function writeRaw(ids: string[]): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(ids))
  } catch {
  }
}

function readDoneIds(): string[] {
  if (!persistDone()) return sessionDone
  return parseDoneIds(readRaw())
}

export function isTourDone(tourId: string): boolean {
  return readDoneIds().includes(tourId)
}

export function markTourDone(tourId: string): void {
  if (!persistDone()) {
    sessionDone = addDoneId(sessionDone, tourId)
    listeners.forEach((listener) => listener())
    return
  }
  const next = addDoneId(parseDoneIds(readRaw()), tourId)
  writeRaw(next)
  listeners.forEach((listener) => listener())
}

export function subscribeTourDone(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange)
  return () => {
    listeners.delete(onStoreChange)
  }
}

export function getTourDoneSnapshot(): string {
  if (!persistDone()) return JSON.stringify(sessionDone)
  return readRaw() ?? ''
}
