/**
 * 助手回复里那点 Markdown。
 *
 * 从旧面板那段渲染代码里把「认字」和「画出来」拆开：这里只负责认字，产出一棵
 * 纯数据的树，画由组件去做。拆开的理由很实际 —— 边界情况（没闭合的代码块、
 * 少一列的表格、`**` 跨行）全在认字这一步，而认字可以脱离浏览器测。
 *
 * 支持的语法与旧面板保持一致，不多也不少：代码块、1–3 级标题、水平线、引用、
 * 表格、有序/无序列表、段落；行内是代码、图片、链接、粗体、斜体。**没打算做成
 * 通用 Markdown 实现** —— 这是助手说话的格式，不是文档系统。
 */

const INLINE_RE =
  /(`[^`]+`)|!\[([^\]]*)\]\(([^)]+)\)|\[([^\]]+)\]\(([^)]+)\)|\*\*(.+?)\*\*|\*([^*]+)\*/g
const HEADING_RE = /^(#{1,3})\s+(.+)/
const UL_RE = /^\s*[-*+]\s+/
const OL_RE = /^\s*\d+\.\s+/
const BLOCKQUOTE_RE = /^>\s?(.*)/
const HR_RE = /^(?:-{3,}|_{3,}|\*{3,})\s*$/
const TABLE_SEP_RE = /^\|[\s:|-]+\|$/

export type InlineToken =
  | { kind: 'text'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'image'; url: string; alt: string }
  | { kind: 'link'; url: string; text: string }
  | { kind: 'bold'; text: string }
  | { kind: 'italic'; text: string }

export type MarkdownBlock =
  | { kind: 'paragraph'; inline: InlineToken[] }
  | { kind: 'heading'; level: 1 | 2 | 3; inline: InlineToken[] }
  | { kind: 'code'; lang: string | null; text: string }
  | { kind: 'quote'; lines: InlineToken[][] }
  | { kind: 'list'; ordered: boolean; items: InlineToken[][] }
  | { kind: 'table'; headers: InlineToken[][]; rows: InlineToken[][][] }
  | { kind: 'rule' }

export function parseInlineTokens(text: string): InlineToken[] {
  const tokens: InlineToken[] = []
  // 正则带 g，复用同一个实例会把 lastIndex 带到下一次调用里
  const inlineRe = new RegExp(INLINE_RE.source, INLINE_RE.flags)
  let lastIndex = 0

  for (
    let match = inlineRe.exec(text);
    match !== null;
    match = inlineRe.exec(text)
  ) {
    if (match.index > lastIndex) {
      tokens.push({ kind: 'text', text: text.slice(lastIndex, match.index) })
    }
    if (match[1]) {
      tokens.push({ kind: 'code', text: match[1].slice(1, -1) })
    } else if (match[3]) {
      tokens.push({ kind: 'image', url: match[3], alt: match[2] || '' })
    } else if (match[4] && match[5]) {
      tokens.push({ kind: 'link', url: match[5], text: match[4] })
    } else if (match[6]) {
      tokens.push({ kind: 'bold', text: match[6] })
    } else if (match[7]) {
      tokens.push({ kind: 'italic', text: match[7] })
    }
    lastIndex = match.index + match[0].length
  }
  if (lastIndex < text.length) {
    tokens.push({ kind: 'text', text: text.slice(lastIndex) })
  }
  return tokens
}

function parseRow(row: string): string[] {
  return row
    .split('|')
    .slice(1, -1)
    .map((cell) => cell.trim())
}

interface BlockRange {
  start: number
  end: number
}

interface ParseCache {
  source: string
  lines: string[]
  blocks: MarkdownBlock[]
  ranges: BlockRange[]
}

let cache: ParseCache = {
  source: '',
  lines: [],
  blocks: [],
  ranges: [],
}

function parseBlocksFrom(
  lines: string[],
  start: number,
): { blocks: MarkdownBlock[]; ranges: BlockRange[] } {
  const blocks: MarkdownBlock[] = []
  const ranges: BlockRange[] = []
  let i = start

  const push = (block: MarkdownBlock, from: number) => {
    blocks.push(block)
    ranges.push({ start: from, end: i })
  }

  while (i < lines.length) {
    const lineStart = i
    const line = lines[i]
    if (line === undefined) break

    // 代码块。流式回复里经常只到了开头那三个反引号，没闭合也要当代码块收下，
    // 否则半截代码会被当成段落，一边流一边变形。
    if (line.trimStart().startsWith('```')) {
      const lang = line.trimStart().slice(3).trim()
      const codeLines: string[] = []
      i += 1
      while (i < lines.length && !lines[i].trimStart().startsWith('```')) {
        codeLines.push(lines[i])
        i += 1
      }
      i += 1
      push(
        {
          kind: 'code',
          lang: lang || null,
          text: codeLines.join('\n'),
        },
        lineStart,
      )
      continue
    }

    const heading = line.match(HEADING_RE)
    if (heading) {
      i += 1
      push(
        {
          kind: 'heading',
          level: heading[1].length as 1 | 2 | 3,
          inline: parseInlineTokens(heading[2]),
        },
        lineStart,
      )
      continue
    }

    if (HR_RE.test(line)) {
      i += 1
      push({ kind: 'rule' }, lineStart)
      continue
    }

    if (BLOCKQUOTE_RE.test(line)) {
      const quoted: InlineToken[][] = []
      while (i < lines.length) {
        const match = lines[i].match(BLOCKQUOTE_RE)
        if (!match) break
        quoted.push(parseInlineTokens(match[1]))
        i += 1
      }
      push({ kind: 'quote', lines: quoted }, lineStart)
      continue
    }

    // 表格必须有分隔行才算表格，否则一句带竖线的话会被排成表
    if (
      line.includes('|') &&
      line.trim().startsWith('|') &&
      i + 1 < lines.length &&
      TABLE_SEP_RE.test(lines[i + 1].trim())
    ) {
      const headers = parseRow(line).map(parseInlineTokens)
      i += 2
      const rows: InlineToken[][][] = []
      while (
        i < lines.length &&
        lines[i].trim().startsWith('|') &&
        lines[i].trim().endsWith('|')
      ) {
        rows.push(parseRow(lines[i]).map(parseInlineTokens))
        i += 1
      }
      push({ kind: 'table', headers, rows }, lineStart)
      continue
    }

    if (UL_RE.test(line) || OL_RE.test(line)) {
      const ordered = !UL_RE.test(line)
      const marker = ordered ? OL_RE : UL_RE
      const items: InlineToken[][] = []
      while (i < lines.length && marker.test(lines[i])) {
        items.push(parseInlineTokens(lines[i].replace(marker, '')))
        i += 1
      }
      push({ kind: 'list', ordered, items }, lineStart)
      continue
    }

    if (!line.trim()) {
      i += 1
      continue
    }

    i += 1
    push({ kind: 'paragraph', inline: parseInlineTokens(line) }, lineStart)
  }

  return { blocks, ranges }
}

/**
 * 流式追加时前面已经认完的块沿用原对象，气泡里写完的标题/列表不用每字重绘。
 */
export function parseMarkdownBlocks(source: string): MarkdownBlock[] {
  if (source === cache.source) return cache.blocks
  const lines = source.split('\n')

  let restart = 0
  let keptBlocks: MarkdownBlock[] = []
  let keptRanges: BlockRange[] = []

  if (cache.ranges.length > 0) {
    let firstDiff = 0
    const limit = Math.min(cache.lines.length, lines.length)
    while (firstDiff < limit && cache.lines[firstDiff] === lines[firstDiff]) {
      firstDiff += 1
    }
    let keep = 0
    while (keep < cache.ranges.length && cache.ranges[keep].end <= firstDiff) {
      keep += 1
    }
    restart = keep < cache.ranges.length ? cache.ranges[keep].start : firstDiff
    keptBlocks = cache.blocks.slice(0, keep)
    keptRanges = cache.ranges.slice(0, keep)
  }

  const parsed = parseBlocksFrom(lines, restart)
  const blocks = keptBlocks.concat(parsed.blocks)
  const ranges = keptRanges.concat(parsed.ranges)
  cache = { source, lines, blocks, ranges }
  return blocks
}
