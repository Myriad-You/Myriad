/**
 * 全站现场形象只跑一套 WebGL。面板聊天档优先于首页小组件；
 * 没拿到租约的表面只出说明，不另开播放器，也不用主立绘占位。
 *
 * 和小组件张数租约是两件事：多张卡片里只有一张是「那张卡」，
 * 这张卡和面板之间还要再争一次播放权。
 *
 * 播放权易手时，现持有者先卸掉播放器（退场用最后一帧平面影像），
 * 下一处才挂上 WebGL 入场。两处同时想要时也不并行开第二套现场。
 */

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

/**
 * 现持有者已经卸掉现场播放器。换包装时仍是赢家就留下租约；
 * 否则把播放权交给下一个。
 */
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
