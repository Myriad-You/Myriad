import type { CommentItem } from '../../../services/brewApi'
import type { AnnotationItem } from '../../../services/brewliaApi'
import type { ThemeKey } from './types'

export interface TextAnchor {
  selected_text: string
  start_offset?: number
  end_offset?: number
  context_before?: string
  context_after?: string
}

const MEDIA_EXEMPT =
  'script, style, button, iframe, [data-embed-exempt], .brew-embed-card, .brew-embed-exempt, .brew-bilibili-embed, .brew-netease-music, .brew-steam-game, .brew-bilibili-video'

/** Marks stay elements: overlap, keyboard, and media exemption need DOM. */
export function cssCustomHighlightAvailable(): boolean {
  return typeof CSS !== 'undefined' && 'highlights' in CSS
}

export function commentAnchorStale(
  comment: { content_revision?: number | null },
  itemRevision: number | null | undefined,
): boolean {
  return (
    typeof comment.content_revision === 'number' &&
    typeof itemRevision === 'number' &&
    comment.content_revision !== itemRevision
  )
}

/** Ambiguous anchors stay in the comment panel; do not guess marks. */
export function resolveCommentAnchor(
  text: string,
  anchor: TextAnchor,
): number | null {
  const quote = anchor.selected_text
  if (!quote) return null
  const matchesContext = (start: number) =>
    (!anchor.context_before ||
      text.slice(0, start).endsWith(anchor.context_before)) &&
    (!anchor.context_after ||
      text.slice(start + quote.length).startsWith(anchor.context_after))
  const offset = anchor.start_offset
  if (
    offset !== undefined &&
    Number.isInteger(offset) &&
    offset >= 0 &&
    text.slice(offset, offset + quote.length) === quote &&
    matchesContext(offset) &&
    (anchor.end_offset === undefined ||
      anchor.end_offset === offset + quote.length)
  ) {
    return offset
  }
  const candidates: number[] = []
  let position = text.indexOf(quote)
  while (position !== -1) {
    if (matchesContext(position)) candidates.push(position)
    position = text.indexOf(quote, position + 1)
  }
  return candidates.length === 1 ? candidates[0] : null
}

function wrapPlainTextRange(
  root: HTMLElement,
  start: number,
  end: number,
  createMark: (doc: Document) => HTMLElement,
): boolean {
  if (end <= start) return false
  const walker = root.ownerDocument.createTreeWalker(root, 4)
  const parts: { node: Text; start: number; end: number }[] = []
  let offset = 0
  let excludedOverlap = false
  while (walker.nextNode()) {
    const node = walker.currentNode as Text
    const next = offset + node.length
    if (offset < end && next > start) {
      if (node.parentElement?.closest(MEDIA_EXEMPT)) excludedOverlap = true
      parts.push({
        node,
        start: Math.max(0, start - offset),
        end: Math.min(node.length, end - offset),
      })
    }
    offset = next
  }
  if (excludedOverlap || parts.length === 0) return false
  for (const part of parts) {
    const selected = part.node.splitText(part.start)
    selected.splitText(part.end - part.start)
    const mark = createMark(root.ownerDocument)
    selected.replaceWith(mark)
    mark.appendChild(selected)
  }
  return true
}

export function highlightAnchoredComments(
  html: string,
  comments: CommentItem[],
  theme: ThemeKey,
): string {
  if (!comments.length) return html
  const root = new DOMParser().parseFromString(html, 'text/html').body
  const text = root.textContent ?? ''
  const backgrounds = {
    light: '#fef08a',
    sepia: '#f5d78e',
    dark: '#854d0e',
    night: '#1e3a5f',
  }
  const borders = {
    light: '#eab308',
    sepia: '#ca8a04',
    dark: '#fbbf24',
    night: '#3b82f6',
  }
  for (const comment of comments) {
    if (!Number.isFinite(Number(comment.id)) || comment.parent_id) continue
    const start = resolveCommentAnchor(text, comment)
    if (start === null) continue
    wrapPlainTextRange(root, start, start + comment.selected_text.length, (doc) => {
      const mark = doc.createElement('mark')
      mark.className = 'user-comment-highlight'
      mark.dataset.commentId = String(Number(comment.id))
      mark.tabIndex = 0
      mark.setAttribute('role', 'button')
      mark.setAttribute('aria-label', comment.comment || comment.selected_text)
      const color =
        comment.color &&
        /^#(?:[\da-f]{3}|[\da-f]{6}|[\da-f]{8})$/i.test(comment.color)
          ? comment.color
          : undefined
      mark.style.backgroundColor = color ?? backgrounds[theme]
      mark.style.borderBottom = `2px solid ${color ?? borders[theme]}`
      mark.style.cursor = 'pointer'
      mark.style.borderRadius = '2px'
      return mark
    })
  }
  return root.innerHTML
}

export function highlightAnchoredAnnotations(
  html: string,
  annotations: AnnotationItem[],
): string {
  if (!annotations.length) return html
  const root = new DOMParser().parseFromString(html, 'text/html').body
  const text = root.textContent ?? ''
  const seen = new Set<string>()
  const ordered = annotations.toSorted((a, b) => b.term.length - a.term.length)
  for (const [index, annotation] of ordered.entries()) {
    const quote = annotation.term
    if (!quote || seen.has(`${annotation.type}:${quote}`)) continue
    const start = resolveCommentAnchor(text, {
      selected_text: quote,
      start_offset: annotation.position,
    })
    if (start === null) continue
    seen.add(`${annotation.type}:${quote}`)
    wrapPlainTextRange(root, start, start + quote.length, (doc) => {
      const mark = doc.createElement('mark')
      mark.className = 'brewlia-annotation'
      const rawId = annotation.id || `${annotation.type}-${index}`
      mark.setAttribute(
        'data-annotation-id',
        String(rawId).replaceAll(/[^\w-]/g, ''),
      )
      mark.setAttribute('data-type', annotation.type)
      mark.setAttribute('data-term', encodeURIComponent(annotation.term))
      mark.setAttribute(
        'data-explanation',
        encodeURIComponent(annotation.explanation),
      )
      return mark
    })
  }
  return root.innerHTML
}
