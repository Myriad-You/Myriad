import type { TransferNote } from './types'
import { addressFields, importOldPaths, importSlug } from './addresses'
import { parseUnixOrDate } from './text'

const FRONT = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/
/** Hugo 的 TOML 头。 */
const FRONT_TOML = /^\+\+\+\r?\n([\s\S]*?)\r?\n\+\+\+\r?\n?/

interface FrontMatter {
  values: Record<string, string>
  /** `aliases` 这类列表：YAML 的 `- x` 行或一行里的 `[a, b]`。 */
  lists: Record<string, string[]>
}

function unquote(value: string): string {
  return value.trim().replace(/^['"]|['"]$/g, '')
}

function inlineList(value: string): string[] | null {
  const match = /^\[(.*)\]$/.exec(value.trim())
  if (!match) return null
  return match[1]!.split(',').map(unquote).filter(Boolean)
}

function parseFront(raw: string, separator: ':' | '='): FrontMatter {
  const values: Record<string, string> = {}
  const lists: Record<string, string[]> = {}
  let listKey: string | null = null
  for (const line of raw.split('\n')) {
    const item = /^\s*-\s(.*)$/.exec(line)
    if (item && listKey) {
      const value = unquote(item[1]!)
      if (value) (lists[listKey] ??= []).push(value)
      continue
    }
    const cut = line.indexOf(separator)
    if (cut <= 0) continue
    const key = line.slice(0, cut).trim().toLowerCase()
    if (!key) continue
    const rest = line.slice(cut + 1).trim()
    listKey = rest ? null : key
    values[key] = unquote(rest)
    const list = inlineList(rest)
    if (list) lists[key] = list
  }
  return { values, lists }
}

function readFront(text: string): { front: FrontMatter; body: string } {
  const yaml = FRONT.exec(text)
  if (yaml) return { front: parseFront(yaml[1]!, ':'), body: text.slice(yaml[0].length) }
  const toml = FRONT_TOML.exec(text)
  if (toml) return { front: parseFront(toml[1]!, '='), body: text.slice(toml[0].length) }
  return { front: { values: {}, lists: {} }, body: text }
}

export function parseMarkdownFile(name: string, text: string): TransferNote {
  const { front, body } = readFront(text)
  const meta = front.values
  const heading = /^# ([^\n]+)$/m.exec(body)
  const title = meta.title || heading?.[1]?.trim() || name.replace(/\.md$/i, '')
  const content = heading ? body.replace(heading[0], '').trim() : body.trim()
  const status = (meta.status || meta.draft || '').toLowerCase()
  // 静态博客把文章地址写在头里：permalink（Hexo 等）、url 和 aliases（Hugo）。
  const oldPaths = importOldPaths([
    meta.permalink,
    meta.url,
    ...(front.lists.aliases ?? (meta.aliases ? [meta.aliases] : [])),
  ])
  return {
    title,
    content_md: content,
    topic: meta.topic || meta.category || meta.tags || null,
    published_at: parseUnixOrDate(meta.date || meta.published || meta.created),
    status:
      status === 'draft' || status === 'true' || status === 'yes'
        ? 'draft'
        : 'published',
    ...addressFields(importSlug(meta.slug), oldPaths),
  }
}

export function serializeMarkdownFile(note: TransferNote): string {
  const head = [
    '---',
    `title: ${JSON.stringify(note.title || 'untitled')}`,
    note.topic ? `topic: ${JSON.stringify(note.topic)}` : null,
    note.published_at
      ? `date: ${new Date(note.published_at * 1000).toISOString()}`
      : null,
    `status: ${note.status}`,
    note.slug ? `slug: ${JSON.stringify(note.slug)}` : null,
    note.old_paths?.length
      ? ['aliases:', ...note.old_paths.map((path) => `  - ${JSON.stringify(path)}`)].join('\n')
      : null,
    '---',
    '',
    note.content_md.trim(),
    '',
  ]
  return head.filter((line): line is string => line != null).join('\n')
}
