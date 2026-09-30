import { useLayoutEffect, useSyncExternalStore } from 'react'

type Listener = () => void

const listeners = new Set<Listener>()
let queue: string[] = []
let holder: string | null = null

function emit(): void {
  for (const listener of listeners) listener()
}

function enqueue(id: string): boolean {
  if (queue.includes(id)) return false
  queue.push(id)
  if (holder === null) holder = id
  return true
}

export function meropeWidgetFaceHolder(): string | null {
  return holder
}

export function subscribeMeropeWidgetFaceSlot(
  onStoreChange: Listener,
): () => void {
  listeners.add(onStoreChange)
  return () => {
    listeners.delete(onStoreChange)
  }
}

export function claimMeropeWidgetFaceSlot(id: string): () => void {
  if (enqueue(id)) emit()
  let released = false
  return () => {
    if (released) return
    released = true
    const index = queue.indexOf(id)
    if (index < 0) return
    queue = queue.toSpliced(index, 1)
    if (holder === id) holder = queue[0] ?? null
    emit()
  }
}

export function resetMeropeWidgetFaceSlotForTests(): void {
  queue.length = 0
  holder = null
  listeners.clear()
}

export function useMeropeWidgetFaceSlot(id: string, active: boolean): boolean {
  const current = useSyncExternalStore(
    subscribeMeropeWidgetFaceSlot,
    meropeWidgetFaceHolder,
    meropeWidgetFaceHolder,
  )

  useLayoutEffect(() => {
    if (!active) return undefined
    return claimMeropeWidgetFaceSlot(id)
  }, [id, active])

  return active && current === id
}
