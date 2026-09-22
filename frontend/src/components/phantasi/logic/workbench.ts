/** 工作台只列表和动作；正文仍在编辑器。 */

import { phantasiCategoryParts } from '../constants'

export type WorkbenchNoteOpen =
  | { kind: 'item'; id: number }
  | { kind: 'doc'; id: number }

export function workbenchNoteOpen(doc: {
  id: number
  item_id: number | null
}): WorkbenchNoteOpen {
  if (doc.item_id != null) return { kind: 'item', id: doc.item_id }
  return { kind: 'doc', id: doc.id }
}

export type WorkbenchNoteStatusKey =
  | 'noteStatusDraft'
  | 'noteStatusScheduled'
  | 'noteStatusPublished'

export function workbenchNoteStatusKey(doc: {
  status: string
}): WorkbenchNoteStatusKey {
  if (doc.status === 'scheduled') return 'noteStatusScheduled'
  if (doc.status === 'published') return 'noteStatusPublished'
  return 'noteStatusDraft'
}

export type WorkbenchNoteStatusFilter =
  | 'all'
  | 'draft'
  | 'scheduled'
  | 'published'

export function workbenchNoteStatusFilter(
  doc: { status: string },
): Exclude<WorkbenchNoteStatusFilter, 'all'> {
  if (doc.status === 'scheduled') return 'scheduled'
  if (doc.status === 'published') return 'published'
  return 'draft'
}

export function collectWorkbenchNoteTopics(
  docs: ReadonlyArray<{ topic?: string | null }>,
): string[] {
  const names = new Set<string>()
  for (const doc of docs) {
    for (const name of phantasiCategoryParts(doc.topic)) names.add(name)
  }
  return [...names].toSorted((a, b) => a.localeCompare(b, 'zh'))
}

export interface WorkbenchNoteAuthor {
  user_id: number
  user_name?: string
  user_display_name?: string
}

export function workbenchAuthorLabel(
  author: { user_display_name?: string; user_name?: string },
  fallback: string,
): string {
  return author.user_display_name?.trim() || author.user_name?.trim() || fallback
}

export function workbenchNoteAuthorsOf(doc: {
  authors?: WorkbenchNoteAuthor[]
  user_id?: number
  user_display_name?: string
  user_name?: string
}): WorkbenchNoteAuthor[] {
  if (doc.authors?.length) return doc.authors
  if (doc.user_id == null) return []
  return [
    {
      user_id: doc.user_id,
      user_name: doc.user_name,
      user_display_name: doc.user_display_name,
    },
  ]
}

export function workbenchNoteAuthorName(
  doc: {
    authors?: WorkbenchNoteAuthor[]
    user_id?: number
    user_display_name?: string
    user_name?: string
  },
  fallback: string,
): string {
  const names = workbenchNoteAuthorsOf(doc)
    .map((author) => workbenchAuthorLabel(author, ''))
    .filter(Boolean)
  return names.join(' · ') || fallback
}

export function collectWorkbenchNoteAuthors(
  docs: ReadonlyArray<{
    authors?: WorkbenchNoteAuthor[]
    user_id?: number
    user_display_name?: string
    user_name?: string
  }>,
  fallback: string,
): Array<{ key: string; label: string }> {
  const seen = new Map<string, string>()
  for (const doc of docs) {
    for (const author of workbenchNoteAuthorsOf(doc)) {
      const key = String(author.user_id)
      if (!seen.has(key)) seen.set(key, workbenchAuthorLabel(author, fallback))
    }
  }
  return [...seen.entries()]
    .map(([key, label]) => ({ key, label }))
    .toSorted((a, b) => a.label.localeCompare(b.label, 'zh'))
}

export function filterWorkbenchReviews<
  T extends {
    site_name: string
    site_url: string
    feed_url?: string | null
    description?: string | null
    message?: string | null
    applicant_name?: string | null
    applicant_email?: string | null
    status: string
  },
>(
  rows: readonly T[],
  query: string,
  status: 'all' | 'pending' | 'approved' | 'rejected' = 'all',
): T[] {
  const needle = query.trim().toLowerCase()
  return rows.filter((row) => {
    if (status !== 'all' && row.status !== status) return false
    if (!needle) return true
    const hay = [
      row.site_name,
      row.site_url,
      row.feed_url,
      row.description,
      row.message,
      row.applicant_name,
      row.applicant_email,
    ]
      .filter(Boolean)
      .join('\n')
      .toLowerCase()
    return hay.includes(needle)
  })
}

export function filterWorkbenchComments<
  T extends {
    comment: string
    selected_text: string
    item_title?: string
    source_name?: string
    user_name?: string
    user_display_name?: string
  },
>(comments: readonly T[], query: string): T[] {
  const needle = query.trim().toLowerCase()
  if (!needle) return [...comments]
  return comments.filter((row) => {
    const hay = [
      row.comment,
      row.selected_text,
      row.item_title,
      row.source_name,
      row.user_display_name,
      row.user_name,
    ]
      .filter(Boolean)
      .join('\n')
      .toLowerCase()
    return hay.includes(needle)
  })
}

export function filterWorkbenchNotes<
  T extends {
    title: string
    content_md?: string
    excerpt?: string | null
    topic?: string | null
    status: string
    last_error?: string | null
    user_id?: number
    user_name?: string
    user_display_name?: string
    authors?: WorkbenchNoteAuthor[]
  },
>(
  docs: readonly T[],
  filter: {
    status: WorkbenchNoteStatusFilter
    query: string
    topic?: string | null
    author?: string | null
  },
): T[] {
  const needle = filter.query.trim().toLowerCase()
  return docs.filter((doc) => {
    if (
      filter.status !== 'all' &&
      workbenchNoteStatusFilter(doc) !== filter.status
    ) {
      return false
    }
    if (filter.topic != null) {
      const parts = phantasiCategoryParts(doc.topic)
      if (filter.topic === '') {
        if (parts.length > 0) return false
      } else if (!parts.includes(filter.topic)) {
        return false
      }
    }
    if (filter.author != null) {
      const ids = workbenchNoteAuthorsOf(doc).map((author) => String(author.user_id))
      if (!ids.includes(filter.author)) return false
    }
    if (!needle) return true
    return noteSearchHaystack(doc).includes(needle)
  })
}

function noteSearchHaystack(doc: {
  title: string
  content_md?: string
  excerpt?: string | null
  topic?: string | null
  user_name?: string
  user_display_name?: string
  authors?: WorkbenchNoteAuthor[]
}): string {
  const authorNames = workbenchNoteAuthorsOf(doc).flatMap((author) => [
    author.user_display_name ?? '',
    author.user_name ?? '',
  ])
  return [
    doc.title,
    doc.topic ?? '',
    doc.excerpt ?? '',
    doc.user_display_name ?? '',
    doc.user_name ?? '',
    ...authorNames,
  ]
    .join('\n')
    .toLowerCase()
}

/** 定时看预约时间，已发布看发布时间，其余看更新。 */
export function workbenchNoteWhen(doc: {
  status: string
  last_error?: string | null
  scheduled_at?: number | null
  published_at?: number | null
  updated_at?: number | null
}): number | null {
  if (doc.last_error || doc.status === 'scheduled') {
    return doc.scheduled_at ?? doc.updated_at ?? null
  }
  if (doc.status === 'published') {
    return doc.published_at ?? doc.updated_at ?? null
  }
  return doc.updated_at ?? null
}

export const WORKBENCH_MEDIA_FORMATS = [
  'jpeg',
  'png',
  'gif',
  'webp',
  'mp4',
  'webm',
  'mov',
  'other',
] as const

export type WorkbenchMediaFormatKey = (typeof WORKBENCH_MEDIA_FORMATS)[number]

const MEDIA_MIME_FORMAT: Record<string, WorkbenchMediaFormatKey> = {
  'image/jpeg': 'jpeg',
  'image/jpg': 'jpeg',
  'image/pjpeg': 'jpeg',
  'image/png': 'png',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'video/mp4': 'mp4',
  'video/webm': 'webm',
  'video/quicktime': 'mov',
}

const MEDIA_EXT_FORMAT: Record<string, WorkbenchMediaFormatKey> = {
  jpg: 'jpeg',
  jpeg: 'jpeg',
  png: 'png',
  gif: 'gif',
  webp: 'webp',
  mp4: 'mp4',
  webm: 'webm',
  mov: 'mov',
}

export function workbenchMediaFormatKey(item: {
  mime: string
  name?: string | null
}): WorkbenchMediaFormatKey {
  const mime = item.mime.trim().toLowerCase().split(';')[0] ?? ''
  const fromMime = MEDIA_MIME_FORMAT[mime]
  if (fromMime) return fromMime
  const ext = item.name?.trim().toLowerCase().split('.').pop()
  if (ext) {
    const fromName = MEDIA_EXT_FORMAT[ext]
    if (fromName) return fromName
  }
  return 'other'
}

export function workbenchMediaFormatLabel(
  key: WorkbenchMediaFormatKey,
  otherLabel: string,
): string {
  if (key === 'jpeg') return 'JPEG'
  if (key === 'png') return 'PNG'
  if (key === 'gif') return 'GIF'
  if (key === 'webp') return 'WebP'
  if (key === 'mp4') return 'MP4'
  if (key === 'webm') return 'WebM'
  if (key === 'mov') return 'MOV'
  return otherLabel
}

export function formatWorkbenchBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return ''
  if (bytes < 1024) return `${Math.round(bytes)} B`
  if (bytes < 1024 * 1024) {
    const kb = bytes / 1024
    return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`
  }
  const mb = bytes / (1024 * 1024)
  return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`
}

export function workbenchNoteCover(doc: {
  image?: string | null
  content_md?: string | null
}): string | null {
  const chosen = doc.image?.trim()
  if (chosen) return chosen
  return firstBodyImage(doc.content_md ?? '')
}

function firstBodyImage(markdown: string): string | null {
  const inline = /!\[[^\]]*\]\(([^\s)]+)/.exec(markdown)
  const ref = /!\[([^\]]*)\]\[([^\]\s]*)\]/.exec(markdown)
  const inlineAt = inline?.index ?? Number.POSITIVE_INFINITY
  const refAt = ref?.index ?? Number.POSITIVE_INFINITY
  if (inline && inlineAt <= refAt) return inline[1]!.trim() || null
  if (!ref) return null
  const key = (ref[2] || ref[1]).trim().toLowerCase()
  for (const match of markdown.matchAll(
    /\[(?!\^)([^\]]+)\]:\s+(?:<([^>\s]+)>|(\S+))/g,
  )) {
    if (match[1]!.trim().toLowerCase() === key) {
      return (match[2] || match[3] || '').trim() || null
    }
  }
  return null
}

export const WORKBENCH_NOTE_EXCERPT_CHARS = 100

export function workbenchNoteListExcerpt(doc: {
  excerpt?: string | null
  content_md?: string | null
}): string {
  const ready = doc.excerpt?.trim()
  if (ready) return ready
  return workbenchNoteExcerpt(doc.content_md)
}

export function workbenchNoteExcerpt(
  markdown: string | null | undefined,
  maxChars = WORKBENCH_NOTE_EXCERPT_CHARS,
): string {
  const chars = [...plainNoteExcerpt(markdown ?? '')]
  if (chars.length <= maxChars) return chars.join('')
  return `${chars.slice(0, maxChars).join('').trimEnd()}…`
}

function plainNoteExcerpt(markdown: string): string {
  let text = markdown.replaceAll('\r\n', '\n')
  text = text.replace(/```[\s\S]*?```/g, ' ')
  text = text.replace(/~~~[\s\S]*?~~~/g, ' ')
  text = text.replace(/^:::widget[^\n]*$/gm, ' ')
  text = text.replace(/^:::(?:columns|col)?\s*$/gm, ' ')
  text = text.replace(/!\[[^\]]*\]\([^)]+\)/g, ' ')
  text = text.replace(/!\[[^\]]*\]\[[^\]]*\]/g, ' ')
  text = text.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
  text = text.replace(/\[([^\]]+)\]\[[^\]]*\]/g, '$1')
  text = text.replace(/^\s*\[(?!\^)[^\]]+\]:\s+\S.*$/gm, ' ')
  text = text.replace(/^\s*\[\^[^\]]+\]:.*$/gm, ' ')
  text = text.replace(/\[\^[^\]]+\]/g, '')
  text = text.replace(/^\s{0,3}#{1,6}\s+/gm, '')
  text = text.replace(/^\s{0,3}>\s?/gm, '')
  text = text.replace(/^\s{0,3}(?:[-*+]|\d+\.)\s+(?:\[[ x]\]\s+)?/gim, '')
  text = text.replace(/^\s{0,3}[-*_]{3,}\s*$/gm, ' ')
  text = text.replace(/`([^`]+)`/g, '$1')
  text = text.replace(/(\*\*|__)(.*?)\1/g, '$2')
  text = text.replace(/(\*|_)(.*?)\1/g, '$2')
  text = text.replace(/~~(.*?)~~/g, '$1')
  text = text.replace(/<[^>]+>/g, ' ')
  return text.replace(/\s+/g, ' ').trim()
}

export function workbenchMediaRefLabel(
  refs: readonly string[],
  copy: { notes: string; articles: string; site: string },
): string {
  return refs
    .map((ref) => {
      if (ref === 'notes') return copy.notes
      if (ref === 'articles') return copy.articles
      if (ref === 'site') return copy.site
      return ''
    })
    .filter(Boolean)
    .join(' · ')
}
