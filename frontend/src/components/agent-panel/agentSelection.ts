/**
 * 用户选中的那段文字。
 *
 * 上下文里最强的一路 —— 页面正文是「你在这儿」，选区是「你指着这个」。指着的
 * 东西比在哪儿具体得多，所以它压过页面正文。
 *
 * **选区必须记住，不能只看当下。** 唤起助手要长按，而按下去那一刻浏览器就把
 * 选区收掉了；只读实时选区的话，等面板打开时想指的那段早就没了。所以这里只在
 * 选中新东西时更新，松开、点空白都不清 —— 靠时效和换页来过期。
 *
 * 只记文本，不记位置也不记来源节点：选区一变就没了，存 DOM 引用只会拿到一堆
 * 悬空节点。
 */

/** 选中这么少通常是误触或者点了一下，不算「指着什么」。 */
export const MIN_SELECTION_LENGTH = 2

/** 再长也不往上送 —— 一整页正文该走页面正文那条路。 */
export const MAX_SELECTION_LENGTH = 4_000

/** 界面上那一行显示多少字。 */
export const SELECTION_PREVIEW_LENGTH = 48

/** 记多久。过了这么久还没用上，多半已经不是他想指的那段了。 */
export const SELECTION_MEMORY_MS = 60_000

export interface AgentSelectionSnapshot {
  text: string
  capturedAtMs: number
}

const EMPTY: AgentSelectionSnapshot = { text: '', capturedAtMs: 0 }

/** 折叠空白并截断。选区常常带着排版换行，原样送上去只是噪音。 */
export function normalizeSelection(raw: string): string {
  const collapsed = raw.replace(/\s+/gu, ' ').trim()
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

/**
 * 面板自己里面的选中不算 —— 那是用户在改自己的话，不是在指页面上的东西。
 */
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

function notify(): void {
  for (const listener of listeners) listener()
}

function handleSelectionChange(): void {
  const selection = window.getSelection()
  if (!selection || selection.isCollapsed) return
  if (selectionIsFromPanel(selection.anchorNode)) return
  const text = normalizeSelection(selection.toString())
  // 收起选区不清账 —— 长按唤起时浏览器正好会收起它
  if (!selectionCounts(text) || text === current.text) return
  current = { text, capturedAtMs: Date.now() }
  notify()
}

/** 换页、用过了：明确地忘掉。 */
export function clearAgentSelection(): void {
  if (current === EMPTY) return
  current = EMPTY
  notify()
}

/** 开始盯着选区。多处调用只装一次监听，最后一个撤走时才卸掉。 */
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

export function getServerAgentSelectionSnapshot(): AgentSelectionSnapshot {
  return EMPTY
}
