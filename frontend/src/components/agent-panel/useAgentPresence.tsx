/**
 * 进出场包装：列表项、贴、同一位置上换内容，都走同一套 data-presence。
 *
 * 真正的位移/淡出在 CSS 里。这里只负责「卸之前多留一帧」。
 * 文件名不能叫 AgentPresence.tsx —— Vite 在大小写不敏感的盘上会先命中
 * 同名的 .ts（进出场记账），浏览器就拿不到这些组件。
 */

import type { CSSProperties, ReactNode } from 'react'
import type { PresenceEntry } from './agentPresenceState'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { AGENT_ROW_STAGGER_MAX } from './agentPanelStage'
import {
  AGENT_ROW_MS,
  AGENT_SWAP_MS,
  presenceEqual,
  presenceNextDrop,
  readPresenceDuration,
  reconcilePresence,
} from './agentPresenceState'

export type AgentPresenceKind = 'row' | 'chip' | 'swap'

/** 从哪来回哪去：进出共用这个原点。 */
export type AgentPresenceFrom = 'composer' | 'attach' | 'context' | 'self'

export function useKeyedPresence<T>(
  items: readonly T[],
  keyOf: (item: T) => string,
  durationMs = AGENT_ROW_MS,
): PresenceEntry<T>[] {
  const keyOfRef = useRef(keyOf)
  keyOfRef.current = keyOf
  const itemsRef = useRef(items)
  itemsRef.current = items
  const signature = items.map(keyOf).join('\0')
  const [entries, setEntries] = useState<PresenceEntry<T>[]>(() =>
    reconcilePresence([], items, keyOf, 0, 0),
  )

  useLayoutEffect(() => {
    const now = performance.now()
    const duration = readPresenceDuration(durationMs)
    setEntries((prev) => {
      const next = reconcilePresence(
        prev,
        itemsRef.current,
        keyOfRef.current,
        now,
        duration,
      )
      return presenceEqual(prev, next) ? prev : next
    })
  }, [durationMs, signature])

  useEffect(() => {
    const wait = presenceNextDrop(entries, performance.now())
    if (wait === null) return undefined
    const timer = setTimeout(() => {
      const now = performance.now()
      const duration = readPresenceDuration(durationMs)
      setEntries((prev) => {
        const next = reconcilePresence(
          prev,
          itemsRef.current,
          keyOfRef.current,
          now,
          duration,
        )
        return presenceEqual(prev, next) ? prev : next
      })
    }, wait)
    return () => clearTimeout(timer)
  }, [durationMs, entries])

  const latest = new Map<string, T>()
  for (const item of items) latest.set(keyOf(item), item)
  return entries.map((entry) => {
    if (entry.phase === 'out') return entry
    const item = latest.get(entry.key)
    if (item === undefined || item === entry.item) return entry
    return { ...entry, item }
  })
}

function PresenceBox({
  kind,
  from,
  phase,
  appear = true,
  stagger = 0,
  children,
}: {
  kind: AgentPresenceKind
  from: AgentPresenceFrom
  phase: PresenceEntry<unknown>['phase']
  appear?: boolean
  stagger?: number
  children: ReactNode
}) {
  return (
    <div
      className="agent-panel-presence"
      data-kind={kind}
      data-from={from}
      data-presence={phase}
      data-appear={appear ? undefined : 'skip'}
      style={{ '--agent-stagger': stagger } as CSSProperties}
    >
      {children}
    </div>
  )
}

export function AgentPresence({
  open,
  kind = 'row',
  from = 'composer',
  durationMs = AGENT_ROW_MS,
  children,
}: {
  open: boolean
  kind?: AgentPresenceKind
  from?: AgentPresenceFrom
  durationMs?: number
  children: ReactNode
}) {
  const items = open ? [{ id: 'on' as const, children }] : []
  const entries = useKeyedPresence(items, (item) => item.id, durationMs)
  if (entries.length === 0) return null
  return (
    <>
      {entries.map((entry) => (
        <PresenceBox
          key={entry.key}
          kind={kind}
          from={from}
          phase={entry.phase}
        >
          {entry.item.children}
        </PresenceBox>
      ))}
    </>
  )
}

export function AgentSwap({
  id,
  kind = 'swap',
  from = 'self',
  appear = true,
  durationMs = AGENT_SWAP_MS,
  children,
}: {
  id: string
  kind?: AgentPresenceKind
  from?: AgentPresenceFrom
  appear?: boolean
  durationMs?: number
  children: ReactNode
}) {
  const entries = useKeyedPresence(
    [{ id, children }],
    (item) => item.id,
    durationMs,
  )
  return (
    <div className="agent-panel-swap" data-from={from}>
      {entries.map((entry) => (
        <PresenceBox
          key={entry.key}
          kind={kind}
          from={from}
          phase={entry.phase}
          appear={appear}
        >
          {entry.item.children}
        </PresenceBox>
      ))}
    </div>
  )
}

export function AgentPresenceList<T>({
  items,
  keyOf,
  kind = 'row',
  from = 'composer',
  children,
}: {
  items: readonly T[]
  keyOf: (item: T) => string
  kind?: AgentPresenceKind
  from?: AgentPresenceFrom
  children: (item: T) => ReactNode
}) {
  const entries = useKeyedPresence(items, keyOf, AGENT_ROW_MS)
  const last = entries.length - 1
  const staggerFor = useRef(new Map<string, number>())
  const live = new Set(entries.map((entry) => entry.key))
  for (const key of staggerFor.current.keys()) {
    if (!live.has(key)) staggerFor.current.delete(key)
  }
  const batch = staggerFor.current.size === 0
  return (
    <>
      {entries.map((entry, index) => {
        let stagger = staggerFor.current.get(entry.key)
        if (stagger === undefined) {
          stagger = batch
            ? Math.min(Math.max(last - index, 0), AGENT_ROW_STAGGER_MAX) + 1
            : 0
          staggerFor.current.set(entry.key, stagger)
        }
        return (
          <PresenceBox
            key={entry.key}
            kind={kind}
            from={from}
            phase={entry.phase}
            stagger={stagger}
          >
            {children(entry.item)}
          </PresenceBox>
        )
      })}
    </>
  )
}
