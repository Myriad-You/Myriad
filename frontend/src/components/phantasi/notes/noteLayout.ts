/**
 * 笔记正文分栏 / 正文小组件的原文形态。
 *
 * 写栏和可视层共用这一套，发布前再由后端渲成消毒 HTML。
 * 不是首页宫格，也不是 Notion 导入的分栏。
 */

import type { WidgetType } from '../../widgetGridTypes'
import { WIDGET_SIZE_KEYS } from '../../../utils/widgetSizeScale'

export const NOTE_MAX_COLUMNS = 4

/** 工具条和原文合法键跟宫格同一份，和后端 layout.rs 对齐。 */
export const NOTE_WIDGET_SIZES = WIDGET_SIZE_KEYS

/** 原文 JSON 和 `data-config` 解码后的上限。再长就丢掉，不当配置。 */
export const NOTE_WIDGET_CONFIG_MAX = 2048

export type NoteWidgetConfig = Record<string, unknown>

const COLUMNS_OPEN = /^:::columns\s*$/
const COLUMN_MARK = /^:::col\s*$/
const FENCE_CLOSE = /^:::\s*$/

export interface MdFence { ch: '`' | '~'; n: number }

export function mdFenceOpen(line: string): MdFence | null {
  const t = line.replace(/^ {0,3}/, '')
  const ch = t[0]
  if (ch !== '`' && ch !== '~') return null
  let n = 0
  while (t[n] === ch) n += 1
  if (n < 3) return null
  if (ch === '`' && t.slice(n).includes('`')) return null
  return { ch, n }
}

export function mdFenceClose(line: string, open: MdFence): boolean {
  const t = line.replace(/^ {0,3}/, '')
  let n = 0
  while (t[n] === open.ch) n += 1
  return n >= open.n && n >= 3 && t.slice(n).trim() === ''
}

export function eatMdFence(line: string, fence: { current: MdFence | null }): boolean {
  if (fence.current) {
    if (mdFenceClose(line, fence.current)) fence.current = null
    return true
  }
  const open = mdFenceOpen(line)
  if (!open) return false
  fence.current = open
  return true
}

export type NoteLayoutSegment =
  | { kind: 'text'; text: string; start: number }
  | { kind: 'columns'; columns: string[]; start: number; end: number }
  | {
      kind: 'widget'
      type: string
      size: string
      config: NoteWidgetConfig | null
      start: number
      end: number
    }

export function normalizeNoteWidgetSize(size: string | undefined): string {
  if (size && (WIDGET_SIZE_KEYS as readonly string[]).includes(size)) return size
  return '2x2'
}

export function parseWidgetConfig(
  raw: string | null | undefined,
): NoteWidgetConfig | null {
  if (!raw) return null
  const text = raw.trim()
  if (!text || text.length > NOTE_WIDGET_CONFIG_MAX) return null
  try {
    const value: unknown = JSON.parse(text)
    if (!value || typeof value !== 'object' || Array.isArray(value)) return null
    const record = value as NoteWidgetConfig
    return Object.keys(record).length > 0 ? record : null
  } catch {
    return null
  }
}

export function serializeWidgetConfigJson(
  config: NoteWidgetConfig | null | undefined,
): string | null {
  if (!config) return null
  try {
    const json = JSON.stringify(config)
    if (json === '{}' || json.length > NOTE_WIDGET_CONFIG_MAX) return null
    return json
  } catch {
    return null
  }
}

export function encodeWidgetConfigAttr(
  config: NoteWidgetConfig | null | undefined,
): string | null {
  const json = serializeWidgetConfigJson(config)
  return json ? encodeURIComponent(json) : null
}

function unescapeHtmlAttr(value: string): string {
  return value
    .replaceAll('&quot;', '"')
    .replaceAll('&#34;', '"')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&amp;', '&')
}

/** 声明式 settings，或首页那套长按自定义面板。 */
export function noteWidgetCanConfigure(widget: WidgetType | undefined): boolean {
  if (!widget) return false
  if (widget.settings?.length) return true
  const id = widget.id
  return (
    id === 'game-presence' ||
    id === 'social-network' ||
    id === 'tapp-shortcut' ||
    id.startsWith('report-')
  )
}

/** `data-config`：先按 URI 解码，再认原始 JSON / HTML 实体。 */
export function decodeWidgetConfigAttr(
  raw: string | null | undefined,
): NoteWidgetConfig | null {
  if (!raw) return null
  const text = unescapeHtmlAttr(raw.trim())
  if (!text) return null
  try {
    return parseWidgetConfig(decodeURIComponent(text))
  } catch {
    return parseWidgetConfig(text)
  }
}

function isWidgetType(typ: string): boolean {
  return /^[a-z][a-z0-9._-]*$/.test(typ) && typ.length <= 64
}

export function parseWidgetDirective(line: string): {
  type: string
  size: string
  config: NoteWidgetConfig | null
} | null {
  const trimmed = line.replace(/[ \t]+$/, '')
  if (!trimmed.toLowerCase().startsWith(':::widget')) return null
  const after = trimmed.slice(':::widget'.length)
  if (after.length > 0 && !/^[ \t]/.test(after)) return null

  const rest = after.replace(/^[ \t]+/, '')
  const typeMatch = /^([A-Z][\w.-]*)/i.exec(rest)
  if (!typeMatch) return null
  const type = typeMatch[1]!.toLowerCase()
  if (!isWidgetType(type)) return null

  let remain = rest.slice(typeMatch[1]!.length)
  if (remain.length > 0 && !/^[ \t]/.test(remain)) return null
  remain = remain.replace(/^[ \t]+/, '')

  let sizeRaw: string | undefined
  const sizeMatch = /^(\d+x\d+)/.exec(remain)
  if (sizeMatch) {
    const afterSize = remain.slice(sizeMatch[1]!.length)
    if (afterSize.length === 0 || /^[ \t]/.test(afterSize)) {
      sizeRaw = sizeMatch[1]
      remain = afterSize.replace(/^[ \t]+/, '')
    }
  }

  let config: NoteWidgetConfig | null = null
  if (remain) {
    if (!remain.startsWith('{')) return null
    config = parseWidgetConfig(remain)
  }

  return {
    type,
    size: normalizeNoteWidgetSize(sizeRaw),
    config,
  }
}

export function noteWidgetTypesInMarkdown(markdown: string): string[] {
  const types: string[] = []
  const seen = new Set<string>()
  for (const seg of parseNoteLayout(markdown)) {
    if (seg.kind !== 'widget' || seen.has(seg.type)) continue
    seen.add(seg.type)
    types.push(seg.type)
  }
  return types
}

export function insertWidgetMarkdown(
  type: string,
  size = '2x2',
  config: NoteWidgetConfig | null = null,
): string {
  const widget = type.trim().toLowerCase()
  const line = `:::widget ${widget} ${normalizeNoteWidgetSize(size)}`
  const json = serializeWidgetConfigJson(config)
  return json ? `${line} ${json}` : line
}

export function insertColumnsMarkdown(count = 2): string {
  const n = Math.min(NOTE_MAX_COLUMNS, Math.max(2, count))
  const cols = Array.from({ length: n }, () => '')
  return serializeColumns(cols)
}

export function serializeColumns(columns: string[]): string {
  const cols = columns.length > 0 ? columns : ['']
  const bodies = cols.map((col) => col.replaceAll(/\r\n/g, '\n').trimEnd())
  return `:::columns\n${bodies[0]}\n${bodies
    .slice(1)
    .map((col) => `:::col\n${col}\n`)
    .join('')}:::`
}

export function serializeWidget(
  type: string,
  size: string,
  config: NoteWidgetConfig | null = null,
): string {
  return insertWidgetMarkdown(type, size, config)
}

/**
 * 按行切开分栏和小组件。代码围栏里的 `:::` 不动。
 * `start` / `end` 是规范化成 `\n` 之后的下标。
 */
export function parseNoteLayout(markdown: string): NoteLayoutSegment[] {
  const src = markdown.replaceAll('\r\n', '\n')
  const lines = src.split('\n')
  const segments: NoteLayoutSegment[] = []
  let index = 0
  let offset = 0
  let textStart = 0
  const textLines: string[] = []
  const fence = { current: null as MdFence | null }

  const lineEnd = (line: string, last: boolean) =>
    offset + line.length + (last ? 0 : 1)

  const flushText = (end: number) => {
    if (textLines.length === 0) {
      textStart = end
      return
    }
    const text = textLines.join('\n')
    textLines.length = 0
    if (text.trim()) segments.push({ kind: 'text', text, start: textStart })
    textStart = end
  }

  while (index < lines.length) {
    const line = lines[index]!
    const last = index === lines.length - 1
    const next = lineEnd(line, last)

    if (eatMdFence(line, fence)) {
      textLines.push(line)
      index += 1
      offset = next
      continue
    }

    const widget = parseWidgetDirective(line)
    if (widget) {
      flushText(offset)
      segments.push({
        kind: 'widget',
        type: widget.type,
        size: widget.size,
        config: widget.config,
        start: offset,
        end: offset + line.length,
      })
      textStart = next
      index += 1
      offset = next
      continue
    }

    if (COLUMNS_OPEN.test(line)) {
      flushText(offset)
      const parsed = takeColumns(lines, index, offset)
      segments.push(parsed.segment)
      index = parsed.nextIndex
      offset = parsed.nextOffset
      textStart = offset
      continue
    }

    textLines.push(line)
    index += 1
    offset = next
  }

  flushText(src.length)
  return segments
}

function takeColumns(
  lines: string[],
  startIndex: number,
  startOffset: number,
): { segment: NoteLayoutSegment; nextIndex: number; nextOffset: number } {
  let index = startIndex + 1
  let offset = startOffset + lines[startIndex]!.length
  if (startIndex < lines.length - 1) offset += 1

  const columns: string[][] = [[]]
  const fence = { current: null as MdFence | null }

  while (index < lines.length) {
    const line = lines[index]!
    const last = index === lines.length - 1
    const next = offset + line.length + (last ? 0 : 1)

    const inFence = eatMdFence(line, fence)

    if (!inFence && COLUMN_MARK.test(line)) {
      columns.push([])
      index += 1
      offset = next
      continue
    }
    if (!inFence && FENCE_CLOSE.test(line)) {
      return {
        segment: {
          kind: 'columns',
          columns: columns.map((col) => col.join('\n')),
          start: startOffset,
          end: offset + line.length,
        },
        nextIndex: index + 1,
        nextOffset: next,
      }
    }

    columns[columns.length - 1]!.push(line)
    index += 1
    offset = next
  }

  return {
    segment: {
      kind: 'columns',
      columns: columns.map((col) => col.join('\n')),
      start: startOffset,
      end: offset,
    },
    nextIndex: index,
    nextOffset: offset,
  }
}

export function layoutAttr(
  attrs: string,
  name: 'class' | 'data-widget' | 'data-size' | 'data-config',
): string | null {
  const match = new RegExp(
    `(?:^|\\s)${name}="([^"]*)"`,
    'i',
  ).exec(attrs)
  return match?.[1] ?? null
}

export function hasNoteClass(attrs: string, className: string): boolean {
  const value = layoutAttr(attrs, 'class')
  if (!value) return false
  return value.split(/\s+/).includes(className)
}
