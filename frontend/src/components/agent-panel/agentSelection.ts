import { authSubject } from '../../utils/authSubject'

/** Click/accidental highlight. */
export const MIN_SELECTION_LENGTH = 2

/** Full-page body belongs on the content path, not here. */
export const MAX_SELECTION_LENGTH = 4_000

export const SELECTION_PREVIEW_LENGTH = 48

export const SELECTION_MEMORY_MS = 60_000

export interface AgentSelectionSnapshot {
  text: string
  capturedAtMs: number
}

const EMPTY: AgentSelectionSnapshot = { text: '', capturedAtMs: 0 }

export function normalizeSelection(raw: string): string {
  const collapsed = raw.replaceAll(/\s+/gu, ' ').trim()
  return collapsed.length > MAX_SELECTION_LENGTH
    ? collapsed.slice(0, MAX_SELECTION_LENGTH)
    : collapsed
}

export function selectionCounts(text: string): boolean {
  return text.length >= MIN_SELECTION_LENGTH
}

export function selectionPreview(text: string): string {
  return text.length > SELECTION_PREVIEW_LENGTH
    ? `${text.slice(0, SELECTION_PREVIEW_LENGTH)}…`
    : text
}

export function selectionIsFresh(
  snapshot: AgentSelectionSnapshot,
  nowMs: number,
): boolean {
  if (!snapshot.text) return false
  return nowMs - snapshot.capturedAtMs <= SELECTION_MEMORY_MS
}

export function selectionIsFromPanel(node: Node | null | undefined): boolean {
  if (!node) return false
  const element =
    node.nodeType === 1 ? (node as Element) : node.parentElement
  return !!element?.closest('.agent-panel-overlay-anchor')
}

let current: AgentSelectionSnapshot = EMPTY
const listeners = new Set<() => void>()
let watchers = 0
let detach: (() => void) | null = null

authSubject.subscribe(clearAgentSelection)

function notify(): void {
  for (const listener of listeners) listener()
}

function handleSelectionChange(): void {
  const selection = window.getSelection()
  if (!selection || selection.isCollapsed) return
  if (selectionIsFromPanel(selection.anchorNode)) return
  const text = normalizeSelection(selection.toString())
  // collapsed selection is not a clear — long-press collapse would drop it
  if (!selectionCounts(text) || text === current.text) return
  current = { text, capturedAtMs: Date.now() }
  notify()
}

export function clearAgentSelection(): void {
  if (current === EMPTY) return
  current = EMPTY
  notify()
}

export function watchAgentSelection(): () => void {
  watchers += 1
  if (watchers === 1) {
    document.addEventListener('selectionchange', handleSelectionChange)
    detach = () => {
      document.removeEventListener('selectionchange', handleSelectionChange)
    }
    handleSelectionChange()
  }
  return () => {
    watchers -= 1
    if (watchers > 0) return
    detach?.()
    detach = null
    clearAgentSelection()
  }
}

export function subscribeAgentSelection(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function getAgentSelectionSnapshot(): AgentSelectionSnapshot {
  return current
}

export function turnSelectionText(nowMs: number = Date.now()): string | undefined {
  return selectionIsFresh(current, nowMs) ? current.text : undefined
}

export function getServerAgentSelectionSnapshot(): AgentSelectionSnapshot {
  return EMPTY
}
