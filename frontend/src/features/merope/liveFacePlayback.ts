import { useLayoutEffect, useSyncExternalStore } from 'react'

type Listener = () => void

interface Claim {
  priority: number
  order: number
  wanted: boolean
}

interface Snapshot {
  holder: string | null
  vacating: string | null
}

const listeners = new Set<Listener>()
const claims = new Map<string, Claim>()
let nextOrder = 0
let holder: string | null = null
let vacating: string | null = null
let snapshot: Snapshot = { holder: null, vacating: null }

function emit(): void {
  snapshot = { holder, vacating }
  for (const listener of listeners) listener()
}

function pickWanted(): string | null {
  let bestId: string | null = null
  let best: Claim | null = null
  for (const [id, claim] of claims) {
    if (!claim.wanted) continue
    if (
      !best ||
      claim.priority > best.priority ||
      (claim.priority === best.priority && claim.order < best.order)
    ) {
      best = claim
      bestId = id
    }
  }
  return bestId
}

function reconcile(): void {
  const next = pickWanted()
  if (holder === next) {
    if (vacating === null) return
    vacating = null
    emit()
    return
  }
  if (holder !== null) {
    if (vacating === holder) return
    vacating = holder
    emit()
    return
  }
  holder = next
  vacating = null
  emit()
}

export const LIVE_FACE_PLAYBACK_PRIORITY = {
  widget: 1,
  panel: 2,
} as const

export function liveFacePlaybackHolder(): string | null {
  return holder
}

export function liveFacePlaybackVacating(): string | null {
  return vacating
}

export function subscribeLiveFacePlayback(onStoreChange: Listener): () => void {
  listeners.add(onStoreChange)
  return () => {
    listeners.delete(onStoreChange)
  }
}

function getSnapshot(): Snapshot {
  return snapshot
}

export function setLiveFaceWanted(
  id: string,
  wanted: boolean,
  priority: number,
): void {
  const existing = claims.get(id)
  if (existing) {
    existing.wanted = wanted
    existing.priority = priority
  } else {
    claims.set(id, { priority, order: nextOrder, wanted })
    nextOrder += 1
  }
  reconcile()
}

export function dropLiveFaceClaim(id: string): void {
  claims.delete(id)
  if (holder !== id) {
    reconcile()
    return
  }
  holder = pickWanted()
  vacating = null
  emit()
}

export function notifyLiveFaceUnmounted(id: string): void {
  if (holder !== id) return
  const next = pickWanted()
  if (next === id) {
    if (vacating === id) {
      vacating = null
      emit()
    }
    return
  }
  holder = next
  vacating = null
  emit()
}

export function claimLiveFacePlayback(
  id: string,
  priority: number,
): () => void {
  setLiveFaceWanted(id, true, priority)
  let released = false
  return () => {
    if (released) return
    released = true
    dropLiveFaceClaim(id)
  }
}

export function resetLiveFacePlaybackForTests(): void {
  claims.clear()
  holder = null
  vacating = null
  nextOrder = 0
  snapshot = { holder: null, vacating: null }
  listeners.clear()
}

export function useLiveFacePlayback(
  id: string,
  active: boolean,
  priority: number,
): boolean {
  const current = useSyncExternalStore(
    subscribeLiveFacePlayback,
    getSnapshot,
    getSnapshot,
  )

  useLayoutEffect(() => {
    setLiveFaceWanted(id, active, priority)
  }, [id, active, priority])

  useLayoutEffect(() => {
    return () => dropLiveFaceClaim(id)
  }, [id])

  return active && current.holder === id && current.vacating !== id
}
