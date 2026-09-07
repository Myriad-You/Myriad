/**
 * 首页人设小组件共用一个现场形象。谁先挂上谁播 WebGL，其余只出说明；
 * 持有者卸掉后租约交给队列里下一个。面板形象不走这条，它有自己的优先级。
 *
 * 租约只在 layout effect 里 claim/release：render 期间改全局队列，
 * 并发模式下被丢弃的那次 render 会把幽灵 id 留在持有者上。
 * useSyncExternalStore 在 layout 里订阅，claim 之后会在绘制前重渲成播放器，
 * 所以首屏不会先闪那句「一次只播放一个」。
 */

import { useLayoutEffect, useSyncExternalStore } from 'react'

type Listener = () => void

const listeners = new Set<Listener>()
const queue: string[] = []
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

/** 先占到的那个播。同一 id 再 claim 不会进队两次。 */
export function claimMeropeWidgetFaceSlot(id: string): () => void {
  if (enqueue(id)) emit()
  let released = false
  return () => {
    if (released) return
    released = true
    const index = queue.indexOf(id)
    if (index < 0) return
    queue.splice(index, 1)
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
