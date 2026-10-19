import type { AnnotationItem } from '../../services/phantasiaiApi'

/**
 * The companion reaches into the blog page that holds it. The page runs no
 * script of its own (its CSP is `script-src 'none'`); the companion frame is
 * hidden at the end of the article and is the same origin, so it paints the
 * annotations into the article and draws its side capsules into a host node of
 * its own. Nodes belong to the parent document: no `instanceof` checks, and AI
 * text only ever goes in as `textContent`.
 */

/** The host the side capsules are drawn into (a shadow root), at the end of `<body>`. */
export const COMPANION_HOST_ID = 'myriad-companion'

export interface CompanionPage {
  doc: Document
  win: Window
  /** The article body the frame was written into: the capsules hug its edges. */
  root: HTMLElement
}

/** Null when the frame is cross-origin or is not the companion frame of a post. */
export function findCompanionPage(frame: Element | null): CompanionPage | null {
  if (!frame?.classList.contains('myriad-companion__frame')) return null
  const root = frame.parentElement
  const win = frame.ownerDocument.defaultView
  if (!root || !win) return null
  return { doc: frame.ownerDocument, win, root }
}

/** The host node, made once per page; scripts there never run, the controller draws into it. */
export function companionHost(page: CompanionPage): HTMLElement {
  const found = page.doc.getElementById(COMPANION_HOST_ID)
  if (found) return found
  const host = page.doc.createElement('div')
  host.id = COMPANION_HOST_ID
  page.doc.body.append(host)
  return host
}

const EXEMPT =
  'a, button, pre, code, script, style, iframe, math, svg, .myriad-annot-tip, .note-widget, .math'

const KNOWN_TYPES = new Set(['term', 'reference', 'implicit', 'context', 'abbreviation'])

interface TextPart {
  node: Text
  offset: number
  excluded: boolean
}

function indexText(root: HTMLElement): TextPart[] {
  const walker = root.ownerDocument.createTreeWalker(root, 4)
  const parts: TextPart[] = []
  let offset = 0
  while (walker.nextNode()) {
    const node = walker.currentNode as Text
    parts.push({ node, offset, excluded: !!node.parentElement?.closest(EXEMPT) })
    offset += node.length
  }
  return parts
}

function wrapRange(
  parts: TextPart[],
  start: number,
  end: number,
  createMark: () => HTMLElement,
): boolean {
  let low = 0
  let high = parts.length
  while (low < high) {
    const mid = (low + high) >>> 1
    if (parts[mid].offset + parts[mid].node.length <= start) low = mid + 1
    else high = mid
  }
  const first = low
  while (low < parts.length && parts[low].offset < end) {
    if (parts[low].excluded) return false
    low++
  }
  if (low === first) return false
  for (let i = low - 1; i >= first; i--) {
    const part = parts[i]
    const from = Math.max(0, start - part.offset)
    const to = Math.min(part.node.length, end - part.offset)
    const selected = from ? part.node.splitText(from) : part.node
    const tail = to - from < selected.length ? selected.splitText(to - from) : null
    const mark = createMark()
    selected.replaceWith(mark)
    mark.appendChild(selected)
    parts.splice(
      i,
      1,
      ...(from ? [{ ...part }] : []),
      { node: selected, offset: part.offset + from, excluded: true },
      ...(tail ? [{ ...part, node: tail, offset: part.offset + to }] : []),
    )
  }
  return true
}

/** Where the term is: the AI's offset when it still points at the term, then each occurrence in order. */
export function termCandidates(text: string, term: string, position?: number): number[] {
  if (!term) return []
  const found: number[] = []
  if (
    typeof position === 'number' &&
    Number.isInteger(position) &&
    position >= 0 &&
    text.slice(position, position + term.length) === term
  ) {
    found.push(position)
  }
  for (let at = text.indexOf(term); at !== -1; at = text.indexOf(term, at + 1)) {
    if (at !== found[0]) found.push(at)
  }
  return found
}

export function unpaintAnnotations(root: HTMLElement): void {
  const parents = new Set<Node>()
  for (const mark of [...root.querySelectorAll('mark.myriad-annot')]) {
    const parent = mark.parentNode
    mark.replaceWith(...mark.childNodes)
    if (parent) parents.add(parent)
  }
  for (const parent of parents) parent.normalize()
}

/**
 * One mark per annotation, at the first place the term reads as plain text:
 * not inside links, code, formulas, widgets or another mark. Longer terms
 * go first so a short one does not split a longer one. Returns the indices that landed.
 */
export function paintAnnotations(root: HTMLElement, annotations: AnnotationItem[]): number[] {
  unpaintAnnotations(root)
  const text = root.textContent ?? ''
  const parts = indexText(root)
  const order = annotations
    .map((annotation, index) => ({ annotation, index }))
    .toSorted((a, b) => b.annotation.term.length - a.annotation.term.length)
  const painted: number[] = []
  for (const { annotation, index } of order) {
    const term = annotation.term
    for (const start of termCandidates(text, term, annotation.position)) {
      const landed = wrapRange(parts, start, start + term.length, () => {
        const mark = root.ownerDocument.createElement('mark')
        mark.className = 'myriad-annot'
        mark.dataset.type = KNOWN_TYPES.has(annotation.type) ? annotation.type : 'term'
        mark.dataset.index = String(index)
        mark.tabIndex = 0
        mark.setAttribute('role', 'button')
        return mark
      })
      if (landed) {
        painted.push(index)
        break
      }
    }
  }
  return painted.toSorted((a, b) => a - b)
}

export interface ArticleLink {
  /** Paints the article; returns the indices found in it. */
  paint: (annotations: AnnotationItem[]) => number[]
  /** Scrolls the article to that annotation and flashes it. False when it is not in the text. */
  reveal: (index: number) => boolean
  dispose: () => void
}

interface ArticleLinkOptions {
  typeLabel: (type: string) => string
  /** A reader chose a term in the article. */
  onPick: (index: number) => void
  /** Esc on the page. */
  onEscape: () => void
}

const TIP_ID = 'myriad-annot-tip'
/** Scrolled-to marks sit this far below the top, as the reader used to. */
const REVEAL_OFFSET = 150

function markOf(target: EventTarget | null): HTMLElement | null {
  const element = target as Element | null
  return (element?.closest?.('mark.myriad-annot') as HTMLElement | null) ?? null
}

export function connectArticle(page: CompanionPage, options: ArticleLinkOptions): ArticleLink {
  const { doc, root, win: view } = page
  let annotations: AnnotationItem[] = []
  let flashTimer: ReturnType<typeof setTimeout> | undefined
  let hideTimer: ReturnType<typeof setTimeout> | undefined

  doc.getElementById(TIP_ID)?.remove()
  const tip = doc.createElement('div')
  tip.className = 'myriad-annot-tip'
  tip.id = TIP_ID
  tip.setAttribute('role', 'tooltip')
  tip.hidden = true
  doc.body.append(tip)

  const annotationAt = (mark: HTMLElement) => annotations[Number(mark.dataset.index)]

  const show = (mark: HTMLElement) => {
    const annotation = annotationAt(mark)
    if (!annotation) return
    clearTimeout(hideTimer)
    const kind = doc.createElement('small')
    kind.textContent = options.typeLabel(mark.dataset.type ?? 'term')
    const term = doc.createElement('b')
    term.textContent = annotation.term
    const body = doc.createElement('span')
    body.textContent = annotation.explanation
    tip.replaceChildren(kind, term, body)
    tip.hidden = false
    mark.setAttribute('aria-describedby', TIP_ID)
    const rect = mark.getBoundingClientRect()
    const width = tip.offsetWidth
    const height = tip.offsetHeight
    const left = Math.min(
      Math.max(16, rect.left + rect.width / 2 - width / 2),
      Math.max(16, view.innerWidth - width - 16),
    )
    const top = rect.top - height - 8 >= 16 ? rect.top - height - 8 : rect.bottom + 8
    tip.style.left = `${left}px`
    tip.style.top = `${top}px`
  }

  const hide = () => {
    clearTimeout(hideTimer)
    tip.hidden = true
  }

  const onOver = (event: Event) => {
    const mark = markOf(event.target)
    if (mark) show(mark)
  }
  const onOut = (event: Event) => {
    const mark = markOf(event.target)
    if (!mark || mark.contains((event as MouseEvent).relatedTarget as Node | null)) return
    clearTimeout(hideTimer)
    hideTimer = setTimeout(hide, 150)
  }
  const onFocusOut = (event: Event) => {
    if (markOf(event.target)) hide()
  }
  const pick = (mark: HTMLElement) => options.onPick(Number(mark.dataset.index))
  const onClick = (event: Event) => {
    const mark = markOf(event.target)
    if (mark) pick(mark)
  }
  const onMarkKey = (event: Event) => {
    const key = (event as KeyboardEvent).key
    const mark = markOf(event.target)
    if (!mark || (key !== 'Enter' && key !== ' ')) return
    event.preventDefault()
    pick(mark)
  }
  const onDocKey = (event: Event) => {
    if ((event as KeyboardEvent).key !== 'Escape') return
    hide()
    options.onEscape()
  }

  root.addEventListener('mouseover', onOver)
  root.addEventListener('mouseout', onOut)
  root.addEventListener('focusin', onOver)
  root.addEventListener('focusout', onFocusOut)
  root.addEventListener('click', onClick)
  root.addEventListener('keydown', onMarkKey)
  doc.addEventListener('keydown', onDocKey)
  view.addEventListener('scroll', hide, { passive: true })

  return {
    paint(next) {
      annotations = next
      hide()
      return paintAnnotations(root, next)
    },
    reveal(index) {
      const mark = root.querySelector<HTMLElement>(`mark.myriad-annot[data-index="${index}"]`)
      if (!mark) return false
      const quiet = view.matchMedia?.('(prefers-reduced-motion: reduce)').matches
      const top = mark.getBoundingClientRect().top + view.scrollY - REVEAL_OFFSET
      view.scrollTo({ top: Math.max(0, top), behavior: quiet ? 'auto' : 'smooth' })
      mark.classList.remove('is-flash')
      void mark.offsetWidth
      mark.classList.add('is-flash')
      clearTimeout(flashTimer)
      flashTimer = setTimeout(() => mark.classList.remove('is-flash'), 1500)
      return true
    },
    dispose() {
      clearTimeout(flashTimer)
      clearTimeout(hideTimer)
      root.removeEventListener('mouseover', onOver)
      root.removeEventListener('mouseout', onOut)
      root.removeEventListener('focusin', onOver)
      root.removeEventListener('focusout', onFocusOut)
      root.removeEventListener('click', onClick)
      root.removeEventListener('keydown', onMarkKey)
      doc.removeEventListener('keydown', onDocKey)
      view.removeEventListener('scroll', hide)
      tip.remove()
      unpaintAnnotations(root)
    },
  }
}
