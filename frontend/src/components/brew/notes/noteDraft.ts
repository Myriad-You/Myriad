/**
 * 编辑器的本地草稿。
 *
 * 第一版没有服务端草稿 —— 加 `status` 列会牵动所有列表查询，那是第二版的事。
 * 在此之前，正在编辑的内容存在这台浏览器里，刷新、误关标签页都能捡回来。
 *
 * 这份草稿只属于这台浏览器上的这个人：不同步、不发给服务端、不进任何载荷。
 */

/** 草稿在 localStorage 里的键。`new` 是「还没发布的那篇」。 */
export function noteDraftKey(id: number | 'new'): string {
  return `brew:note-draft:${id}`
}

export interface NoteDraft {
  title: string
  contentMd: string
  /** 写入时刻（毫秒）。过期草稿靠它判断。 */
  savedAt: number
}

/** 草稿多久之后不再自动恢复。超过就当没有。 */
export const NOTE_DRAFT_TTL_MS = 7 * 24 * 60 * 60 * 1000

export function readNoteDraft(
  id: number | 'new',
  now: number = Date.now(),
): NoteDraft | null {
  try {
    const raw = globalThis.localStorage?.getItem(noteDraftKey(id))
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<NoteDraft>
    if (typeof parsed.contentMd !== 'string') return null
    const savedAt = Number(parsed.savedAt)
    if (!Number.isFinite(savedAt)) return null
    if (now - savedAt > NOTE_DRAFT_TTL_MS) return null
    return {
      title: typeof parsed.title === 'string' ? parsed.title : '',
      contentMd: parsed.contentMd,
      savedAt,
    }
  } catch {
    // 隐私模式 / 站点数据被禁用时读 localStorage 会抛
    return null
  }
}

export function writeNoteDraft(
  id: number | 'new',
  draft: Omit<NoteDraft, 'savedAt'>,
  now: number = Date.now(),
): void {
  try {
    globalThis.localStorage?.setItem(
      noteDraftKey(id),
      JSON.stringify({ ...draft, savedAt: now }),
    )
  } catch {
    // 写不进去就不写。草稿丢失不该挡住编辑本身。
  }
}

export function clearNoteDraft(id: number | 'new'): void {
  try {
    globalThis.localStorage?.removeItem(noteDraftKey(id))
  } catch {
    /* 同上 */
  }
}

/**
 * 草稿与已保存内容是否真的不同。
 *
 * 只比正文和标题，不比时间戳 —— 否则每次自动保存都会显示「有未保存改动」。
 * 首尾空白不算差异：编辑器结束时也会 trim。
 */
export function draftDiffersFrom(
  draft: NoteDraft | null,
  saved: { title: string; contentMd: string },
): boolean {
  if (!draft) return false
  return (
    draft.title.trim() !== saved.title.trim() ||
    draft.contentMd.trim() !== saved.contentMd.trim()
  )
}

/** 光标处插入 Markdown 标记后的新状态。工具栏每个按钮都走这一个函数。 */
export interface WrapResult {
  value: string
  /** 插入后光标（或选区）的新位置。 */
  selectionStart: number
  selectionEnd: number
}

/**
 * 用 `before` / `after` 包住选区。没有选区时插入 `placeholder` 并把它选中，
 * 这样按下按钮就能直接打字覆盖。
 */
export function wrapSelection(
  value: string,
  start: number,
  end: number,
  before: string,
  after: string,
  placeholder = '',
): WrapResult {
  const selected = value.slice(start, end)
  const body = selected || placeholder
  const next = value.slice(0, start) + before + body + after + value.slice(end)
  return {
    value: next,
    selectionStart: start + before.length,
    selectionEnd: start + before.length + body.length,
  }
}

/**
 * 在选区所在的每一行前面加前缀（标题、引用、列表）。
 *
 * 按行处理而不是按选区包裹 —— `## ` 加在选区中间不产生标题，只产生一段
 * 带井号的普通文字。已经有同样前缀的行会被去掉前缀，按钮因此是可切换的。
 */
export function prefixLines(
  value: string,
  start: number,
  end: number,
  prefix: string,
): WrapResult {
  const lineStart = value.lastIndexOf('\n', start - 1) + 1
  // 选区正好停在换行符之后时，用户选的是「到上一行为止」，不该把下一行也带上。
  // 少了这一步，选中「甲\n乙\n」会连丙一起加上前缀。
  const scanFrom = end > start && value[end - 1] === '\n' ? end - 1 : end
  const lineEndIndex = value.indexOf('\n', scanFrom)
  const lineEnd = lineEndIndex === -1 ? value.length : lineEndIndex
  const block = value.slice(lineStart, lineEnd)
  const lines = block.split('\n')
  const allPrefixed = lines.every((l) => l.startsWith(prefix))
  const next = lines
    .map((l) => (allPrefixed ? l.slice(prefix.length) : prefix + l))
    .join('\n')
  return {
    value: value.slice(0, lineStart) + next + value.slice(lineEnd),
    selectionStart: lineStart,
    selectionEnd: lineStart + next.length,
  }
}
