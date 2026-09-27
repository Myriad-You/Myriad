/**
 * 剪贴板富文本 → 可视层能认的 HTML / Markdown。
 * Word / Excel / Google Docs 的表格、列表、加粗斜体都走这里，不直接把 Mso 垃圾贴进编辑器。
 */

import { mathIslandHtml, replaceMathMarkdown } from './noteMath'
import { markdownToVisualHtml, visualHtmlToMarkdown } from './noteVisual'

const ELEMENT = 1
const TEXT = 3
const SAFE_HREF = /^(https?:|mailto:)/i
const SAFE_IMG = /^https?:/i

export function extractClipboardFragment(html: string): string {
  const start = html.indexOf('<!--StartFragment-->')
  const end = html.indexOf('<!--EndFragment-->')
  if (start >= 0 && end > start) {
    return html.slice(start + '<!--StartFragment-->'.length, end)
  }
  const body = /<body\b[^>]*>([\s\S]*?)<\/body>/i.exec(html)
  return body?.[1] ?? html
}

export function looksLikeOfficeHtml(html: string): boolean {
  return /xmlns:o=|xmlns:w=|mso-|MsoNormal|MsoTable|MsoList|office:office|docs-internal-guid|urn:schemas-microsoft-com/i.test(
    html,
  )
}

export function htmlHasRichBlocks(html: string): boolean {
  return (
    looksLikeOfficeHtml(html) ||
    /<(?:table|h[1-6]|ul|ol|blockquote|pre|thead|tbody|math|strong|b|em|i|img)\b/i.test(
      html,
    ) ||
    /<[^>]*\b(?:note-math|math-inline|math-display|katex|mwe-math)/i.test(html)
  )
}

export function looksLikePlainTable(text: string): boolean {
  const lines = text.replaceAll('\r\n', '\n').replaceAll('\r', '\n').trim().split('\n')
  if (lines.length < 2) return false
  const rows = lines.map((line) => line.split('\t'))
  const width = rows[0]?.length ?? 0
  if (width < 2) return false
  const consistent = rows.filter((row) => row.length === width).length
  return consistent >= Math.ceil(rows.length * 0.8)
}

export function plainTableToMarkdown(text: string): string {
  const lines = text.replaceAll('\r\n', '\n').replaceAll('\r', '\n').trim().split('\n')
  const rows = lines.map((line) =>
    line.split('\t').map((cell) => cell.trim().replaceAll('|', String.raw`\|`)),
  )
  const width = Math.max(...rows.map((row) => row.length), 0)
  if (width < 2) return text
  const padded = rows.map((row) => [
    ...row,
    ...Array.from({ length: width - row.length }, () => ''),
  ])
  const header = `| ${padded[0]!.join(' | ')} |`
  const sep = `| ${Array.from({ length: width }, () => '---').join(' | ')} |`
  const body = padded.slice(1).map((row) => `| ${row.join(' | ')} |`)
  return [header, sep, ...body].join('\n')
}

export function insertPastedMarkdown(
  value: string,
  start: number,
  end: number,
  pasted: string,
): { value: string; selectionStart: number; selectionEnd: number } {
  const block = pasted.includes('\n') || pasted.startsWith('|') || pasted.startsWith('#')
  let lead = ''
  let tail = ''
  if (block) {
    if (start > 0 && value[start - 1] !== '\n') lead = '\n\n'
    else if (start > 1 && value[start - 1] === '\n' && value[start - 2] !== '\n') lead = '\n'
    if (end < value.length && value[end] !== '\n') {
      tail = '\n\n'
    }
    else if (end < value.length - 1 && value[end] === '\n' && value[end + 1] !== '\n') {
      tail = '\n'
    }
  }
  const body = lead + pasted + tail
  return {
    value: value.slice(0, start) + body + value.slice(end),
    selectionStart: start + body.length,
    selectionEnd: start + body.length,
  }
}

function parseHtmlDocument(html: string): Document {
  const Parser = globalThis.DOMParser
  if (!Parser) throw new Error('DOMParser missing')
  return new Parser().parseFromString(html, 'text/html')
}

function escapeText(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
}

const TEX_ANNOTATION =
  'annotation[encoding="application/x-tex"], annotation[encoding="TeX"], annotation[encoding="text/x-tex"]'

function isDisplayMathEl(el: Element): boolean {
  return (
    el.classList.contains('math-display') ||
    el.classList.contains('note-math-display') ||
    el.classList.contains('katex-display') ||
    el.classList.contains('notion-equation') ||
    el.getAttribute('display') === 'block' ||
    Boolean(
      el.classList.contains('mwe-math-element') &&
        el.querySelector(
          '.mwe-math-mathml-display, .mwe-math-fallback-image-display, math[display="block"]',
        ),
    )
  )
}

function isMathHost(el: Element): boolean {
  return (
    el.classList.contains('note-math') ||
    el.classList.contains('math') ||
    el.classList.contains('katex') ||
    el.classList.contains('katex-display') ||
    el.classList.contains('notion-equation') ||
    el.classList.contains('notion-inline-equation') ||
    el.classList.contains('mwe-math-element') ||
    el.tagName.toLowerCase() === 'math'
  )
}

function pastedMathIsland(el: Element): string | null {
  if (!isMathHost(el)) return null
  const tex =
    (el.getAttribute('data-tex') ?? '').trim() ||
    (el.querySelector(TEX_ANNOTATION)?.textContent ?? '').trim()
  if (!tex) return null
  return mathIslandHtml(
    tex,
    isDisplayMathEl(el) || el.getAttribute('display') === 'block',
  )
}

function cellAlign(el: Element): 'left' | 'center' | 'right' | null {
  const align = el.getAttribute('align')?.toLowerCase()
  if (align === 'left' || align === 'center' || align === 'right') return align
  const style = el.getAttribute('style') ?? ''
  const match = /text-align:\s*(left|center|right)/i.exec(style)
  const value = match?.[1]?.toLowerCase()
  return value === 'left' || value === 'center' || value === 'right' ? value : null
}

function unwrapElement(el: Element): void {
  const parent = el.parentNode
  if (!parent) {
    el.remove()
    return
  }
  while (el.firstChild) parent.insertBefore(el.firstChild, el)
  el.remove()
}

function stripOfficeJunk(root: Element): void {
  root
    .querySelectorAll('style, script, meta, link, xml, title, noscript')
    .forEach((node) => node.remove())
  const comments: Comment[] = []
  const walker = root.ownerDocument.createTreeWalker(root, 128)
  while (walker.nextNode()) comments.push(walker.currentNode as Comment)
  for (const node of comments) node.remove()
  for (const el of [...root.querySelectorAll('*')]) {
    const name = el.tagName
    if (!(name.includes(':') || /^([owvm]):/i.test(name))) continue
    const island = pastedMathIsland(el)
    if (island) {
      const wrap = el.ownerDocument.createElement('span')
      wrap.innerHTML = island
      const node = wrap.firstElementChild
      if (node) el.replaceWith(node)
      else unwrapElement(el)
      continue
    }
    unwrapElement(el)
  }
}

function promoteInlineStyles(root: Element): void {
  for (const el of [...root.querySelectorAll('[style]')]) {
    const style = el.getAttribute('style') ?? ''
    const wrap = (tag: string) => {
      if (el.querySelector(`:scope > ${tag}`) && el.childNodes.length === 1) return
      const inner = el.ownerDocument.createElement(tag)
      while (el.firstChild) inner.appendChild(el.firstChild)
      el.appendChild(inner)
    }
    if (/font-weight:\s*(bold|[7-9]00)/i.test(style)) wrap('strong')
    if (/font-style:\s*italic/i.test(style)) wrap('em')
    if (/line-through/i.test(style)) wrap('del')
  }
}

function isWordListPara(el: Element): boolean {
  if (el.tagName !== 'P') return false
  const style = el.getAttribute('style') ?? ''
  const cls = el.getAttribute('class') ?? ''
  return /mso-list/i.test(style) || /MsoList/i.test(cls)
}

function listKind(p: Element): 'ul' | 'ol' {
  const marker =
    p.querySelector('[style*="mso-list:Ignore"], [style*="mso-list: Ignore"]')?.textContent ??
    ''
  const text = marker.trim() || (p.textContent ?? '').trim()
  if (/^\d/.test(text) || /^\(\d+\)/.test(text) || /^[a-z][.)]/i.test(text)) return 'ol'
  return 'ul'
}

function listLevel(p: Element): number {
  const match = /mso-list:[^;]*level(\d+)/i.exec(p.getAttribute('style') ?? '')
  return Math.max(1, Number(match?.[1]) || 1)
}

function wordParaInner(p: Element): string {
  const clone = p.cloneNode(true) as Element
  clone
    .querySelectorAll('[style*="mso-list:Ignore"], [style*="mso-list: Ignore"]')
    .forEach((node) => node.remove())
  return serializeInline(clone).replace(/^(?:[·•○■□◦▪▫]|\d+[.)]|[a-z][.)])\s*/i, '')
}

function convertWordLists(root: Element): void {
  const paras = [...root.querySelectorAll('p')].filter(isWordListPara)
  const handled = new Set<Element>()
  for (const start of paras) {
    if (handled.has(start) || !start.parentNode) continue
    const group: Element[] = []
    let cur: Element | null = start
    while (cur && isWordListPara(cur)) {
      group.push(cur)
      cur = cur.nextElementSibling
    }
    if (group.length === 0) continue
    const list = buildWordList(group)
    start.parentNode.insertBefore(list, start)
    for (const item of group) {
      handled.add(item)
      item.remove()
    }
  }
}

function buildWordList(paras: Element[]): Element {
  const doc = paras[0]!.ownerDocument
  const root = doc.createElement(listKind(paras[0]!))
  const stack: { level: number; el: Element }[] = [{ level: 1, el: root }]
  for (const p of paras) {
    const level = listLevel(p)
    const kind = listKind(p)
    while (stack.length > 1 && stack.at(-1)!.level > level) stack.pop()
    if (stack.at(-1)!.level < level) {
      const nested = doc.createElement(kind)
      const lastLi = stack.at(-1)!.el.lastElementChild
      if (lastLi) {
        lastLi.appendChild(nested)
      }
      else {
        const li = doc.createElement('li')
        li.appendChild(nested)
        stack.at(-1)!.el.appendChild(li)
      }
      stack.push({ level, el: nested })
    }
    const top = stack.at(-1)!
    if (top.level === level && top.el.tagName.toLowerCase() !== kind) {
      stack.pop()
      const sibling = doc.createElement(kind)
      top.el.after(sibling)
      stack.push({ level, el: sibling })
    }
    const li = doc.createElement('li')
    li.innerHTML = wordParaInner(p)
    stack.at(-1)!.el.appendChild(li)
  }
  return root
}

function flattenCellBlocks(cell: Element): void {
  const blocks = [...cell.querySelectorAll('p, div, h1, h2, h3, h4, h5, h6')]
  for (const [index, block] of blocks.entries()) {
    if (block.closest('table') !== cell.closest('table')) continue
    if (index < blocks.length - 1) {
      const br = cell.ownerDocument.createElement('br')
      block.parentNode?.insertBefore(br, block.nextSibling)
    }
    unwrapElement(block)
  }
}

interface GridCell {
  html: string
  align: 'left' | 'center' | 'right' | null
}

function tableRows(table: Element): Element[] {
  return [...table.querySelectorAll('tr')].filter((row) => row.closest('table') === table)
}

function expandTableGrid(table: Element): GridCell[][] {
  const rows = tableRows(table)
  const occupancy: boolean[][] = []
  const grid: GridCell[][] = []
  const ensure = (r: number) => {
    occupancy[r] ??= []
    grid[r] ??= []
  }
  rows.forEach((tr, r) => {
    ensure(r)
    let c = 0
    for (const cell of [...tr.children].filter((el) => /^(TD|TH)$/.test(el.tagName))) {
      while (occupancy[r]![c]) c += 1
      flattenCellBlocks(cell)
      const colspan = Math.max(1, Number.parseInt(cell.getAttribute('colspan') ?? '1', 10) || 1)
      const rowspan = Math.max(1, Number.parseInt(cell.getAttribute('rowspan') ?? '1', 10) || 1)
      const payload: GridCell = {
        html: serializeInline(cell),
        align: cellAlign(cell),
      }
      for (let i = 0; i < rowspan; i += 1) {
        ensure(r + i)
        for (let j = 0; j < colspan; j += 1) {
          occupancy[r + i]![c + j] = true
          grid[r + i]![c + j] =
            i === 0 && j === 0 ? payload : { html: '', align: payload.align }
        }
      }
      c += colspan
    }
  })
  const width = Math.max(0, ...grid.map((row) => row.length))
  return grid.map((row) =>
    Array.from(
      { length: width },
      (_, i) => row[i] ?? { html: '', align: null as GridCell['align'] },
    ),
  )
}

function rebuildTable(table: Element): string {
  const nested = [...table.querySelectorAll('table')].filter((inner) => inner !== table)
  for (const inner of nested) {
    const text = (inner.textContent ?? '').replace(/\s+/g, ' ').trim()
    const span = table.ownerDocument.createElement('span')
    span.textContent = text
    inner.replaceWith(span)
  }
  const grid = expandTableGrid(table)
  if (grid.length === 0 || (grid[0]?.length ?? 0) === 0) return ''
  const body = grid
    .map((row, index) => {
      const tag = index === 0 ? 'th' : 'td'
      const cells = row
        .map((cell) => {
          const align = cell.align ? ` align="${cell.align}"` : ''
          return `<${tag}${align}>${cell.html}</${tag}>`
        })
        .join('')
      return `<tr>${cells}</tr>`
    })
    .join('')
  return `<table>${body}</table>`
}

function serializeNoteMath(el: Element): string {
  return (
    pastedMathIsland(el) ??
    mathIslandHtml(
      el.getAttribute('data-tex') ?? '',
      el.classList.contains('note-math-display') || el.classList.contains('math-display'),
    )
  )
}

function serializeNoteWidget(el: Element): string {
  const type = escapeText(el.getAttribute('data-widget') ?? '')
  if (!type) return ''
  const size = escapeText(el.getAttribute('data-size') ?? '2x2')
  const config = el.getAttribute('data-config')
  const configAttr = config ? ` data-config="${escapeText(config)}"` : ''
  return `<div class="note-widget not-prose" data-widget="${type}" data-size="${size}"${configAttr} contenteditable="false"></div>`
}

function serializeInline(root: Element, allowMath = true): string {
  let out = ''
  for (const node of [...root.childNodes]) {
    if (node.nodeType === TEXT) {
      const escaped = escapeText((node.textContent ?? '').replaceAll('\u00A0', ' '))
      out += allowMath ? replaceMathMarkdown(escaped) : escaped
      continue
    }
    if (node.nodeType !== ELEMENT) continue
    const el = node as Element
    const tag = el.tagName.toLowerCase()
    const island = pastedMathIsland(el)
    if (island) {
      out += island
      continue
    }
    if (el.classList.contains('note-math')) {
      out += serializeNoteMath(el)
      continue
    }
    if (tag === 'br') {
      out += '<br>'
      continue
    }
    if (tag === 'img') {
      const src = el.getAttribute('src') ?? el.getAttribute('data-src') ?? ''
      if (!SAFE_IMG.test(src)) continue
      const alt = escapeText(el.getAttribute('alt') ?? '')
      const title = el.getAttribute('title')
      const titleAttr = title ? ` title="${escapeText(title)}"` : ''
      out += `<img src="${escapeText(src)}" alt="${alt}"${titleAttr}>`
      continue
    }
    if (tag === 'a') {
      const href = el.getAttribute('href') ?? ''
      const inner = serializeInline(el)
      if (SAFE_HREF.test(href)) {
        out += `<a href="${escapeText(href)}">${inner}</a>`
      } else {
        out += inner
      }
      continue
    }
    if (tag === 'code' && el.parentElement?.tagName !== 'PRE') {
      out += `<code>${serializeInline(el, false)}</code>`
      continue
    }
    if (tag === 'strong' || tag === 'b') {
      out += `<strong>${serializeInline(el)}</strong>`
      continue
    }
    if (tag === 'em' || tag === 'i') {
      out += `<em>${serializeInline(el)}</em>`
      continue
    }
    if (tag === 'del' || tag === 's' || tag === 'strike') {
      out += `<del>${serializeInline(el)}</del>`
      continue
    }
    if (tag === 'table') {
      out += escapeText((el.textContent ?? '').replace(/\s+/g, ' ').trim())
      continue
    }
    out += serializeInline(el)
  }
  return out.replace(/(?:<br>)+$/g, '').trim()
}

function serializeList(el: Element, tag: 'ul' | 'ol'): string {
  const items = [...el.children]
    .filter((child) => child.tagName === 'LI')
    .map((li) => {
      const nested = [...li.children].filter(
        (child) => child.tagName === 'UL' || child.tagName === 'OL',
      )
      const clone = li.cloneNode(true) as Element
      for (const child of [...clone.children]) {
        if (child.tagName === 'UL' || child.tagName === 'OL') child.remove()
      }
      const own = serializeInline(clone)
      const kids = nested
        .map((child) => serializeList(child, child.tagName === 'OL' ? 'ol' : 'ul'))
        .join('')
      return `<li>${own}${kids}</li>`
    })
    .join('')
  return items ? `<${tag}>${items}</${tag}>` : ''
}

function isInlineish(el: Element): boolean {
  return /^([ABISU]|ABBR|BR|CODE|DEL|EM|IMG|SPAN|STRONG|STRIKE|SUB|SUP)$/.test(el.tagName)
}

function serializeFlow(root: Element): string {
  let out = ''
  let inline = ''
  const flushInline = () => {
    const text = inline.replace(/(?:<br>)+/g, '<br>').replace(/^(?:<br>)+|(?:<br>)+$/g, '')
    if (text.trim()) out += `<p>${text}</p>`
    inline = ''
  }
  for (const node of [...root.childNodes]) {
    if (node.nodeType === TEXT) {
      const text = (node.textContent ?? '').replaceAll('\u00A0', ' ')
      if (text.trim()) inline += replaceMathMarkdown(escapeText(text))
      continue
    }
    if (node.nodeType !== ELEMENT) continue
    const el = node as Element
    const island = pastedMathIsland(el)
    if (
      island &&
      !isDisplayMathEl(el) &&
      el.getAttribute('display') !== 'block'
    ) {
      inline += island
      continue
    }
    if (isInlineish(el) && !el.classList.contains('note-math')) {
      const wrap = el.ownerDocument.createElement('span')
      wrap.appendChild(el.cloneNode(true))
      inline += serializeInline(wrap)
      continue
    }
    if (el.classList.contains('note-math') && el.tagName === 'SPAN') {
      inline += serializeNoteMath(el)
      continue
    }
    flushInline()
    out += serializeBlock(el)
  }
  flushInline()
  return out
}

function serializeBlock(el: Element): string {
  const tag = el.tagName.toLowerCase()
  const island = pastedMathIsland(el)
  if (island) return island
  if (el.classList.contains('note-math')) return serializeNoteMath(el)
  if (el.classList.contains('note-widget')) return serializeNoteWidget(el)
  if (el.classList.contains('note-columns')) {
    const cols = [...el.children]
      .filter((child) => child.classList.contains('note-column'))
      .map((col) => `<div class="note-column">${serializeFlow(col) || '<p><br></p>'}</div>`)
      .join('')
    return cols ? `<div class="note-columns">${cols}</div>` : ''
  }
  if (tag === 'table') return rebuildTable(el)
  if (/^h[1-6]$/.test(tag)) {
    const inner = serializeInline(el)
    return inner ? `<${tag}>${inner}</${tag}>` : ''
  }
  if (tag === 'p') {
    const inner = serializeInline(el)
    return inner ? `<p>${inner}</p>` : ''
  }
  if (tag === 'ul' || tag === 'ol') return serializeList(el, tag)
  if (tag === 'blockquote') {
    const inner = serializeFlow(el)
    return inner ? `<blockquote>${inner}</blockquote>` : ''
  }
  if (tag === 'pre') {
    const code = escapeText(el.textContent ?? '')
    return code ? `<pre><code>${code}</code></pre>` : ''
  }
  if (tag === 'hr') return '<hr>'
  if (tag === 'caption') {
    const inner = serializeInline(el)
    return inner ? `<p>${inner}</p>` : ''
  }
  return serializeFlow(el)
}

/** Word / 网页剪贴板 HTML → 可视层白名单 HTML。 */
export function normalizePastedHtml(html: string): string {
  const fragment = extractClipboardFragment(html).trim()
  if (!fragment) return ''
  const doc = parseHtmlDocument(fragment)
  const root = doc.body
  stripOfficeJunk(root)
  promoteInlineStyles(root)
  convertWordLists(root)
  return serializeFlow(root)
}

export function pastedHtmlToMarkdown(html: string): string {
  const cleaned = normalizePastedHtml(html)
  return cleaned ? visualHtmlToMarkdown(cleaned) : ''
}

/** 可视层：有 HTML 就正规化；否则把 TSV 表转成可视表。 */
export function pastedClipboardToVisualHtml(html: string, plain: string): string | null {
  if (html.trim()) {
    const cleaned = normalizePastedHtml(html)
    if (cleaned) return cleaned
    if (looksLikeOfficeHtml(html) && looksLikePlainTable(plain)) {
      return markdownToVisualHtml(plainTableToMarkdown(plain))
    }
    if (looksLikeOfficeHtml(html) && plain.trim()) {
      return markdownToVisualHtml(plain)
    }
  }
  if (looksLikePlainTable(plain)) return markdownToVisualHtml(plainTableToMarkdown(plain))
  return null
}

/** Markdown 栏：优先富文本，其次 Word/Excel 的制表符表。 */
export function pastedClipboardToMarkdown(html: string, plain: string): string | null {
  if (html.trim() && htmlHasRichBlocks(html)) {
    const md = pastedHtmlToMarkdown(html)
    if (md.trim()) return md
  }
  if (looksLikePlainTable(plain)) return plainTableToMarkdown(plain)
  return null
}
